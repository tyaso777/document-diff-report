# セキュリティ・脆弱性とライセンスの整理

対象: document-diff-report v0.1.0(PDF / docx / pptx / xlsx / xlsm 対応版)
実施日: 2026-07-09
監査手段: `cargo audit` v0.22.1(RustSec advisory-db、1,159件のアドバイザリ)、`cargo metadata` によるライセンス棚卸し

---

## 1. 脆弱性監査の結果

### 解消済み

| ID | クレート | 内容 | 対処 |
|---|---|---|---|
| RUSTSEC-2026-0009 | time 0.3.36(zip経由) | スタック枯渇によるDoS(Medium 6.8) | `zip` を `default-features = false, features = ["deflate"]` に変更し、time・bzip2・zstd・AES暗号のネイティブ依存ごと排除(OOXMLの読み取りにはdeflateのみで十分) |

### 残存(要対応)

| ID | クレート | 深刻度 | 内容 |
|---|---|---|---|
| RUSTSEC-2026-0194 | quick-xml 0.31.0 | High 7.5 | 属性名の重複チェックが二次時間になり、細工されたXMLでCPUを占有できる(DoS) |
| RUSTSEC-2026-0195 | quick-xml 0.31.0 | High 7.5 | `NsReader` の名前空間宣言の割当が無制限で、メモリ枯渇DoSが可能 |

- **経路**: 本ツール自身のdocx/pptx解析、および `calamine`(xlsx読み取り)の内部利用。
- **影響の性質**: いずれも「細工されたファイルを開くとハング/クラッシュする」DoSであり、
  任意コード実行や情報漏えいではない。ローカルCLIとして自分の入手したファイルを比較する
  用途では実害は限定的だが、**第三者から受け取ったファイルを自動処理するサーバ用途等では対応必須**。
- **恒久対応**: quick-xml >= 0.41 へ更新(calamineも最新へ)。ただし新版は新しいRustを要求する
  (下記「サプライチェーン」参照)。
- **暫定緩和**: 信頼できない出所のファイルを処理しない/`ulimit -v` 等でメモリ・CPU制限を
  かけたプロセスで実行する。

## 2. 脅威モデル(このツール固有の注意点)

主な攻撃面は「**信頼できない入力ファイルの解析**」です。

1. **libpdfium(最大のリスク)** — C++製ネイティブライブラリで、PDF解析はメモリ安全性の
   バグが歴史的に多い領域(Chromiumで継続的にCVEが出る)。Rust側の安全性保証は及ばない。
   → 対策: bblanchon/pdfium-binaries の**最新リリースを定期的に取得**する運用にする。
   バイナリ入手時はリリースページのチェックサム確認を推奨。不特定多数のPDFを処理する場合は
   コンテナ等で隔離実行する。
2. **zip爆弾** — docx/pptx/xlsxの実体はzip。現在の実装は展開サイズの上限チェックを
   していないため、高圧縮の悪意あるファイルでメモリを消費させられる(既知の制限)。
   → 将来対応: エントリごとの展開サイズ上限(例: 100MB)を入れる。
3. **XML(XXE等)** — quick-xml は外部実体を解決しないためXXE(外部ファイル読み出し)は
   構造的に発生しない。上記RUSTSECのDoS 2件が現状の残リスク。
4. **生成HTMLのXSS** — 抽出テキスト・位置ラベル・ファイル名はすべて `escape()`
   (`& < > "`)を通して要素内容または二重引用符属性にのみ埋め込んでおり、
   ファイル内容経由のスクリプト注入は緩和済み。レポートは外部リソースを一切読み込まない
   単一ファイルで、閲覧時の通信は発生しない。

## 3. サプライチェーン上の注意

- **Cargo.lock が古いバージョンに固定されている**ことが最大の技術的負債。
  これは開発環境(Ubuntu 24.04のapt版 Rust 1.75)でビルドするための措置で、
  quick-xml等のセキュリティ修正版は新しいRustを要求するため取り込めていない。
  → **本番運用では rustup の最新stableを使い、`cargo update` で最新化した上で
  `cargo audit` をCIに組み込む**ことを強く推奨(固定を外せば残存2件は解消可能)。
- bindgen / clang-sys 等はビルド時のみ使われ、実行バイナリには含まれない。

## 4. ライセンスの棚卸し

### Rustクレート(依存115件)

すべてパーミッシブライセンスで、**GPL/LGPL等のコピーレフトは一切含まれない**。

| ライセンス | 件数 | 備考 |
|---|---|---|
| MIT OR Apache-2.0(表記ゆれ含む) | 90 | Rustエコシステム標準のデュアル |
| MIT | 11 | zip, quick-xml, calamine 等 |
| BSD-3-Clause | 1 | bindgen(ビルド時のみ) |
| ISC | 1 | libloading |
| Unlicense OR MIT | 3 | memchr 等 |
| その他(0BSD/Zlib/Unicode-3.0等の選択式) | 9 | いずれもパーミッシブ |

### libpdfium バイナリ(実行時に同梱・配布する場合)

- ラッパー(bblanchon/pdfium-binaries): MIT
- PDFium本体: BSD-3-Clause(`licenses/pdfium.txt`)
- 同梱サードパーティ: FreeType(FTL)、ICU、libjpeg-turbo、libpng、libtiff、
  OpenJPEG、zlib、abseil 等 — いずれもパーミッシブ。配布物の `licenses/` フォルダを
  そのまま同梱すれば表示義務を満たせる。

### 配布時の義務(社内利用のみなら実質不要)

- バイナリを社外配布する場合: MIT/BSD/Apache-2.0 の**著作権表示とライセンス文の同梱**が必要。
  `cargo about` や `cargo license` でTHIRD-PARTY-NOTICES を自動生成できる。
  libpdfiumを同梱するなら上記 `licenses/` フォルダも添付する。
- Apache-2.0 には明示的な特許ライセンス条項があり、利用側に有利。
- **本プロジェクト自身のライセンスが未設定**。公開・配布の予定があれば、Rust慣習の
  「MIT OR Apache-2.0」デュアルを `Cargo.toml` の `license` フィールドと
  LICENSE-MIT / LICENSE-APACHE ファイルで宣言することを推奨。

## 5. 推奨アクション(優先順)

1. 本番ビルドは最新stable Rustで行い、依存を最新化して `cargo audit` をクリーンにする(残存2件が解消)
2. `cargo audit` をCIに組み込み、libpdfiumの更新をリリース監視で追う
3. zip展開サイズの上限チェックを実装する(不特定のファイルを扱うなら必須)
4. プロジェクトのライセンスを宣言する(配布予定がある場合)
