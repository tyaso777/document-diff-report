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
    let csv_text = read_text_file(path, "CSV")?;
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

// ---------------------------------------------------------------- txt / html

pub fn extract_txt(path: &Path) -> Result<Vec<Block>> {
    let text = read_text_file(path, "TXT")?;
    Ok(lines_to_paragraphs(text.lines())
        .into_iter()
        .enumerate()
        .map(|(i, text)| Block {
            loc: format!("txt:{}", i + 1),
            text,
        })
        .collect())
}

pub fn extract_html(path: &Path) -> Result<Vec<Block>> {
    let html = read_text_file(path, "HTML")?;
    let text = html_to_text(&html);
    Ok(lines_to_paragraphs(text.lines())
        .into_iter()
        .enumerate()
        .map(|(i, text)| Block {
            loc: format!("html:{}", i + 1),
            text,
        })
        .collect())
}

fn read_text_file(path: &Path, label: &str) -> Result<String> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("{label}を開けませんでした: {}", path.display()))?;
    let decoded = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => {
            let bytes = err.into_bytes();
            let (text, _, had_errors) = encoding_rs::SHIFT_JIS.decode(&bytes);
            if had_errors {
                bail!(
                    "{label}の文字コードを判別できませんでした: {}",
                    path.display()
                );
            }
            text.into_owned()
        }
    };
    Ok(decoded
        .strip_prefix('\u{feff}')
        .unwrap_or(&decoded)
        .to_string())
}

fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut text = String::new();
    let mut chars = html.chars().peekable();
    let mut skip_until: Option<String> = None;

    while let Some(ch) = chars.next() {
        if ch != '<' {
            if skip_until.is_none() {
                text.push(ch);
            }
            continue;
        }

        flush_html_text(&mut out, &mut text);
        let mut tag = String::new();
        for next in chars.by_ref() {
            if next == '>' {
                break;
            }
            tag.push(next);
        }

        let tag_name = html_tag_name(&tag);
        let closing = tag.trim_start().starts_with('/');
        if let Some(skip) = &skip_until {
            if closing && tag_name == *skip {
                skip_until = None;
            }
            continue;
        }

        if !closing && matches!(tag_name.as_str(), "script" | "style" | "noscript") {
            skip_until = Some(tag_name);
            continue;
        }
        if is_html_break_tag(&tag_name) {
            push_html_newline(&mut out);
        }
    }
    flush_html_text(&mut out, &mut text);
    out
}

fn flush_html_text(out: &mut String, text: &mut String) {
    if text.is_empty() {
        return;
    }
    out.push_str(&decode_html_entities(text));
    text.clear();
}

