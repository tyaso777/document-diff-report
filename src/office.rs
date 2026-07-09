//! Officeドキュメント(docx / pptx / xlsx・xlsm)を、比較用のテキストブロック列に直列化する。
//!
//! 方針:
//! - docx: document.xml の <w:p>(段落)ごとにテキストを抽出(表のセル内段落も同じ流れで拾える)。
//!   その後PDFと同じ段落まとめ処理に通す。位置ラベルは通し番号「¶N」。
//! - pptx: スライドごとに、シェイプ(<p:sp>等)のテキストフレームを1ブロックとして抽出。
//!   位置ラベルは「s.N」(スライド番号)。
//! - xlsx/xlsm: 「1行=1ブロック」で直列化。行番号・シート名は本文に含めず位置ラベルに持たせる
//!   (行挿入で以降の行がすべて偽差分になるのを防ぐため)。数式があるセルは数式を優先して表示。
//!   シート自体の増減が見えるよう、各シートの先頭に「【シート】名前」ブロックを置く。

use crate::extract::{Block, is_cjk, lines_to_paragraphs};
use anyhow::{Context, Result, bail};
use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::Event;
use std::io::Read;
use std::path::Path;

// ---------------------------------------------------------------- docx

pub fn extract_docx(path: &Path) -> Result<Vec<Block>> {
    let xml = read_zip_entry(path, "word/document.xml")?;
    let lines = docx_paragraph_lines(&xml)?;
    let paras = lines_to_paragraphs(lines.iter().map(String::as_str));
    Ok(paras
        .into_iter()
        .enumerate()
        .map(|(i, text)| Block {
            loc: format!("¶{}", i + 1),
            text,
        })
        .collect())
}

/// document.xml から <w:p> 単位のテキスト行を取り出す。
fn docx_paragraph_lines(xml: &str) -> Result<Vec<String>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut lines = Vec::new();
    let mut cur = String::new();
    let mut in_text = false;
    loop {
        match reader.read_event()? {
            Event::Start(e) => match e.name().as_ref() {
                b"w:t" => in_text = true,
                b"w:br" | b"w:tab" => cur.push(' '),
                _ => {}
            },
            Event::End(e) => match e.name().as_ref() {
                b"w:t" => in_text = false,
                b"w:p" => {
                    lines.push(std::mem::take(&mut cur));
                    // 空行として段落境界を明示
                    lines.push(String::new());
                }
                _ => {}
            },
            Event::Text(t) if in_text => cur.push_str(&t.xml_content(XmlVersion::Implicit1_0)?),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(lines)
}

// ---------------------------------------------------------------- pptx

pub fn extract_pptx(path: &Path) -> Result<Vec<Block>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("開けませんでした: {}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)?;

    // ppt/slides/slideN.xml を番号順に集める
    let mut slide_names: Vec<(u32, String)> = Vec::new();
    for i in 0..archive.len() {
        let name = archive.by_index(i)?.name().to_string();
        if let Some(rest) = name.strip_prefix("ppt/slides/slide") {
            if let Some(numstr) = rest.strip_suffix(".xml") {
                if let Ok(n) = numstr.parse::<u32>() {
                    slide_names.push((n, name));
                }
            }
        }
    }
    slide_names.sort();

    let mut blocks = Vec::new();
    for (no, name) in slide_names {
        let mut xml = String::new();
        archive.by_name(&name)?.read_to_string(&mut xml)?;
        for text in pptx_shape_texts(&xml)? {
            blocks.push(Block {
                loc: format!("s.{no}"),
                text,
            });
        }
    }
    Ok(blocks)
}

/// スライドXMLから、テキストフレーム(<p:txBody>)ごとの本文を取り出す。
/// フレーム内の段落(<a:p>)は、CJK同士なら空白なしで連結する。
fn pptx_shape_texts(xml: &str) -> Result<Vec<String>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut texts = Vec::new();
    let mut frame = String::new(); // 現在のtxBody
    let mut para = String::new(); // 現在のa:p
    let mut in_text = false;
    let mut in_frame = false;
    loop {
        match reader.read_event()? {
            Event::Start(e) => match e.name().as_ref() {
                b"p:txBody" => {
                    in_frame = true;
                    frame.clear();
                }
                b"a:t" if in_frame => in_text = true,
                _ => {}
            },
            Event::End(e) => match e.name().as_ref() {
                b"a:t" => in_text = false,
                b"a:p" if in_frame => {
                    let p = para.trim();
                    if !p.is_empty() {
                        join_cjk_aware(&mut frame, p);
                    }
                    para.clear();
                }
                b"p:txBody" => {
                    in_frame = false;
                    let t = frame.trim();
                    if !t.is_empty() {
                        texts.push(t.to_string());
                    }
                }
                _ => {}
            },
            Event::Text(t) if in_text => para.push_str(&t.xml_content(XmlVersion::Implicit1_0)?),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(texts)
}

fn join_cjk_aware(acc: &mut String, next: &str) {
    if acc.is_empty() {
        acc.push_str(next);
        return;
    }
    let last_cjk = acc.chars().last().map(is_cjk).unwrap_or(false);
    let next_cjk = next.chars().next().map(is_cjk).unwrap_or(false);
    if !(last_cjk && next_cjk) {
        acc.push(' ');
    }
    acc.push_str(next);
}

// ---------------------------------------------------------------- xlsx / xlsm

pub fn extract_xlsx(path: &Path) -> Result<Vec<Block>> {
    use calamine::{Data, Reader as _, open_workbook_auto};

    let mut wb = open_workbook_auto(path)
        .with_context(|| format!("ブックを開けませんでした: {}", path.display()))?;
    let sheets = wb.sheet_names().to_vec();
    let mut blocks = Vec::new();

    for sheet in sheets {
        // シート構成の増減が段落diffで見えるように、見出しブロックを置く
        blocks.push(Block {
            loc: sheet.clone(),
            text: format!("【シート】{sheet}"),
        });

        // 数式マップ(絶対セル座標 → 数式)
        let mut formulas = std::collections::HashMap::new();
        if let Ok(frange) = wb.worksheet_formula(&sheet) {
            let (fr, fc) = frange.start().unwrap_or((0, 0));
            for (r, c, f) in frange.used_cells() {
                if !f.is_empty() {
                    formulas.insert((fr + r as u32, fc + c as u32), f.clone());
                }
            }
        }

        let Ok(range) = wb.worksheet_range(&sheet) else {
            continue;
        };
        let (r0, c0) = range.start().unwrap_or((0, 0));
        for (ri, row) in range.rows().enumerate() {
            let abs_row = r0 + ri as u32;
            let mut parts: Vec<String> = Vec::new();
            for (ci, cell) in row.iter().enumerate() {
                let abs_col = c0 + ci as u32;
                let shown = if let Some(f) = formulas.get(&(abs_row, abs_col)) {
                    format!("={f}")
                } else {
                    match cell {
                        Data::Empty => continue,
                        other => other.to_string(),
                    }
                };
                let shown = shown.trim();
                if shown.is_empty() {
                    continue;
                }
                parts.push(format!("{}: {}", col_letter(abs_col), shown));
            }
            if !parts.is_empty() {
                blocks.push(Block {
                    loc: format!("{}!{}", sheet, abs_row + 1),
                    text: parts.join(" ┃ "),
                });
            }
        }
    }
    Ok(blocks)
}

fn col_letter(mut c: u32) -> String {
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (c % 26) as u8) as char);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s
}

