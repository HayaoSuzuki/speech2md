# Leanによる話者割り当てモデルの検証

このディレクトリは、yasumaroの純粋な話者割り当て規則をLean 4でモデル化する技術検証です。
音声デコード、Whisper、sherpa-onnx、ファイルシステムなどのI/Oは対象に含めません。

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

現在、Leanで次の性質を検査しています。

- `TimeSpan`は`startMs <= endMs`の証明を保持し、逆転した区間を`create`が拒否する
- 重なり時間は左右を交換しても変わらない
- 交差しない区間の重なり時間は0になる
- 重なり時間は左側区間の長さを超えない
- 最大重複が同率なら、小さい`SpeakerId`を選ぶ
- 最大重複率が有理数の閾値未満なら話者を割り当てない

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
