# セキュリティ・脆弱性とライセンスの整理

対象: document-diff-report v0.1.0(PDF / docx / pptx / xlsx / xlsm / csv 対応版)
実施日: 2026-07-10

監査コマンド:

```sh
cargo audit
cargo metadata --format-version 1 --no-deps
```

`cargo audit` は cargo-audit 0.22.2 で実行し、RustSec advisory-db 1,159件のアドバイザリに照合しました。

## 1. 脆弱性監査の結果

現在の `Cargo.lock` では、`cargo audit` による RustSec 脆弱性報告はありません。

過去に問題になり得た XML / ZIP 周辺の依存は、現在は以下のように更新済みです。

- `quick-xml 0.41`
- `calamine 0.36`
- `zip 6.0`

`zip` は `default-features = false` とし、Office Open XML の読み取りに必要な `deflate` のみを有効化しています。これにより、不要な圧縮形式や暗号関連の依存を避けています。

## 2. CSV対応による変化

CSV対応で追加した直接依存は以下です。

| クレート | 用途 | ライセンス |
|---|---|---|
| `csv` | CSVパース | MIT OR Unlicense |
| `encoding_rs` | UTF-8 / UTF-8 BOM / CP932(Shift_JIS) デコード | Apache-2.0 OR MIT |

セキュリティ面の増分リスクは、PDFやOffice文書の解析に比べると小さいです。

- CSVパースはRust製ライブラリで処理します。
- CSVはファイル全体をメモリに読み込んでから解析するため、非常に大きなCSVではメモリ使用量が増えます。
- 文字コード判定は「UTF-8として読めなければCP932/Shift_JISとして読む」という単純な方式です。その他のレガシー文字コードは未対応です。
- CSVセル内容は、他形式から抽出したテキストと同じくHTML出力時にエスケープされます。

## 3. 脅威モデル

主な攻撃面は「信頼できない入力ファイルの解析」です。

1. **PDF解析**

   PDF抽出は libpdfium に依存します。libpdfium はC++製ネイティブライブラリであり、Rust側のメモリ安全性保証は及びません。

   対策: PDFiumバイナリを定期的に更新し、可能ならチェックサムを確認してください。不特定多数のPDFを処理する場合は、コンテナ等で隔離実行するのが安全です。

2. **ZIPベースのOffice形式**

   docx / pptx / xlsx / xlsm はZIPコンテナです。現在の実装では、ZIPエントリの展開後サイズ上限を明示的には設けていません。

   将来対応: 信頼できないファイルを大量処理する用途では、エントリごとの展開サイズ上限を入れるべきです。

3. **XML解析**

   `quick-xml` は外部実体を解決しないため、典型的なXXEによる外部ファイル読み取りは想定していません。現在のバージョンは `cargo audit` でも指摘なしです。

4. **生成HTML**

   ファイル名、位置ラベル、抽出テキストはHTMLに埋め込む前にエスケープしています。生成HTMLは単一ファイルで、外部リソースは読み込みません。

   ただし、レポートにはナビゲーションとコピー機能のためのJavaScriptを含みます。生成HTMLは入力ファイルから作られるローカル成果物として扱ってください。

## 4. ライセンスの棚卸し

直接依存はすべてパーミッシブライセンスです。

| クレート | 用途 | ライセンス |
|---|---|---|
| `anyhow` | エラー処理 | MIT OR Apache-2.0 |
| `clap` | CLI引数パース | MIT OR Apache-2.0 |
| `pdfium-render` | PDFium Rustラッパー | MIT OR Apache-2.0 |
| `similar` | テキストdiff | Apache-2.0 |
| `zip` | Office ZIPコンテナ読み取り | MIT |
| `quick-xml` | docx/pptx XML解析 | MIT |
| `calamine` | xlsx/xlsm読み取り | MIT |
| `csv` | CSVパース | MIT OR Unlicense |
| `encoding_rs` | CSV文字コードfallback | Apache-2.0 OR MIT |

直接依存としてGPL/LGPL等のコピーレフトライセンスは意図的に導入していません。

## 5. libpdfiumを同梱配布する場合

PDF比較に必要な `pdfium.dll` / `libpdfium.so` / `libpdfium.dylib` を配布物に同梱する場合は、Rustクレートとは別にPDFium側のライセンス表示が必要です。

- `pdfium-render`: MIT OR Apache-2.0
- bblanchon/pdfium-binaries のラッパー情報: MIT
- PDFium本体: BSD-3-Clause
- PDFium同梱サードパーティ: FreeType、ICU、libjpeg-turbo、libpng、libtiff、OpenJPEG、zlib、abseil 等

PDFiumバイナリを同梱する場合は、PDFium配布物の `licenses/` フォルダまたは同等の第三者ライセンス表示を添付してください。

## 6. 本プロジェクト自身のライセンス

現時点では、このリポジトリ自身のライセンスが未宣言です。

公開・再利用・配布を想定するなら、Rustプロジェクトで一般的な以下の形を推奨します。

- `Cargo.toml` に `license = "MIT OR Apache-2.0"` を追加
- `LICENSE-MIT` を追加
- `LICENSE-APACHE` を追加

## 7. 推奨アクション

1. `cargo audit` をCIに組み込む
2. Rust stable と依存クレートを定期的に更新する
3. libpdfium のリリースを別途監視し、定期的に更新する
4. 信頼できないOfficeファイルを大量処理する用途では、ZIP展開サイズ上限を実装する
5. 配布前にプロジェクト自身のライセンスを宣言する
