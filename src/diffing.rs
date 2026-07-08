//! 段落ブロック列同士の差分を取り、HTMLレポート用の「行(左右ペア)」列に変換する。
//!
//! 1. 空白を除去した正規化キーで段落単位のdiff(similarのMyersアルゴリズム)
//! 2. Replaceされた領域は段落を1対1で対応付け、類似度が十分なら文字単位diffで
//!    <del>/<ins> を埋め込んだ「変更」行に、低ければ「削除」+「追加」行に分解

use crate::extract::Block;
use similar::{Algorithm, ChangeTag, DiffOp, TextDiff, TextDiffConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Same,
    Changed,
    Deleted,
    Inserted,
    /// 削除と追加で本文が完全一致(=文書内の移動)
    Moved,
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub loc: String,
    /// エスケープ済みHTML(変更行では <del>/<ins> を含む)
    pub html: String,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub kind: RowKind,
    pub left: Option<Cell>,
    pub right: Option<Cell>,
    /// 移動検出用の正規化キー(削除/追加行のみ)
    key: Option<String>,
}

/// これ未満の類似度のペアは「変更」ではなく「削除+追加」として扱う
const PAIR_RATIO_THRESHOLD: f32 = 0.3;

pub fn diff_blocks(old: &[Block], new: &[Block]) -> Vec<Row> {
    let old_keys: Vec<String> = old.iter().map(|b| normalize(&b.text)).collect();
    let new_keys: Vec<String> = new.iter().map(|b| normalize(&b.text)).collect();
    let old_refs: Vec<&str> = old_keys.iter().map(String::as_str).collect();
    let new_refs: Vec<&str> = new_keys.iter().map(String::as_str).collect();

    // 行数の多いExcel直列化などでもO(N×D)で破綻しないよう、Patience法を使う
    let diff = TextDiffConfig::default()
        .algorithm(Algorithm::Patience)
        .diff_slices(&old_refs, &new_refs);
    let mut rows = Vec::new();

    for op in diff.ops() {
        match *op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                for k in 0..len {
                    rows.push(same_row(&old[old_index + k], &new[new_index + k]));
                }
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                for k in 0..old_len {
                    rows.push(deleted_row(&old[old_index + k]));
                }
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                for k in 0..new_len {
                    rows.push(inserted_row(&new[new_index + k]));
                }
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                let olds = &old[old_index..old_index + old_len];
                let news = &new[new_index..new_index + new_len];
                emit_replace_group(olds, news, &mut rows);
            }
        }
    }
    mark_moves(&mut rows);
    rows
}

/// 本文が完全一致する「削除」と「追加」のペアを「移動」に変換する。
/// 目次や章の並べ替えを、赤/緑ではなく移動として表示するため。
fn mark_moves(rows: &mut [Row]) {
    use std::collections::HashMap;
    let mut inserted_by_key: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, r) in rows.iter().enumerate() {
        if r.kind == RowKind::Inserted {
            if let Some(k) = &r.key {
                inserted_by_key.entry(k.clone()).or_default().push(i);
            }
        }
    }
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        if r.kind == RowKind::Deleted {
            if let Some(k) = &r.key {
                if let Some(list) = inserted_by_key.get_mut(k) {
                    if let Some(j) = list.pop() {
                        pairs.push((i, j));
                    }
                }
            }
        }
    }
    for (i, j) in pairs {
        rows[i].kind = RowKind::Moved;
        rows[j].kind = RowKind::Moved;
    }
}

