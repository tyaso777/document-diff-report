//! 差分行の列から、単一ファイルで完結するインタラクティブなHTMLレポートを生成する。
//!
//! - 対応する段落が必ず同じ高さの行に横並びになる(CSS Grid)
//! - 「変更箇所のみ表示」トグル
//! - 前へ/次へボタンとキーボード(n / p)で変更箇所をジャンプ
//! - 依存なし(CSS/JS埋め込み)なのでそのまま共有できる

use crate::diffing::{Row, RowKind, escape};
use std::path::Path;

pub fn render(old_path: &Path, new_path: &Path, rows: &[Row]) -> String {
    let old_name = escape(&old_path.file_name().unwrap_or_default().to_string_lossy());
    let new_name = escape(&new_path.file_name().unwrap_or_default().to_string_lossy());

    let changed = rows.iter().filter(|r| r.kind == RowKind::Changed).count();
    let deleted = rows.iter().filter(|r| r.kind == RowKind::Deleted).count();
    let inserted = rows.iter().filter(|r| r.kind == RowKind::Inserted).count();
    let moved = rows.iter().filter(|r| r.kind == RowKind::Moved).count() / 2;
    let total_changes = changed + deleted + inserted;

    let mut body = String::new();
    for row in rows {
        let class = match row.kind {
            RowKind::Same => "row same",
            RowKind::Changed => "row chg changed",
            RowKind::Deleted => "row chg deleted",
            RowKind::Inserted => "row chg inserted",
            RowKind::Moved => "row moved",
        };
        body.push_str(&format!("<div class=\"{class}\">"));
        body.push_str(&cell_html(&row.left));
        body.push_str(&cell_html(&row.right));
        body.push_str("</div>\n");
    }

    format!(
        r#"<!DOCTYPE html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>文書差分レポート: {old_name} → {new_name}</title>
<style>
:root {{
  --bg: #eceae4;
  --paper: #fffdf8;
  --ink: #26241f;
  --ink-soft: #6f6a5e;
  --rule: #d8d4c8;
  --navy: #1f3a5f;
  --del-bg: #fbe9e7;
  --del-ink: #b3261e;
  --ins-bg: #e4f2e6;
  --ins-ink: #1b6e35;
  --chg-bg: #fdf6df;
  --focus: #c8871e;
}}
* {{ box-sizing: border-box; }}
html {{ scroll-padding-top: 96px; }}
body {{
  margin: 0;
  background: var(--bg);
  color: var(--ink);
  font-family: "Hiragino Mincho ProN", "Yu Mincho", "BIZ UDMincho", serif;
  line-height: 1.75;
  font-size: 15px;
}}
header {{
  position: sticky; top: 0; z-index: 10;
  background: var(--navy); color: #f2efe8;
  padding: 10px 20px;
  display: flex; flex-wrap: wrap; align-items: center; gap: 14px;
  box-shadow: 0 2px 8px rgba(0,0,0,.25);
  font-family: "Hiragino Kaku Gothic ProN", "Yu Gothic", sans-serif;
}}
header h1 {{ font-size: 15px; font-weight: 600; margin: 0; letter-spacing: .04em; }}
.stats {{ display: flex; gap: 8px; font-size: 12px; }}
.stats span {{ padding: 2px 8px; border-radius: 3px; background: rgba(255,255,255,.14); }}
.stats .s-chg {{ box-shadow: inset 3px 0 0 var(--focus); }}
.stats .s-ins {{ box-shadow: inset 3px 0 0 #7fd396; }}
.stats .s-del {{ box-shadow: inset 3px 0 0 #f0938c; }}
.stats .s-mov {{ box-shadow: inset 3px 0 0 #9db4d8; }}
.controls {{ margin-left: auto; display: flex; align-items: center; gap: 10px; font-size: 13px; }}
.controls label {{ display: flex; align-items: center; gap: 5px; cursor: pointer; user-select: none; }}
.controls button {{
  background: transparent; color: inherit;
  border: 1px solid rgba(255,255,255,.45); border-radius: 4px;
  padding: 3px 12px; font-size: 13px; cursor: pointer;
}}
.controls button:hover {{ background: rgba(255,255,255,.12); }}
.controls button:focus-visible {{ outline: 2px solid #fff; outline-offset: 1px; }}
#counter {{ min-width: 64px; text-align: center; font-variant-numeric: tabular-nums; }}
.colheads {{
  position: sticky; top: 48px; z-index: 9;
  display: grid; grid-template-columns: 1fr 1fr; gap: 16px;
  max-width: 1400px; margin: 0 auto; padding: 8px 20px;
  background: var(--bg);
  font-family: "Hiragino Kaku Gothic ProN", "Yu Gothic", sans-serif;
  font-size: 12.5px; color: var(--ink-soft); letter-spacing: .06em;
}}
.colheads div {{ border-bottom: 2px solid var(--navy); padding: 2px 6px 4px; }}
.colheads .new {{ border-bottom-color: var(--ins-ink); }}
main {{ max-width: 1400px; margin: 0 auto; padding: 4px 20px 80px; }}
.row {{
  display: grid; grid-template-columns: 1fr 1fr; gap: 16px;
  margin-bottom: 2px;
}}
.cell {{
  background: var(--paper);
  border: 1px solid var(--rule);
  padding: 8px 12px 8px 14px;
  position: relative;
  overflow-wrap: anywhere;
}}
.cell .pg {{
  float: right; margin: 0 0 2px 10px;
  font-family: "Hiragino Kaku Gothic ProN", sans-serif;
  font-size: 10.5px; color: var(--ink-soft);
  border: 1px solid var(--rule); border-radius: 3px; padding: 0 5px;
}}
.cell.empty {{
  background: repeating-linear-gradient(-45deg, transparent 0 6px, rgba(0,0,0,.03) 6px 12px);
  border-style: dashed;
}}
.row.same .cell {{ color: #4b473e; }}
.row.changed .cell {{ background: var(--chg-bg); border-left: 3px solid var(--focus); }}
.row.deleted .cell:not(.empty) {{ background: var(--del-bg); border-left: 3px solid var(--del-ink); }}
.row.inserted .cell:not(.empty) {{ background: var(--ins-bg); border-left: 3px solid var(--ins-ink); }}
.row.deleted .cell:not(.empty) {{ color: var(--del-ink); }}
.row.inserted .cell:not(.empty) {{ color: var(--ins-ink); }}
.row.moved .cell:not(.empty) {{
  background: #e9eef7; border-left: 3px solid #4a6ea9; color: #33507e;
}}
.row.moved .cell:not(.empty)::before {{
  content: "移動"; float: right; margin: 0 0 2px 8px;
  font-family: "Hiragino Kaku Gothic ProN", sans-serif;
  font-size: 10px; color: #4a6ea9; border: 1px solid #9db4d8; border-radius: 3px; padding: 0 5px;
}}
body.only-changes .row.moved {{ display: none; }}
body.only-changes.show-moved .row.moved {{ display: grid; }}
del {{ background: #f6cbc6; color: var(--del-ink); text-decoration: line-through; text-decoration-thickness: 1px; border-radius: 2px; padding: 0 1px; }}
ins {{ background: #bfe6c7; color: var(--ins-ink); text-decoration: none; border-radius: 2px; padding: 0 1px; }}
.row.current .cell {{ outline: 2px solid var(--focus); outline-offset: -1px; }}
body.only-changes .row.same {{ display: none; }}
body.only-changes main {{ font-size: 14px; }}
.gap-note {{ display: none; }}
body.only-changes .gap-note {{
  display: block; text-align: center; color: var(--ink-soft);
  font-size: 11px; margin: 10px 0; letter-spacing: .3em;
}}
footer {{ text-align: center; color: var(--ink-soft); font-size: 12px; padding: 20px; }}
@media (max-width: 860px) {{
  .row, .colheads {{ grid-template-columns: 1fr; gap: 4px; }}
}}
@media (prefers-reduced-motion: no-preference) {{
  html {{ scroll-behavior: smooth; }}
}}
</style>
</head>
<body>
<header>
  <h1>文書差分レポート</h1>
  <div class="stats">
    <span class="s-chg">変更 {changed}</span>
    <span class="s-ins">追加 {inserted}</span>
    <span class="s-del">削除 {deleted}</span>
    <span class="s-mov" title="本文が同一のまま位置が変わった段落">移動 {moved}</span>
  </div>
  <div class="controls">
    <label><input type="checkbox" id="only-chg">変更箇所のみ表示</label>
    <button id="prev" title="前の変更へ (p)">◀ 前へ</button>
    <span id="counter">– / {total_changes}</span>
    <button id="next" title="次の変更へ (n)">次へ ▶</button>
  </div>
</header>
<div class="colheads">
  <div class="old">旧版: {old_name}</div>
  <div class="new">新版: {new_name}</div>
</div>
<main>
{body}
</main>
<footer>document-diff-report により生成 / n・p キーで変更箇所を移動できます</footer>
<script>
(function () {{
  var changes = Array.prototype.slice.call(document.querySelectorAll('.row.chg'));
  var idx = -1;
  var counter = document.getElementById('counter');

  function go(i) {{
    if (!changes.length) return;
    if (idx >= 0) changes[idx].classList.remove('current');
    idx = (i + changes.length) % changes.length;
    var el = changes[idx];
    el.classList.add('current');
    el.scrollIntoView({{ block: 'center' }});
    counter.textContent = (idx + 1) + ' / ' + changes.length;
  }}

  document.getElementById('next').addEventListener('click', function () {{ go(idx + 1); }});
  document.getElementById('prev').addEventListener('click', function () {{ go(idx - 1); }});
  document.addEventListener('keydown', function (e) {{
    if (e.target.tagName === 'INPUT') return;
    if (e.key === 'n') go(idx + 1);
    if (e.key === 'p') go(idx - 1);
  }});
  document.getElementById('only-chg').addEventListener('change', function (e) {{
    document.body.classList.toggle('only-changes', e.target.checked);
    if (idx >= 0) changes[idx].scrollIntoView({{ block: 'center' }});
  }});
}})();
</script>
</body>
</html>
"#
    )
}

fn cell_html(cell: &Option<crate::diffing::Cell>) -> String {
    match cell {
        Some(c) => format!(
            "<div class=\"cell\"><span class=\"pg\">{}</span>{}</div>",
            crate::diffing::escape(&c.loc),
            c.html
        ),
        None => "<div class=\"cell empty\"></div>".to_string(),
    }
}
