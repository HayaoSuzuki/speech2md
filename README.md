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
