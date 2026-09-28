# Apple Intel公式配布終了のセルフレビュー

## 対象

公式Releaseとpull request CIからmacOS Intelを除外する。Windows x86-64、Linux x86-64、macOS Apple Siliconの3対象は維持する。Intel Mac向けのプラットフォーム判定とローカルビルドスクリプトは削除しない。

## 第1回: 要件と配布契約

`.github/workflows/release.yml`のエンジン行列とCLI行列、`.github/workflows/tests.yml`のworkspaceテスト行列を確認した。3行列はいずれもWindows x86-64、Linux x86-64、macOS Apple Siliconだけを含む。`macos-15-intel`は残っていない。

`scripts/release.py`の公式プラットフォームは同じ3対象である。リリーステストでは3個のエンジンarchiveからマニフェストを生成し、1個を欠落させると失敗することを固定した。公開処理はCLI 3個とエンジン3個の計6 archiveを要求する。

README、開発ガイド、テストガイド、エンジン文書、設計仕様、実装計画を確認した。公式配布、CI、成果物数の記述は3対象と6 archiveに統一されている。Intel Mac用のarchiveをGitHub Releaseで提供する記述は残っていない。

## 第2回: 実行時状態とローカルビルド境界

Rustの実装、`engines/manifest.json`、CLIと推論エンジンのビルドスクリプトに差分がないことを確認した。`Platform::MacosX86_64`、`build-cli.py`のDarwin x86-64判定、`build-whisper-engine.sh`のDarwin-x86_64判定は残っている。このため、配布行列からの除外によってローカルarchive生成まで削除されることはない。

今回の変更はモデルのinstall、cancel、remove、lease、partial fileを変更しない。新しい状態遷移もないためLeanモデルは変更していない。既存のLean定理、固定witness、生成fixture、Rust strict oracleを再実行し、状態管理の対応が維持されていることを確認した。

## 第3回: 回帰検査と残存記述

次の検査が成功した。

- `python3 scripts/test_release.py`: 5件成功
- `cargo fmt --all -- --check`
- `cargo check --workspace --all-targets --locked`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `cargo test --workspace --all-features --locked`: 190件成功、5件はローカル資源が必要なためignore
- `lake -d formal build`
- `lake -d formal exe YasumaroTests`
- `lake -d formal exe model-lifecycle-testgen -- --check crates/yasumaro-runtime/tests/fixtures/lean-model-lifecycle.json`
- `cargo run -p yasumaro-runtime --example model_lifecycle_oracle --features test-support --locked -- --strict`: 6 case一致
- `prek run --all-files`
- `git diff --check`

最初の`prek`実行では`whisper_constructor_failure_releases_the_model_lease`が1回失敗した。同じ対象を単独、テストバイナリ全体、workspace全体の順に再実行すると、すべて成功した。続けて`prek run --all-files`を再実行し、全hookの成功を確認した。このテストとlease実装には今回の差分がなく、再現する変更起因の不具合は確認できなかった。

最後に`macos-15-intel`、4構成、8 archive、Intel向け公式配布を示す表現を横断検索した。残る`macos-x86_64`は、ローカルビルド、プラットフォーム解決、または公式対象外であることの説明に限られる。