// ---------------------------------------------------------------- 共通

pub fn extract_csv(path: &Path) -> Result<Vec<Block>> {
    let csv_text = read_csv_text(path)?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(csv_text.as_bytes());
    let mut blocks = Vec::new();

    for (row_index, record) in reader.records().enumerate() {
        let record =
            record.with_context(|| format!("CSVの{}行目を読み込めませんでした", row_index + 1))?;
        let mut parts = Vec::new();
        for (col_index, field) in record.iter().enumerate() {
            let shown = field.trim();
            if shown.is_empty() {
                continue;
            }
            parts.push(format!("{}: {}", col_letter(col_index as u32), shown));
        }
        if !parts.is_empty() {
            blocks.push(Block {
                loc: format!("csv:{}", row_index + 1),
                text: parts.join(" ┃ "),
            });
        }
    }

    Ok(blocks)
}

fn read_csv_text(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("CSVを開けませんでした: {}", path.display()))?;
    let decoded = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => {
            let bytes = err.into_bytes();
            let (text, _, had_errors) = encoding_rs::SHIFT_JIS.decode(&bytes);
            if had_errors {
                bail!("CSVの文字コードを判別できませんでした: {}", path.display());
            }
            text.into_owned()
        }
    };
    Ok(decoded
        .strip_prefix('\u{feff}')
        .unwrap_or(&decoded)
        .to_string())
}

fn read_zip_entry(path: &Path, entry: &str) -> Result<String> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("開けませんでした: {}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut s = String::new();
    archive
        .by_name(entry)
        .with_context(|| format!("{} 内に {entry} がありません", path.display()))?
        .read_to_string(&mut s)?;
    Ok(s)
}

/// 拡張子に応じた抽出器のディスパッチ(pdfはmain側で処理)
pub fn extract_office(path: &Path) -> Result<Vec<Block>> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("docx") => extract_docx(path),
        Some("pptx") => extract_pptx(path),
        Some("xlsx") | Some("xlsm") => extract_xlsx(path),
        Some("csv") => extract_csv(path),
        other => bail!("未対応の形式です: {:?}", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_csv_path(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "document-diff-report-{name}-{}-{unique}.csv",
            std::process::id()
        ))
    }

    fn write_csv_bytes(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = temp_csv_path(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn extracts_utf8_csv() {
        let path = write_csv_bytes("utf8", "名称,金額\r\n東京,100\r\n".as_bytes());
        let blocks = extract_csv(&path).unwrap();
        std::fs::remove_file(path).ok();

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].loc, "csv:1");
        assert_eq!(blocks[0].text, "A: 名称 ┃ B: 金額");
        assert_eq!(blocks[1].text, "A: 東京 ┃ B: 100");
    }

    #[test]
    fn extracts_utf8_bom_csv_without_bom_in_first_cell() {
        let path = write_csv_bytes("utf8-bom", b"\xef\xbb\xbfname,amount\r\nalpha,100\r\n");
        let blocks = extract_csv(&path).unwrap();
        std::fs::remove_file(path).ok();

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "A: name ┃ B: amount");
        assert!(!blocks[0].text.contains('\u{feff}'));
    }

    #[test]
    fn extracts_cp932_csv() {
        let (encoded, _, had_errors) = encoding_rs::SHIFT_JIS.encode("名称,金額\r\n東京,100\r\n");
        assert!(!had_errors);
        let path = write_csv_bytes("cp932", &encoded);
        let blocks = extract_csv(&path).unwrap();
        std::fs::remove_file(path).ok();

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "A: 名称 ┃ B: 金額");
        assert_eq!(blocks[1].text, "A: 東京 ┃ B: 100");
    }
}
