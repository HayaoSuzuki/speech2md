# 開発ガイド

yasumaroのビルド、変更の検証、配布ファイルの生成、リリース手順をまとめています。
インストールと使い方は[README](../README.md)を参照してください。
以下のコマンドはリポジトリのルートで実行します。

## ソースからのビルド

[rustup](https://www.rust-lang.org/tools/install)を導入し、リポジトリのルートでreleaseバイナリをビルドします。
`rust-toolchain.toml`に指定したRust 1.98.1を使用します。CIも同じバージョンで検証します。

```console
cargo build --release -p yasumaro-cli --locked
```

Windowsでは`target\release\yasumaro.exe`、LinuxとmacOSでは`target/release/yasumaro`が生成されます。
任意のディレクトリへコピーし、そのディレクトリを`PATH`へ追加してください。

文字起こしにはCPU版whisper.cpp、話者分離にはsherpa-onnxを使用します。
リポジトリ内の`engines/manifest.json`には、公開済みのWindows版、Linux x86-64版、macOS Apple Silicon版を登録しています。
これらの環境では、ソースからビルドしたCLIでも`engine install`を実行できます。
ソースから直接ビルドする場合のエンジン登録手順は[エンジンのビルド手順](../engines/README.md)を参照してください。
Release用CLIには、同じReleaseで公開する全OSのエンジン配布情報を埋め込みます。

## コミット前チェック

コミット前チェックには[prek](https://github.com/j178/prek)を使用します。

```powershell
uv tool install prek
prek install
prek run --all-files
```

フックはRust関連ファイルの変更時に`cargo fmt`、厳格な`cargo clippy`、workspaceテストを実行します。
JSON・TOML・YAMLの構文、マージ競合マーカー、ファイル名の大文字・小文字衝突、リンク切れも検査します。
Pythonスクリプトには構文とデバッガーの消し忘れ検査を適用し、1 MiBを超える新規追加ファイルはコミットを拒否します。
モデルや評価音声はGit管理に含めず、小さなテスト用フィクスチャだけをコミットしてください。

行末の余分な空白とファイル末尾の改行は自動修正します。Markdownの改行用スペースは保持し、
`vendor/whisper.cpp-patches/`は空白・改行の自動修正から除外します。
自動修正された場合は差分を確認してステージし直し、再実行してください。

## テストと評価

通常テスト、シャッフルテスト、カバレッジ、Fuzzing、実モデルによる評価は[テストと外部評価](testing.md)を参照してください。
同文書にGitHub Actionsの分担と、VOICEPEAK・SRV-DBを用いた評価方法も記載しています。

モデルライフサイクルを変更した場合は、Lean fixtureのfreshnessとRust oracleも確認します。

```console
lake -d formal exe model-lifecycle-testgen -- --check crates/yasumaro-runtime/tests/fixtures/lean-model-lifecycle.json
cargo test -p yasumaro-runtime --example model_lifecycle_oracle --features test-support --locked
cargo run -p yasumaro-runtime --example model_lifecycle_oracle --features test-support -- --strict
cargo run -p yasumaro-runtime --example model_lifecycle_oracle --features test-support -- --case cancel-before-authorization
```

## 配布ファイルの生成

Linux／macOSでは、対象のOSとCPU上で次のコマンドを実行します。Rustに加えてPython 3.12以降が必要です。

```sh
./scripts/build-posix-cli.sh ./dist
```

`dist/`へ`bin/yasumaro`、README、ライセンス、エンジン配布情報を含むアーカイブとSHA-256ファイルを生成します。
展開後のCLIのバージョン、起動、`doctor`、エンジン配布情報を確認します。推論エンジンとモデルは含みません。
Windowsでは`python scripts/build-cli.py dist`で`.zip`を生成できます。

| 環境 | CLIアーカイブ |
|---|---|
| Windows x86-64 | `yasumaro-v0.1.0-windows-x86_64.zip` |
| Linux x86-64（glibc） | `yasumaro-v0.1.0-linux-x86_64.tar.gz` |
| macOS Apple Silicon | `yasumaro-v0.1.0-macos-aarch64.tar.gz` |
| macOS Intel | `yasumaro-v0.1.0-macos-x86_64.tar.gz` |

例えば、Apple Silicon版は次のように展開して起動できます。

```sh
tar -xzf dist/yasumaro-v0.1.0-macos-aarch64.tar.gz
./bin/yasumaro --help
```

## マージ時の自動リリース

`main`へのPRをマージすると、GitHub Actionsの`Build and release`ワークフローが次の処理を実行します。

1. マージコミットへ`vMAJOR.MINOR.PATCH`タグを付けます。既存の`v`タグの最大バージョンからパッチ番号を1増やします。初回はCLIの`Cargo.toml`のバージョンを使います。
2. Windows x86-64、Linux x86-64、macOS Apple Silicon／Intelのエンジンをビルドし、実モデルで推論とキャンセルを検証します。
3. 各エンジンのサイズ、SHA-256、ReleaseのURLからマニフェストを生成します。
4. タグと同じバージョンおよび生成したマニフェストを各OSのCLIへ埋め込み、ビルド・展開後の起動を検証します。
5. 全構成が成功した場合に、CLIとエンジンの計8アーカイブ、マニフェスト、チェックサムをGitHub Releaseへ登録して公開します。

ソースのバージョンとマニフェストの変更はビルド環境内で行い、`main`への書き戻しはありません。
失敗した実行はGitHub Actionsから再実行できます。同じコミットではタグを再利用し、公開済みReleaseのファイルは置き換えません。
マージせずに閉じたPRでは、タグもReleaseも作りません。
品質検査、workspaceテスト、シャッフルテスト、カバレッジ、リリーススクリプトのテストはPR側のCIで実行し、マージ後のリリースでは繰り返しません。配布物の起動・実推論・チェックサム検証はリリース時にも実行します。
ワークフローの分担は[テストと外部評価](testing.md#github-actionsの構成)を参照してください。

PRの作成・更新と手動実行では、同じビルド処理を公開なしで検証します。ブランチへのpush自体では起動しないため、PR更新とpushによる二重実行はありません。
プレビュー成果物は実行結果のArtifactsから取得でき、14日間保存します。
プレビューのエンジンURLは未公開のため、`engine install`で利用する場合はマージ後のRelease版CLIを使用してください。

## 設計資料

- [設計仕様](superpowers/specs/2026-09-25-yasumaro-design.md)
- [実装計画](superpowers/plans/2026-09-25-yasumaro-implementation.md)
- [モデル削除と失敗時クリーンアップの設計](superpowers/specs/2026-09-28-model-remove-clean-install-design.md)
- [モデル削除と失敗時クリーンアップの実装計画](superpowers/plans/2026-09-28-model-remove-clean-install.md)
- [キャンセル可能なモデル公開の設計改訂](superpowers/specs/2026-09-28-model-remove-clean-install-design.md#キャンセルと強制終了)
- [キャンセル境界と形式対応の実装計画](superpowers/plans/2026-09-28-model-cancellation-formal-correspondence.md)
- [Leanによる話者割り当てとモデルライフサイクルの検証](../formal/README.md)
- [whisper.cppエンジンのビルド](../engines/README.md)

モデルライフサイクルの状態遷移と不変条件を変更した場合は、Rustテストに加えてLeanのbuildと実行テストを確認します。

```console
lake -d formal build
lake -d formal exe YasumaroTests
lake -d formal exe model-lifecycle-testgen -- --check crates/yasumaro-runtime/tests/fixtures/lean-model-lifecycle.json
cargo run -p yasumaro-runtime --example model_lifecycle_oracle --features test-support -- --strict
```

Leanは検証済みモデル、部分ファイル、公開許可、キャンセル要求、reader、writerの抽象状態を扱います。ハッシュ計算、rename、symlink、OSのfile lockはRustの統合テストで検査します。Rust oracleはLean生成fixtureと公開APIの観測結果を比較します。