/// Replace領域内の段落を、順序を保ったまま類似度合計が最大になるように対応付ける。
/// 類似度が閾値未満のペアは作らず、旧側は「削除」、新側は「追加」として出力する。
fn emit_replace_group(olds: &[Block], news: &[Block], rows: &mut Vec<Row>) {
    let (n, m) = (olds.len(), news.len());

    // 領域が大きすぎる場合(シート丸ごと入替等)、全組み合わせの類似度計算は現実的でないため、
    // インデックス順の簡易ペアリングにフォールバックする。
    if n * m > 40_000 {
        let paired = n.min(m);
        for k in 0..paired {
            let (o, nb) = (&olds[k], &news[k]);
            let ratio = pair_ratio(&o.text, &nb.text);
            if ratio >= PAIR_RATIO_THRESHOLD {
                let (lh, rh) = char_diff_html(&o.text, &nb.text);
                rows.push(Row {
                    kind: RowKind::Changed,
                    left: Some(Cell {
                        loc: o.loc.clone(),
                        html: lh,
                    }),
                    right: Some(Cell {
                        loc: nb.loc.clone(),
                        html: rh,
                    }),
                    key: None,
                });
            } else {
                rows.push(deleted_row(o));
                rows.push(inserted_row(nb));
            }
        }
        for k in paired..n {
            rows.push(deleted_row(&olds[k]));
        }
        for k in paired..m {
            rows.push(inserted_row(&news[k]));
        }
        return;
    }

    // 類似度行列(Replace領域は通常小さいので全組み合わせを計算して問題ない)
    let mut sim = vec![vec![0f32; m]; n];
    for i in 0..n {
        for j in 0..m {
            sim[i][j] = pair_ratio(&olds[i].text, &news[j].text);
        }
    }

    // dp[i][j] = olds[..i] と news[..j] の最大類似度合計
    let mut dp = vec![vec![0f32; m + 1]; n + 1];
    for i in 1..=n {
        for j in 1..=m {
            let mut best = dp[i - 1][j].max(dp[i][j - 1]);
            if sim[i - 1][j - 1] >= PAIR_RATIO_THRESHOLD {
                best = best.max(dp[i - 1][j - 1] + sim[i - 1][j - 1]);
            }
            dp[i][j] = best;
        }
    }

    // トレースバックしながら行を組み立てる(後ろから見るので最後に反転)
    let mut out: Vec<Row> = Vec::new();
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        if i > 0
            && j > 0
            && sim[i - 1][j - 1] >= PAIR_RATIO_THRESHOLD
            && (dp[i][j] - (dp[i - 1][j - 1] + sim[i - 1][j - 1])).abs() < f32::EPSILON
        {
            let (lh, rh) = char_diff_html(&olds[i - 1].text, &news[j - 1].text);
            out.push(Row {
                kind: RowKind::Changed,
                left: Some(Cell {
                    loc: olds[i - 1].loc.clone(),
                    html: lh,
                }),
                right: Some(Cell {
                    loc: news[j - 1].loc.clone(),
                    html: rh,
                }),
                key: None,
            });
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j] == dp[i][j - 1]) {
            out.push(inserted_row(&news[j - 1]));
            j -= 1;
        } else {
            out.push(deleted_row(&olds[i - 1]));
            i -= 1;
        }
    }
    out.reverse();
    rows.extend(out);
}

fn same_row(o: &Block, n: &Block) -> Row {
    Row {
        kind: RowKind::Same,
        left: Some(Cell {
            loc: o.loc.clone(),
            html: escape(&o.text),
        }),
        right: Some(Cell {
            loc: n.loc.clone(),
            html: escape(&n.text),
        }),
        key: None,
    }
}

fn deleted_row(o: &Block) -> Row {
    Row {
        kind: RowKind::Deleted,
        left: Some(Cell {
            loc: o.loc.clone(),
            html: escape(&o.text),
        }),
        right: None,
        key: Some(normalize(&o.text)),
    }
}

fn inserted_row(n: &Block) -> Row {
    Row {
        kind: RowKind::Inserted,
        left: None,
        right: Some(Cell {
            loc: n.loc.clone(),
            html: escape(&n.text),
        }),
        key: Some(normalize(&n.text)),
    }
}

/// 長文でも破綻しない類似度計算(先頭一定文字数のみで概算)
fn pair_ratio(a: &str, b: &str) -> f32 {
    const CAP: usize = 1500;
    let ta: String = a.chars().take(CAP).collect();
    let tb: String = b.chars().take(CAP).collect();
    TextDiff::from_chars(ta.as_str(), tb.as_str()).ratio()
}

/// 文字単位のdiffを取り、(旧側HTML, 新側HTML) を返す。
/// 双方が非常に長い場合は文字diffを省略し、全体を変更として塗る。
fn char_diff_html(old: &str, new: &str) -> (String, String) {
    if old.len() + new.len() > 30_000 {
        return (
            format!("<del>{}</del>", escape(old)),
            format!("<ins>{}</ins>", escape(new)),
        );
    }
    let diff = TextDiff::from_chars(old, new);
    let mut l = String::new();
    let mut r = String::new();
    // 連続する同種の変更をまとめてタグ数を減らす
    let mut pending_del = String::new();
    let mut pending_ins = String::new();

    let flush = |l: &mut String, r: &mut String, d: &mut String, i: &mut String| {
        if !d.is_empty() {
            l.push_str("<del>");
            l.push_str(d);
            l.push_str("</del>");
            d.clear();
        }
        if !i.is_empty() {
            r.push_str("<ins>");
            r.push_str(i);
            r.push_str("</ins>");
            i.clear();
        }
    };

    for change in diff.iter_all_changes() {
        let v = escape(change.value());
        match change.tag() {
            ChangeTag::Equal => {
                flush(&mut l, &mut r, &mut pending_del, &mut pending_ins);
                l.push_str(&v);
                r.push_str(&v);
            }
            ChangeTag::Delete => pending_del.push_str(&v),
            ChangeTag::Insert => pending_ins.push_str(&v),
        }
    }
    flush(&mut l, &mut r, &mut pending_del, &mut pending_ins);
    (l, r)
}

/// 段落対応付け用の正規化: 全空白を除去(PDF抽出由来の揺れを吸収)
fn normalize(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}
