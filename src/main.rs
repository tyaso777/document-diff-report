mod diffing;
mod extract;
mod office;
mod report;

use anyhow::{Context, Result};
use clap::Parser;
use pdfium_render::prelude::*;
use std::path::PathBuf;

/// 文書の新旧2版を段落単位で比較し、横並びHTMLレポートを生成します。
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// 旧版の文書
    old: PathBuf,

    /// 新版の文書
    new: PathBuf,

    /// 出力するHTMLファイル
    #[arg(short, long, default_value = "diff_report.html")]
    output: PathBuf,

    /// pdfium 共有ライブラリのあるディレクトリ
    /// (省略時: カレント → 実行ファイルの隣 → システムの順に探索)
    #[arg(long)]
    pdfium_dir: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // PDF入力がある場合のみpdfiumを初期化(Office文書だけならlibpdfium不要)
    let needs_pdf = [&args.old, &args.new].iter().any(|p| is_pdf(p));
    let pdfium = if needs_pdf {
        Some(init_pdfium(args.pdfium_dir.as_deref())?)
    } else {
        None
    };

    eprintln!("旧版を読み込み中: {}", args.old.display());
    let old_blocks = load_blocks(&args.old, pdfium.as_ref())?;
    eprintln!("  → {} ブロック", old_blocks.len());

    eprintln!("新版を読み込み中: {}", args.new.display());
    let new_blocks = load_blocks(&args.new, pdfium.as_ref())?;
    eprintln!("  → {} ブロック", new_blocks.len());

    eprintln!("差分を計算中...");
    let rows = diffing::diff_blocks(&old_blocks, &new_blocks);
    let changes = rows
        .iter()
        .filter(|r| r.kind != diffing::RowKind::Same)
        .count();

    let html = report::render(&args.old, &args.new, &rows);
    std::fs::write(&args.output, html)
        .with_context(|| format!("書き込みに失敗: {}", args.output.display()))?;

    eprintln!("完了: 変更箇所 {} 件 → {}", changes, args.output.display());
    Ok(())
}

fn is_pdf(p: &std::path::Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
}

fn load_blocks(path: &std::path::Path, pdfium: Option<&Pdfium>) -> Result<Vec<extract::Block>> {
    if is_pdf(path) {
        extract::extract_blocks(pdfium.expect("pdfium initialized"), path)
    } else {
        office::extract_office(path)
    }
}

fn init_pdfium(dir: Option<&std::path::Path>) -> Result<Pdfium> {
    let bindings = match dir {
        Some(d) => Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&d))
            .with_context(|| format!("{} に pdfium ライブラリが見つかりません", d.display()))?,
        None => Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&"./"))
            .or_else(|_| {
                let exe_dir = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                    .unwrap_or_default();
                Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&exe_dir))
            })
            .or_else(|_| Pdfium::bind_to_system_library())
            .context(
                "pdfium ライブラリが見つかりません。README の手順でダウンロードし、\
                 実行ファイルと同じディレクトリに置くか --pdfium-dir で指定してください",
            )?,
    };
    Ok(Pdfium::new(bindings))
}