fn html_tag_name(tag: &str) -> String {
    tag.trim_start()
        .trim_start_matches('/')
        .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn is_html_break_tag(tag: &str) -> bool {
    matches!(
        tag,
        "br" | "p"
            | "div"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "main"
            | "aside"
            | "nav"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "li"
            | "ul"
            | "ol"
            | "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "th"
            | "td"
    )
}

fn push_html_newline(out: &mut String) {
    if out.is_empty() {
        return;
    }
    if out.ends_with("\n\n") {
        return;
    }
    if out.ends_with('\n') {
        out.push('\n');
    } else {
        out.push_str("\n\n");
    }
}

fn decode_html_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '&' {
            out.push(ch);
            continue;
        }

        let mut entity = String::new();
        let mut terminated = false;
        while let Some(&next) = chars.peek() {
            chars.next();
            if next == ';' {
                terminated = true;
                break;
            }
            if entity.len() > 12 {
                out.push('&');
                out.push_str(&entity);
                out.push(next);
                entity.clear();
                break;
            }
            entity.push(next);
        }

        match entity.as_str() {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            "nbsp" => out.push(' '),
            _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                if let Ok(code) = u32::from_str_radix(&entity[2..], 16) {
                    if let Some(c) = char::from_u32(code) {
                        out.push(c);
                    }
                }
            }
            _ if entity.starts_with('#') => {
                if let Ok(code) = entity[1..].parse::<u32>() {
                    if let Some(c) = char::from_u32(code) {
                        out.push(c);
                    }
                }
            }
            _ => {
                out.push('&');
                out.push_str(&entity);
                if terminated {
                    out.push(';');
                }
            }
        }
    }
    out
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
        Some("txt") => extract_txt(path),
        Some("html") | Some("htm") => extract_html(path),
        other => bail!("未対応の形式です: {:?}", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
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

    fn temp_path(name: &str, ext: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "document-diff-report-{name}-{}-{unique}.{ext}",
            std::process::id()
        ))
    }

    fn write_bytes(name: &str, ext: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = temp_path(name, ext);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn write_zip_bytes(name: &str, ext: &str, entries: &[(&str, &[u8])]) -> std::path::PathBuf {
        let path = temp_path(name, ext);
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        for (entry_name, bytes) in entries {
            zip.start_file(entry_name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    fn write_minimal_xlsx(name: &str, ext: &str) -> std::path::PathBuf {
        let content_types = br#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
</Types>"#;
        let root_rels = br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#;
        let workbook = br#"<?xml version="1.0" encoding="UTF-8"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
          xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
    <sheet name="Data" sheetId="1" r:id="rId1"/>
  </sheets>
</workbook>"#;
        let workbook_rels = br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>"#;
        let sheet = br#"<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1">
      <c r="A1" t="inlineStr"><is><t>Name</t></is></c>
      <c r="B1" t="inlineStr"><is><t>Amount</t></is></c>
    </row>
    <row r="2">
      <c r="A2" t="inlineStr"><is><t>Alpha</t></is></c>
      <c r="B2"><v>100</v></c>
    </row>
  </sheetData>
</worksheet>"#;

        write_zip_bytes(
            name,
            ext,
            &[
                ("[Content_Types].xml", content_types),
                ("_rels/.rels", root_rels),
                ("xl/workbook.xml", workbook),
                ("xl/_rels/workbook.xml.rels", workbook_rels),
                ("xl/worksheets/sheet1.xml", sheet),
            ],
        )
    }

    #[test]
    fn extracts_utf8_txt() {
        let path = write_bytes(
            "utf8",
            "txt",
            b"First paragraph\r\n\r\nSecond paragraph\r\n",
        );
        let blocks = extract_txt(&path).unwrap();
        std::fs::remove_file(path).ok();

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].loc, "txt:1");
        assert_eq!(blocks[0].text, "First paragraph");
        assert_eq!(blocks[1].text, "Second paragraph");
    }

    #[test]
    fn extracts_utf8_bom_txt_without_bom() {
        let path = write_bytes("utf8-bom", "txt", b"\xef\xbb\xbfFirst paragraph\r\n");
        let blocks = extract_txt(&path).unwrap();
        std::fs::remove_file(path).ok();

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "First paragraph");
        assert!(!blocks[0].text.contains('\u{feff}'));
    }

    #[test]
    fn extracts_cp932_txt() {
        let source = "\u{540d}\u{79f0}\r\n\r\n\u{6771}\u{4eac}\r\n";
        let (encoded, _, had_errors) = encoding_rs::SHIFT_JIS.encode(source);
        assert!(!had_errors);
        let path = write_bytes("cp932", "txt", &encoded);
        let blocks = extract_txt(&path).unwrap();
        std::fs::remove_file(path).ok();

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "\u{540d}\u{79f0}");
        assert_eq!(blocks[1].text, "\u{6771}\u{4eac}");
    }

    #[test]
    fn extracts_html_text_blocks() {
        let html = br#"<!doctype html>
<html><head><style>.x{color:red}</style><script>ignored()</script></head>
<body><h1>Title &amp; Plan</h1><p>First&nbsp;paragraph</p><ul><li>A</li><li>B</li></ul></body></html>"#;
        let path = write_bytes("basic", "html", html);
        let blocks = extract_html(&path).unwrap();
        std::fs::remove_file(path).ok();

        let texts: Vec<&str> = blocks.iter().map(|b| b.text.as_str()).collect();
        assert!(texts.contains(&"Title & Plan"));
        assert!(texts.contains(&"First paragraph"));
        assert!(texts.contains(&"A"));
        assert!(texts.contains(&"B"));
        assert!(!texts.iter().any(|t| t.contains("ignored")));
    }

    #[test]
    fn dispatches_txt_and_html_extensions() {
        let txt = write_bytes("dispatch", "txt", b"Text");
        let htm = write_bytes("dispatch", "htm", b"<p>Html</p>");

        assert_eq!(extract_office(&txt).unwrap()[0].loc, "txt:1");
        assert_eq!(extract_office(&htm).unwrap()[0].loc, "html:1");

        std::fs::remove_file(txt).ok();
        std::fs::remove_file(htm).ok();
    }

    #[test]
    fn extracts_docx_paragraphs() {
        let document_xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>First paragraph</w:t></w:r></w:p>
    <w:p><w:r><w:t>Second paragraph</w:t></w:r></w:p>
  </w:body>
</w:document>"#;
        let path = write_zip_bytes("basic", "docx", &[("word/document.xml", document_xml)]);
        let blocks = extract_docx(&path).unwrap();
        std::fs::remove_file(path).ok();

        let texts: Vec<&str> = blocks.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts, vec!["First paragraph", "Second paragraph"]);
    }

    #[test]
    fn extracts_pptx_slide_text() {
        let slide_xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
       xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:sp><p:txBody><a:bodyPr/><a:lstStyle/>
        <a:p><a:r><a:t>Slide title</a:t></a:r></a:p>
        <a:p><a:r><a:t>Slide body</a:t></a:r></a:p>
      </p:txBody></p:sp>
    </p:spTree>
  </p:cSld>
</p:sld>"#;
        let path = write_zip_bytes("basic", "pptx", &[("ppt/slides/slide1.xml", slide_xml)]);
        let blocks = extract_pptx(&path).unwrap();
        std::fs::remove_file(path).ok();

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].loc, "s.1");
        assert_eq!(blocks[0].text, "Slide title Slide body");
    }

    #[test]
    fn extracts_xlsx_rows() {
        let path = write_minimal_xlsx("basic", "xlsx");
        let blocks = extract_xlsx(&path).unwrap();
        std::fs::remove_file(path).ok();

        assert!(blocks.iter().any(|b| b.loc == "Data"));
        let row1 = blocks.iter().find(|b| b.loc == "Data!1").unwrap();
        assert!(row1.text.contains("A: Name"));
        assert!(row1.text.contains("B: Amount"));
        let row2 = blocks.iter().find(|b| b.loc == "Data!2").unwrap();
        assert!(row2.text.contains("A: Alpha"));
        assert!(row2.text.contains("B: 100"));
    }

    #[test]
    fn extracts_xlsm_rows() {
        let path = write_minimal_xlsx("basic", "xlsm");
        let blocks = extract_xlsx(&path).unwrap();
        std::fs::remove_file(path).ok();

        let row = blocks.iter().find(|b| b.loc == "Data!1").unwrap();
        assert!(row.text.contains("A: Name"));
        assert!(row.text.contains("B: Amount"));
    }

    #[test]
    fn extracts_utf8_bom_html_without_bom() {
        let path = write_bytes("utf8-bom", "html", b"\xef\xbb\xbf<h1>Title</h1>");
        let blocks = extract_html(&path).unwrap();
        std::fs::remove_file(path).ok();

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "Title");
        assert!(!blocks[0].text.contains('\u{feff}'));
    }

    #[test]
    fn extracts_cp932_html() {
        let source = "<p>\u{540d}\u{79f0}</p><p>\u{6771}\u{4eac}</p>";
        let (encoded, _, had_errors) = encoding_rs::SHIFT_JIS.encode(source);
        assert!(!had_errors);
        let path = write_bytes("cp932", "html", &encoded);
        let blocks = extract_html(&path).unwrap();
        std::fs::remove_file(path).ok();

        let texts: Vec<&str> = blocks.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts, vec!["\u{540d}\u{79f0}", "\u{6771}\u{4eac}"]);
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
