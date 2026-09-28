# Leanによる純粋モデルの検証

このディレクトリでは、yasumaroの話者割り当て規則とモデルライフサイクルをLean 4でモデル化します。
音声デコード、Whisper、sherpa-onnx、HTTP、ファイルシステム、OSのファイルロックは対象に含めません。

## Windowsでの準備

Lean公式の`elan`をインストールします。

```powershell
curl.exe -O --location https://elan.lean-lang.org/elan-init.ps1
powershell -ExecutionPolicy Bypass -File .\elan-init.ps1
```

新しいPowerShellを開き、バージョンを確認します。

```powershell
lean --version
lake --version
```

このプロジェクトは[`lean-toolchain`](lean-toolchain)でLean 4.34.1を固定しています。

## 検証

```powershell
cd formal
lake build
lake exe YasumaroTests
```

### 話者割り当て

Leanで次の性質を検査しています。

- `TimeSpan`は`startMs <= endMs`の証明を保持し、逆転した区間を`create`が拒否する
- 重なり時間は左右を交換しても変わらない
- 交差しない区間の重なり時間は0になる
- 重なり時間は左側区間の長さを超えない
- 最大重複が同率なら、小さい`SpeakerId`を選ぶ
- 最大重複率が有理数の閾値未満なら話者を割り当てない

### モデルライフサイクル

`ModelLifecycle.lean`は、確定モデルと部分ファイルの検証状態、公開許可、キャンセル要求、共有reader数、排他writerを状態として定義します。検証、キャンセル要求、公開許可、公開は別のeventです。公開許可には、検証済みの部分ファイル、writerの保持、キャンセル要求がないことを要求します。

`ModelLifecycleProofs.lean`は、次の性質を任意の安全な状態について証明します。

- 確定モデルが存在するなら、全体検証済みである
- 検証済みの部分ファイルが存在するなら、部分ファイルも存在する
- 公開許可があるなら、部分ファイルは検証済みでwriterが存在する
- 未検証の部分ファイルと公開許可前にキャンセルされた部分ファイルは公開されない
- 公開許可後のキャンセルは許可を取り消さない
- writerが存在するなら、readerは存在しない
- cleanup成功後に部分ファイルは存在しない
- cleanup失敗は既存の確定モデルと検証状態を変更しない
- 使用中のremoveは状態を変更しない
- remove成功後は確定モデルと部分ファイルが存在しない
- removeを繰り返しても結果は変わらない

壊れた公開許可遷移には、未検証かつキャンセル済みの部分ファイルを許可する固定witnessを置いています。正常遷移が安全条件を保ち、壊れた遷移が安全条件を破ることを同じ定理で検査します。

Leanは抽象状態の遷移を証明します。SHA-256の計算、ファイル削除、symlink、rename、OSのファイルロックはRustの統合テストで検査します。

## Rustテスト用JSON

実行可能なLeanモデルから境界値の期待結果をJSONとして標準出力へ生成します。

```powershell
lake exe testgen
```

ファイルへ保存する場合は次のように実行します。

```powershell
lake exe testgen | Set-Content -Encoding utf8 test-vectors.json
```

Rust側の契約fixtureを更新する場合は、リポジトリルートから次を実行します。

```powershell
Push-Location formal
lake exe testgen | Set-Content -Encoding utf8 ..\crates\yasumaro-core\tests\fixtures\lean-speaker-assignment.json
Pop-Location
```

Rustの統合テストは、このJSONを読み込んでLeanモデルとRust実装の結果を比較します。
Leanの証明はLeanモデルの性質を保証しますが、Rust実装、ネイティブライブラリ、OSの挙動まで保証するものではありません。

### モデルライフサイクルfixture

モデルライフサイクル用のgeneratorは、schema version 1の7 caseをJSONへ出力します。

```powershell
lake exe model-lifecycle-testgen
lake exe model-lifecycle-testgen -- --output ..\crates\yasumaro-runtime\tests\fixtures\lean-model-lifecycle.json
lake exe model-lifecycle-testgen -- --check ..\crates\yasumaro-runtime\tests\fixtures\lean-model-lifecycle.json
```

`--check`は生成結果とコミット済みfixtureが異なる場合にexit 1、不正な引数にexit 2を返します。fixtureの`expected`はLeanの`run`から生成し、Rust側では書き直しません。

Rust oracleはリポジトリルートから実行します。

```console
cargo test -p yasumaro-runtime --example model_lifecycle_oracle --features test-support --locked
cargo run -p yasumaro-runtime --example model_lifecycle_oracle --features test-support -- --strict
cargo run -p yasumaro-runtime --example model_lifecycle_oracle --features test-support -- --case cancel-before-authorization
```

`strict`は通常の公開・検証・removeを扱い、`internal-fixture`は公開許可前後の同期点を使います。`model-only`は壊れたLean遷移の検出能力だけを確認します。oracleは公開APIを通じてfinal、partial、結果分類、leaseとlock解放を観測します。
