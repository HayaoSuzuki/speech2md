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

`ModelLifecycle.lean`は、検証済みの確定モデル、部分ファイル、共有reader数、排他writerの状態遷移を定義します。`publishVerified`は、Rust実装が行うサイズ・SHA-256検証とrenameを一つにまとめた抽象イベントです。Leanがハッシュ計算やrenameの成否を検証するわけではありません。

`ModelLifecycleProofs.lean`は、次の性質を任意の安全な状態について証明します。

- 確定モデルが存在するなら、全体検証済みである
- writerが存在するなら、readerは存在しない
- cleanup成功後に部分ファイルは存在しない
- cleanup失敗は既存の確定モデルと検証状態を変更しない
- 使用中のremoveは状態を変更しない
- remove成功後は確定モデルと部分ファイルが存在しない
- removeを繰り返しても結果は変わらない

ファイル削除、symlink、rename、OSのファイルロックはRustの統合テストで検査します。Leanの定理は、Rust実装そのものを証明するものではありません。

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
