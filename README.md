# document-diff-report

文書の新旧2版を、**段落単位のテキスト/構造比較**にかけ、
対応する段落が横並びになる**単一ファイルのHTMLレポート**を生成するCLIツールです。

対応形式: **PDF / docx / pptx / xlsx / xlsm**(新旧は同じ形式同士で比較してください)

## レポートの機能

- 旧版(左)/新版(右)の2カラム。対応する段落は常に同じ行に横並び
- 変更行は黄色、追加は緑、削除は赤。段落内の変更は文字単位で `del`/`ins` ハイライト
- 各段落に元文書内の位置ラベル(PDF: p.3、pptx: s.2、xlsx: Sheet!7 など)
- ヘッダに変更/追加/削除の件数サマリ
- 「変更箇所のみ表示」トグル
- 「前へ/次へ」ボタンと `n` / `p` キーで変更箇所間をジャンプ
- CSS/JS埋め込みの単一HTMLなので、そのままメール添付・共有可能

## セットアップ

### 1. pdfium 共有ライブラリを配置

テキスト抽出に [PDFium](https://github.com/bblanchon/pdfium-binaries) を使います
(日本語CIDフォントのPDFでも安定して抽出できるため)。

```sh
# Linux x64 の例
curl -LO https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-linux-x64.tgz
tar xzf pdfium-linux-x64.tgz lib/libpdfium.so
cp lib/libpdfium.so .   # プロジェクト直下 or 実行ファイルの隣に置く
```

- macOS (Apple Silicon): `pdfium-mac-arm64.tgz` → `lib/libpdfium.dylib`
- Windows: `pdfium-win-x64.tgz` → `bin\pdfium.dll`

置き場所は「カレントディレクトリ」「実行ファイルと同じディレクトリ」「システムライブラリパス」
の順に探索されます。任意の場所を使う場合は `--pdfium-dir <DIR>` で指定してください。

### 2. ビルド

```sh
cargo build --release
```

PDFを比較しない場合(docx/pptx/xlsxのみ)、pdfiumライブラリは不要です。

このプロジェクトは Rust 1.88 以降を前提にしています。依存ライブラリは `Cargo.lock` で
更新済みの解決結果を固定しています。

## 使い方

```sh
document-diff-report old.pdf new.pdf -o diff_report.html
open diff_report.html   # ブラウザで開く
```

```
Usage: document-diff-report [OPTIONS] <OLD> <NEW>

Arguments:
  <OLD>  旧版の文書
  <NEW>  新版の文書

Options:
  -o, --output <OUTPUT>          出力するHTMLファイル [default: diff_report.html]
      --pdfium-dir <PDFIUM_DIR>  pdfium 共有ライブラリのあるディレクトリ
```

## Office文書の扱い

- **docx** — `document.xml` の段落(表のセル内も含む)を抽出し、PDFと同じ段落diffに載せます。
  位置ラベルは通し段落番号(¶N)。
- **pptx** — スライドごとにテキストフレーム単位でブロック化します。位置ラベルはスライド番号(s.N)。
  図形・レイアウト・画像の変更はテキストに現れないため検出できません。
- **xlsx / xlsm** — 「1行=1ブロック」で直列化します(例: `B: 提出日 ┃ D: 入力必須…`)。
  行番号・シート名は本文に含めず位置ラベル(`シート名!行`)に持たせるため、行の挿入で
  以降の行が偽差分になりません。数式があるセルは数式を表示するので、審査ロジックの変更が
  数式差分として見えます。シートの増減は「【シート】名前」ブロックの追加/削除として現れます。
  ※ .xlsm のマクロ(VBA)はバイナリ格納のため比較対象外です。

複数ペアの一括処理はシェルループで可能です:

```sh
for name in 様式1 様式2 様式3; do
  document-diff-report "5次_${name}.xlsx" "6次_${name}.xlsx" -o "差分_${name}.html"
done
```

## 仕組み

1. **抽出** (`src/extract.rs`) — PDFiumで全ページのテキストを取り出し、
   空行・箇条書き記号・「第◯」見出し・文末記号(。/:)を手がかりに段落ブロックへ分割。
   日本語の折返し行は空白を挟まずに連結し、PDF抽出特有の不要スペースを抑制します。
   ページ番号のみの段落は除外し、目次のドットリーダー(……)は圧縮してノイズを抑えます。
2. **比較** (`src/diffing.rs`) — 空白を除去した正規化テキストで段落単位のdiff
   (`similar` クレート / Myers法)。Replaceされた領域内は、段落同士の類似度が
   合計最大になるようDPで順序保存マッチングし、類似度0.3未満のペアは
   「削除+追加」に分解。対応が付いたペアは文字単位diffでハイライトを生成します。
   さらに、本文が完全一致する削除・追加の組は「移動」(青)として表示し、
   章の並べ替えが赤/緑のノイズにならないようにしています。
3. **出力** (`src/report.rs`) — CSS Gridで左右セルを同一行に固定した
   自己完結型HTMLを書き出します。

## チューニングポイント

- `diffing.rs` の `PAIR_RATIO_THRESHOLD`(既定 0.3):
  上げると「変更」判定が厳しくなり、削除+追加に分かれやすくなります。
- `extract.rs` の `starts_new_block` / `is_heading` / `ends_block`:
  対象文書の書式(箇条書き記号、見出しパターン)に合わせて調整してください。
- ヘッダ・フッタ(ページ番号など)がノイズになる場合は、`extract_blocks` 内で
  ページ上下端のテキストを座標(`page.text().segments()`)で除外する拡張が有効です。

## 既知の制約

- スキャンPDF(画像のみ)はテキストが取れないため比較できません。事前にOCRが必要です。
- 表はセルの読み取り順がPDF依存のため、行単位の細かい差分には向きません。
- 2段組レイアウトは読み取り順が乱れることがあります。
