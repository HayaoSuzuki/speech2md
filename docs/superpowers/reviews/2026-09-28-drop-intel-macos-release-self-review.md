# Apple Intel公式配布終了のセルフレビュー

## 対象

公式Releaseとpull request CIからmacOS Intelを除外する。Windows x86-64、Linux x86-64、macOS Apple Siliconの3対象は維持する。Intel Mac向けのプラットフォーム判定とローカルビルドスクリプトは削除しない。

## 第1回: 要件と配布契約

`.github/workflows/release.yml`のエンジン行列とCLI行列、`.github/workflows/tests.yml`のworkspaceテスト行列を確認した。3行列はいずれもWindows x86-64、Linux x86-64、macOS Apple Siliconだけを含む。`macos-15-intel`は残っていない。

`scripts/release.py`の公式プラットフォームは同じ3対象である。リリーステストでは3個のエンジンarchiveからマニフェストを生成し、1個を欠落させると失敗することを固定した。公開処理はCLI 3個とエンジン3個の計6 archiveを要求する。

README、開発ガイド、テストガイド、エンジン文書、設計仕様、実装計画を確認した。公式配布、CI、成果物数の記述は3対象と6 archiveに統一されている。Intel Mac用のarchiveをGitHub Releaseで提供する記述は残っていない。

## 第2回: 実行時状態とローカルビルド境界

公式配布対象の変更では、Rustの実装、`engines/manifest.json`、CLIと推論エンジンのビルドスクリプトを変更していない。`Platform::MacosX86_64`、`build-cli.py`のDarwin x86-64判定、`build-whisper-engine.sh`のDarwin-x86_64判定は残っている。このため、配布行列からの除外によってローカルarchive生成まで削除されることはない。

公式配布対象の変更は、モデルのinstall、cancel、remove、lease、partial fileへ新しい状態遷移を追加しない。後続の検証で追加したlease修正は、reader解放を暗黙のcloseから明示的なunlockへ変えるだけであり、抽象状態の遷移を変えない。このためLeanモデルは変更していない。既存のLean定理、固定witness、生成fixture、Rust strict oracleを再実行し、状態管理の対応が維持されていることを確認した。

## 第3回: 回帰検査と残存記述

次の検査が成功した。

- `python3 scripts/test_release.py`: 5件成功
- `cargo fmt --all -- --check`
- `cargo check --workspace --all-targets --locked`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `cargo test --workspace --all-features --locked`: 191件成功、5件はローカル資源が必要なためignore
- `lake -d formal build`
- `lake -d formal exe YasumaroTests`
- `lake -d formal exe model-lifecycle-testgen -- --check crates/yasumaro-runtime/tests/fixtures/lean-model-lifecycle.json`
- `cargo run -p yasumaro-runtime --example model_lifecycle_oracle --features test-support --locked -- --strict`: 6 case一致
- `prek run --all-files`
- `git diff --check`

最初の`prek`実行では`whisper_constructor_failure_releases_the_model_lease`が1回失敗した。同じ対象を単独、テストバイナリ全体、workspace全体の順に再実行すると、すべて成功した。しかし、後の`prek`実行では`whisper_transcriber_holds_the_model_lease_until_drop`が同じ`ModelInUse`で失敗したため、偶発的な失敗という判断を撤回した。

`ModelLease`は従来、`File`のcloseによる暗黙のunlockだけに依存していた。`Drop`で明示的にunlockし、closeによる解放も残した。修正後は`whisper_process`の6件を10回反復して全60件が成功した。共有leaseを2本取得し、1本をdropしても残るleaseがremoveを拒否する回帰テストも追加した。

最後に`macos-15-intel`、4構成、8 archive、Intel向け公式配布を示す表現を横断検索した。残る`macos-x86_64`は、ローカルビルド、プラットフォーム解決、または公式対象外であることの説明に限られる。

コミット後の再検索では、旧実装計画にLinuxだけをGitHub-hosted CIで検証する記述が2か所残っていた。公式3対象を各OSのrunnerで検証する記述へ直し、同じ検索を再実行した。
