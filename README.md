# speech2md
音声の文字起こしツール

設計資料は[`docs/superpowers/specs/2026-09-25-speech2md-design.md`](docs/superpowers/specs/2026-09-25-speech2md-design.md)、実装計画は[`docs/superpowers/plans/2026-09-25-speech2md-implementation.md`](docs/superpowers/plans/2026-09-25-speech2md-implementation.md)にあります。

話者割り当ての純粋なドメインモデルはLeanでも検証しています。Windowsでの実行方法、証明済みの性質、Rustテスト用JSONの生成方法は[`formal/README.md`](formal/README.md)を参照してください。

## コミット前チェック

コミット前の軽量チェックには[prek](https://github.com/j178/prek)を使用します。
プロジェクトのRust 1.85では`cargo install prek`をビルドできないため、ビルド済みバイナリを導入する方法を使用してください。

```powershell
uv tool install prek
prek install
```

フックはRust関連ファイルを変更したときだけ、次の検査を実行します。

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

コミットせずに全ファイルを手動検査する場合は、次を実行します。

```powershell
prek run --all-files
```

## ログ

CLIは[`tracing`](https://github.com/tokio-rs/tracing)を使用し、ログを標準エラーへ出力します。
Markdownなどの変換結果を標準出力へ書き出す場合も、ログは変換結果に混ざりません。

既定ではspeech2md自身の`info`以上のログだけを出力し、依存ライブラリのログは出力しません。
`RUST_LOG`を設定すると、ログレベルを全体またはクレート単位で変更できます。
機密情報の混入を防ぐため、`RUST_LOG`で指定しても依存ライブラリのログは出力しません。

```powershell
$env:RUST_LOG = "speech2md=debug"
cargo run -p speech2md-cli

$env:RUST_LOG = "speech2md_runtime=debug,speech2md_cli=info"
cargo run -p speech2md-cli

Remove-Item Env:RUST_LOG
```

無効な`RUST_LOG`を指定した場合、CLIは設定を無視せずエラーで終了します。
ログには音声、文字起こし本文、モデルのURL、完全なファイルパスを記録しません。

## Fuzzing

[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz)で、音声デコーダー境界とCommonMarkレンダラーを検査します。
通常のpre-commitとGitHub Actionsには含めず、時間を区切って手動実行します。

### Windowsの事前準備

Visual Studio Installerから次のコンポーネントを導入します。

- MSVC v143以降のC++ x64/x86ビルドツール
- C++ AddressSanitizer
- Windows 11 SDK

インストール後は、スタートメニューから使用中のVisual Studioに対応する「x64 Native Tools Command Prompt」を起動してください。
通常のPowerShellやコマンドプロンプトでは、別のVisual Studioに含まれる古いリンカーを参照する場合があります。

次のコマンドで、64ビット版のMSVCリンカーを参照していることを確認できます。

```console
where link
```

複数の`link.exe`が表示された場合は、使用するVisual Studioの`Hostx64\x64`以下にあるものが先頭に表示されている必要があります。

nightlyとcargo-fuzzを導入します。

```console
rustup toolchain install nightly
cargo +stable install cargo-fuzz --version 0.13.2 --locked
cargo fuzz --version
```

### Fuzzテストの実行

リポジトリのルートへ移動し、「x64 Native Tools Command Prompt」から実行します。

音声デコーダーでは、既存のWAV、MP3、M4A、音声なしMP4を初期corpusとして使用できます。
次のコマンドはそれぞれ60秒間実行します。

```console
cargo +nightly fuzz run decode_audio fuzz/corpus/decode_audio crates/speech2md-runtime/tests/fixtures -- -max_total_time=60 -max_len=1048576
cargo +nightly fuzz run render_commonmark -- -max_total_time=60 -max_len=65536
```

実行回数を固定して短時間で確認する場合は、`-max_total_time`の代わりに`-runs`を指定します。

```console
cargo +nightly fuzz run decode_audio fuzz/corpus/decode_audio crates/speech2md-runtime/tests/fixtures -- -runs=200 -max_len=1048576
cargo +nightly fuzz run render_commonmark -- -runs=1000 -max_len=65536
```

ビルドだけを確認する場合は次を実行します。

```console
cargo +nightly fuzz check
```

停止時は`Ctrl+C`を入力します。
クラッシュやサニタイザー違反を検出した入力は、`fuzz/artifacts/<ターゲット名>/`に保存されます。
生成されたcorpus、artifact、ビルド成果物はGitの管理対象に含めません。

### Windowsでリンクに失敗する場合

`clang_rt.asan`が見つからない場合は、Visual Studio Installerで「C++ AddressSanitizer」が導入済みか確認します。

`dbghelp.lib`が見つからない場合は、Visual Studio Installerで「Windows 11 SDK」が導入済みか確認します。

必要なコンポーネントが導入済みでも失敗する場合は、開いているシェルを閉じてから「x64 Native Tools Command Prompt」を起動し直してください。
`where link`の先頭が意図したVisual Studioを指していなければ、正しいバージョンの開発者用プロンプトを使用します。
