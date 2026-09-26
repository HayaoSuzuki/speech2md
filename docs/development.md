# 開発ガイド

speech2mdのビルド、変更の検証、配布ファイルの生成、リリース手順をまとめています。
インストールと使い方は[README](../README.md)を参照してください。
以下のコマンドはリポジトリのルートで実行します。

## ソースからのビルド

[Rust 1.85以降](https://www.rust-lang.org/tools/install)を導入し、リポジトリのルートでreleaseバイナリをビルドします。

```console
cargo build --release -p speech2md-cli --locked
```

Windowsでは`target\release\speech2md.exe`、LinuxとmacOSでは`target/release/speech2md`が生成されます。
任意のディレクトリへコピーし、そのディレクトリを`PATH`へ追加してください。

文字起こしにはCPU版whisper.cpp、話者分離にはsherpa-onnxを使用します。
リポジトリ内の`engines/manifest.json`は既存のWindows版を参照します。
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

## テストと評価

通常テスト、シャッフルテスト、カバレッジ、Fuzzing、実モデルによる評価は[テストと外部評価](testing.md)を参照してください。
同文書にGitHub Actionsの分担と、VOICEPEAK・SRV-DBを用いた評価方法も記載しています。

## 配布ファイルの生成

Linux／macOSでは、対象のOSとCPU上で次のコマンドを実行します。Rustに加えてPython 3.12以降が必要です。

```sh
./scripts/build-posix-cli.sh ./dist
```

`dist/`へ`bin/speech2md`、README、ライセンス、エンジン配布情報を含むアーカイブとSHA-256ファイルを生成します。
展開後のCLIのバージョン、起動、`doctor`、エンジン配布情報を確認します。推論エンジンとモデルは含みません。
Windowsでは`python scripts/build-cli.py dist`で`.zip`を生成できます。

| 環境 | CLIアーカイブ |
|---|---|
| Windows x86-64 | `speech2md-v0.1.0-windows-x86_64.zip` |
| Linux x86-64（glibc） | `speech2md-v0.1.0-linux-x86_64.tar.gz` |
| macOS Apple Silicon | `speech2md-v0.1.0-macos-aarch64.tar.gz` |
| macOS Intel | `speech2md-v0.1.0-macos-x86_64.tar.gz` |

例えば、Apple Silicon版は次のように展開して起動できます。

```sh
tar -xzf dist/speech2md-v0.1.0-macos-aarch64.tar.gz
./bin/speech2md --help
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

- [設計仕様](superpowers/specs/2026-09-25-speech2md-design.md)
- [実装計画](superpowers/plans/2026-09-25-speech2md-implementation.md)
- [Leanによる話者割り当てモデル](../formal/README.md)
- [whisper.cppエンジンのビルド](../engines/README.md)
