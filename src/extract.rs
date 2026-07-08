//! PDFからテキストを抽出し、比較単位となる「段落ブロック」に分割する。
//!
//! 日本語文書を想定し、以下のヒューリスティクスで段落をまとめる:
//! - 空行は段落境界
//! - 箇条書き記号・番号・「第◯章」などで始まる行は新しい段落
//! - 直前の行が「。」「:」などで終わっていたら段落境界
//! - 行の連結時、日本語(CJK)同士なら空白を挟まず連結(PDFの折返し由来の不要スペースを防ぐ)

use anyhow::{Context, Result};
use pdfium_render::prelude::*;
use std::path::Path;

/// 比較の最小単位。ページ番号(1始まり)と本文を持つ。
#[derive(Debug, Clone)]
pub struct Block {
    /// 位置ラベル(PDF: "p.3", pptx: "s.12", xlsx: "シート名!行7" など)
    pub loc: String,
    pub text: String,
}

/// PDFを読み込み、全ページの段落ブロック列を返す。
pub fn extract_blocks(pdfium: &Pdfium, path: &Path) -> Result<Vec<Block>> {
    let doc = pdfium
        .load_pdf_from_file(path, None)
        .with_context(|| format!("PDFを開けませんでした: {}", path.display()))?;

    let mut blocks = Vec::new();
    for (idx, page) in doc.pages().iter().enumerate() {
        let page_no = idx as u16 + 1;
        let text = page
            .text()
            .with_context(|| {
                format!(
                    "{} の {} ページ目のテキスト抽出に失敗",
                    path.display(),
                    page_no
                )
            })?
            .all();
        for para in lines_to_paragraphs(text.lines()) {
            blocks.push(Block {
                loc: format!("p.{page_no}"),
                text: para,
            });
        }
    }
    Ok(blocks)
}

/// 行の集合を段落にまとめる。
pub(crate) fn lines_to_paragraphs<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut paras: Vec<String> = Vec::new();
    let mut cur = String::new();

    let flush = |paras: &mut Vec<String>, cur: &mut String| {
        let t = cur.trim();
        if !t.is_empty() && !is_page_number(t) {
            paras.push(collapse_dot_leaders(t));
        }
        cur.clear();
    };

    for raw in lines {
        let line = raw.trim();
        if line.is_empty() {
            flush(&mut paras, &mut cur);
            continue;
        }
        if !cur.is_empty() && (starts_new_block(line) || ends_block(&cur)) {
            flush(&mut paras, &mut cur);
        }
        append_line(&mut cur, line);
        // 「第1 事業の目的」のような短い見出し行は、それ単体で1ブロックにする
        if is_heading(line) {
            flush(&mut paras, &mut cur);
        }
    }
    flush(&mut paras, &mut cur);
    paras
}

/// この行から新しい段落が始まるとみなすか。
fn starts_new_block(line: &str) -> bool {
    let mut chars = line.chars();
    let Some(c) = chars.next() else { return false };
    if matches!(
        c,
        '・' | '■'
            | '●'
            | '○'
            | '◆'
            | '▶'
            | '※'
            | '【'
            | '〔'
            | '('
            | '\u{FF08}'
            | '<'
            | '\u{FF1C}'
            | '「'
    ) {
        return true;
    }
    if ('①'..='⑳').contains(&c) {
        return true;
    }
    // "1." "2)" "(1)" などの番号始まり
    if c.is_ascii_digit() || ('\u{FF10}'..='\u{FF19}').contains(&c) {
        return true;
    }
    // 「第1章」「第2 公募内容」など
    if c == '第' {
        if let Some(d) = chars.next() {
            if d.is_ascii_digit()
                || ('\u{FF10}'..='\u{FF19}').contains(&d)
                || matches!(
                    d,
                    '一' | '二' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十'
                )
            {
                return true;
            }
        }
    }
    false
}

/// 「第1 事業の目的」「1. 補助対象者」のような短い見出し行か。
fn is_heading(line: &str) -> bool {
    if line.chars().count() > 25 {
        return false;
    }
    let mut chars = line.chars();
    match chars.next() {
        Some('第') => chars
            .next()
            .map(|d| d.is_ascii_digit() || ('\u{FF10}'..='\u{FF19}').contains(&d))
            .unwrap_or(false),
        Some(c) if c.is_ascii_digit() => {
            // "1." "2." "3 " のような番号のみで始まり、文末記号で終わらない行
            !line.ends_with('。')
        }
        _ => false,
    }
}

/// これまでの内容が段落として完結しているとみなすか。
fn ends_block(cur: &str) -> bool {
    matches!(
        cur.chars().last(),
        Some('。') | Some(':') | Some('\u{FF1A}') | Some('】') | Some('」')
    )
}

/// 行を連結する。CJK同士は空白なし、それ以外(英数字など)は半角スペースで繋ぐ。
fn append_line(cur: &mut String, line: &str) {
    if cur.is_empty() {
        cur.push_str(line);
        return;
    }
    let last_cjk = cur.chars().last().map(is_cjk).unwrap_or(false);
    let next_cjk = line.chars().next().map(is_cjk).unwrap_or(false);
    if !(last_cjk && next_cjk) {
        cur.push(' ');
    }
    cur.push_str(line);
}

pub(crate) fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{3000}'..='\u{303F}' // CJK記号・句読点
        | '\u{3040}'..='\u{30FF}' // ひらがな・カタカナ
        | '\u{31F0}'..='\u{31FF}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}' // CJK統合漢字
        | '\u{F900}'..='\u{FAFF}'
        | '\u{FF00}'..='\u{FFEF}' // 全角英数・半角カナ
    )
}

/// ページ番号と思われる、数字のみ(1〜4桁)の段落か。
fn is_page_number(t: &str) -> bool {
    let n = t.chars().count();
    n >= 1 && n <= 4 && t.chars().all(|c| c.is_ascii_digit())
}

/// 目次のドットリーダー(「......」)を「……」に圧縮し、文字単位diffのノイズを抑える。
fn collapse_dot_leaders(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut dots = 0usize;
    for c in t.chars() {
        if c == '.' || c == '\u{2026}' || c == '\u{FF0E}' {
            dots += 1;
            continue;
        }
        if dots > 0 {
            if dots >= 3 {
                out.push_str("\u{2026}\u{2026}");
            } else {
                for _ in 0..dots {
                    out.push('.');
                }
            }
            dots = 0;
        }
        out.push(c);
    }
    if dots >= 3 {
        out.push_str("\u{2026}\u{2026}");
    } else {
        for _ in 0..dots {
            out.push('.');
        }
    }
    out
}
