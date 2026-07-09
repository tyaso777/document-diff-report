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
.cell .pg {{ cursor: pointer; }}
.cell .pg:hover {{ background: var(--navy); color: #fff; border-color: var(--navy); }}
.row.selected .cell:not(.empty) {{ outline: 2px dashed #4a6ea9; outline-offset: -2px; }}
.row.selected .cell .pg {{ background: #4a6ea9; color: #fff; border-color: #4a6ea9; }}
#selbar {{
  position: fixed; bottom: 18px; left: 50%; transform: translateX(-50%);
  display: none; align-items: center; gap: 10px; z-index: 20;
  background: var(--navy); color: #f2efe8; border-radius: 6px;
  padding: 8px 16px; box-shadow: 0 4px 16px rgba(0,0,0,.3);
  font-family: "Hiragino Kaku Gothic ProN", sans-serif; font-size: 13px;
}}
#selbar.on {{ display: flex; }}
#selbar button {{
  background: transparent; color: inherit; cursor: pointer;
  border: 1px solid rgba(255,255,255,.45); border-radius: 4px; padding: 3px 12px; font-size: 13px;
}}
#selbar button:hover {{ background: rgba(255,255,255,.12); }}
#selbar button.done {{ background: var(--ins-ink); border-color: var(--ins-ink); }}
#selbar button.fail {{ background: var(--del-ink); border-color: var(--del-ink); }}
.copybar {{
  position: absolute; top: 4px; right: 4px; display: none; gap: 4px; z-index: 5;
}}
.cell:hover .copybar, .copybar:hover {{ display: flex; }}
.copybar button {{
  font-family: "Hiragino Kaku Gothic ProN", sans-serif;
  font-size: 10.5px; padding: 1px 8px; cursor: pointer;
  background: var(--navy); color: #f2efe8;
  border: none; border-radius: 3px; opacity: .85;
}}
.copybar button:hover {{ opacity: 1; }}
.copybar button.done, .controls button.done {{ background: var(--ins-ink); border-color: var(--ins-ink); }}
.copybar button.fail, .controls button.fail {{ background: var(--del-ink); border-color: var(--del-ink); color: #fff; }}
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
    <button id="copy-tsv" title="全変更を 種別/旧位置/旧/新位置/新 のTSVでコピー(Excel貼付用)">変更一覧をコピー</button>
  </div>
</header>
<div class="colheads">
  <div class="old">旧版: {old_name}</div>
  <div class="new">新版: {new_name}</div>
</div>
<main>
{body}
</main>
<footer>document-diff-report により生成 / n・p キーで変更箇所を移動 /
位置ラベルをクリックで行を選択し、複数行まとめてコピーできます(Excelには5列、メモ帳にはテキストで貼り付き)</footer>
<div id="selbar"><span id="selcount"></span><button id="selcopy">コピー</button><button id="selclear">解除</button></div>
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

  // --- コピー機能 ---
  function copyText(text, btn) {{
    function show(label, cls) {{
      if (!btn.getAttribute('data-label')) btn.setAttribute('data-label', btn.textContent);
      btn.textContent = label;
      btn.classList.add(cls);
      setTimeout(function () {{
        btn.textContent = btn.getAttribute('data-label');
        btn.classList.remove('done'); btn.classList.remove('fail');
      }}, 1600);
    }}
    function legacy() {{
      var ta = document.createElement('textarea');
      ta.value = text;
      ta.setAttribute('readonly', '');
      ta.style.position = 'fixed'; ta.style.top = '0'; ta.style.left = '0'; ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.focus(); ta.select();
      ta.setSelectionRange(0, ta.value.length);
      var ok = false;
      try {{ ok = document.execCommand('copy'); }} catch (e) {{ ok = false; }}
      document.body.removeChild(ta);
      show(ok ? '✓ コピー済み' : '✗ コピー不可', ok ? 'done' : 'fail');
    }}
    try {{
      if (window.isSecureContext && navigator.clipboard && navigator.clipboard.writeText) {{
        navigator.clipboard.writeText(text).then(
          function () {{ show('✓ コピー済み', 'done'); }},
          function () {{ legacy(); }}
        );
      }} else {{
        legacy();
      }}
    }} catch (e) {{ legacy(); }}
  }}

  function cellText(cell) {{
    if (!cell || cell.classList.contains('empty')) return '';
    var clone = cell.cloneNode(true);
    var trims = Array.prototype.slice.call(clone.querySelectorAll('.pg, .copybar'));
    for (var i = 0; i < trims.length; i++) {{ trims[i].parentNode.removeChild(trims[i]); }}
    return (clone.textContent || '').trim();
  }}

  function cellLoc(cell) {{
    var pg = cell && cell.querySelector('.pg');
    return pg ? pg.textContent.trim() : '';
  }}

  var bar = document.createElement('div');
  bar.className = 'copybar';
  var bCell = document.createElement('button');
  bCell.textContent = 'コピー';
  var bPair = document.createElement('button');
  bPair.textContent = '旧→新';
  bar.appendChild(bCell); bar.appendChild(bPair);
  document.querySelector('main').addEventListener('mouseover', function (e) {{
    var cell = e.target.closest('.cell');
    if (!cell || cell.classList.contains('empty')) return;
    var row = cell.closest('.row');
    bPair.style.display = (row.classList.contains('changed') || row.classList.contains('moved')) ? '' : 'none';
    if (bar.parentNode !== cell) cell.appendChild(bar);
  }});
  bCell.addEventListener('click', function (e) {{
    e.stopPropagation();
    copyText(cellText(bar.closest('.cell')), bCell);
  }});

  function rowKind(row) {{
    var kinds = {{ changed: '変更', deleted: '削除', inserted: '追加', moved: '移動', same: '同一' }};
    for (var k in kinds) {{ if (row.classList.contains(k)) return kinds[k]; }}
    return '';
  }}

  function rowData(row) {{
    var l = row.children[0], r = row.children[1];
    return {{ kind: rowKind(row), lloc: cellLoc(l), ltext: cellText(l), rloc: cellLoc(r), rtext: cellText(r) }};
  }}

  function escHtml(t) {{
    return t.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  }}

  function rowsPlain(datas) {{
    return datas.map(function (d) {{
      return '【' + d.kind + '】' + d.lloc + ' → ' + d.rloc + '\n旧: ' + d.ltext + '\n新: ' + d.rtext;
    }}).join('\n\n');
  }}

  function rowsHtmlTable(datas) {{
    var trs = datas.map(function (d) {{
      return '<tr><td>' + escHtml(d.lloc) + '</td><td>' + escHtml(d.ltext) +
             '</td><td>' + escHtml(d.rloc) + '</td><td>' + escHtml(d.rtext) + '</td></tr>';
    }}).join('');
    return '<table>' + trs + '</table>';
  }}

  function copyRich(plain, htmlStr, btn) {{
    function show(label, cls) {{
      if (!btn.getAttribute('data-label')) btn.setAttribute('data-label', btn.textContent);
      btn.textContent = label; btn.classList.add(cls);
      setTimeout(function () {{
        btn.textContent = btn.getAttribute('data-label');
        btn.classList.remove('done'); btn.classList.remove('fail');
      }}, 1600);
    }}
    function legacyRich() {{
      var handler = function (e) {{
        e.clipboardData.setData('text/plain', plain);
        e.clipboardData.setData('text/html', htmlStr);
        e.preventDefault();
      }};
      document.addEventListener('copy', handler);
      var ta = document.createElement('textarea');
      ta.value = plain; ta.setAttribute('readonly', '');
      ta.style.position = 'fixed'; ta.style.top = '0'; ta.style.opacity = '0';
      document.body.appendChild(ta); ta.focus(); ta.select();
      var ok = false;
      try {{ ok = document.execCommand('copy'); }} catch (e) {{ ok = false; }}
      document.removeEventListener('copy', handler);
      document.body.removeChild(ta);
      show(ok ? '✓ コピー済み' : '✗ コピー不可', ok ? 'done' : 'fail');
    }}
    try {{
      if (window.isSecureContext && navigator.clipboard && navigator.clipboard.write && window.ClipboardItem) {{
        var item = new ClipboardItem({{
          'text/plain': new Blob([plain], {{ type: 'text/plain' }}),
          'text/html': new Blob([htmlStr], {{ type: 'text/html' }})
        }});
        navigator.clipboard.write([item]).then(
          function () {{ show('✓ コピー済み', 'done'); }},
          function () {{ legacyRich(); }}
        );
      }} else {{ legacyRich(); }}
    }} catch (e) {{ legacyRich(); }}
  }}

  bPair.addEventListener('click', function (e) {{
    e.stopPropagation();
    var d = [rowData(bar.closest('.row'))];
    copyRich(rowsPlain(d), rowsHtmlTable(d), bPair);
  }});

  var selected = [];
  var selbar = document.getElementById('selbar');
  var selcount = document.getElementById('selcount');
  function updateSelbar() {{
    selcount.textContent = selected.length + ' 行選択中';
    selbar.classList.toggle('on', selected.length > 0);
  }}
  document.querySelector('main').addEventListener('click', function (e) {{
    var pg = e.target.closest('.pg');
    if (!pg) return;
    var row = pg.closest('.row');
    var i = selected.indexOf(row);
    if (i >= 0) {{ selected.splice(i, 1); row.classList.remove('selected'); }}
    else {{ selected.push(row); row.classList.add('selected'); }}
    updateSelbar();
  }});
  document.getElementById('selcopy').addEventListener('click', function () {{
    if (!selected.length) return;
    var ordered = Array.prototype.slice.call(document.querySelectorAll('.row.selected'));
    var datas = ordered.map(rowData);
    copyRich(rowsPlain(datas), rowsHtmlTable(datas), this);
  }});
  document.getElementById('selclear').addEventListener('click', function () {{
    for (var i = 0; i < selected.length; i++) selected[i].classList.remove('selected');
    selected = [];
    updateSelbar();
  }});
  document.getElementById('copy-tsv').addEventListener('click', function () {{
    var kinds = {{ changed: '変更', deleted: '削除', inserted: '追加', moved: '移動' }};
    var lines = ['種別\t旧位置\t旧\t新位置\t新'];
    Array.prototype.slice.call(document.querySelectorAll('.row.chg, .row.moved')).forEach(function (row) {{
      var kind = Object.keys(kinds).filter(function (k) {{ return row.classList.contains(k); }})[0];
      var l = row.children[0], r = row.children[1];
      function tsv(s) {{ return s.replace(/[\t\n]/g, ' '); }}
      lines.push([kinds[kind] || '', cellLoc(l), tsv(cellText(l)), cellLoc(r), tsv(cellText(r))].join('\t'));
    }});
    copyText(lines.join('\n'), this);
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
