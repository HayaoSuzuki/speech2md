# テストと外部評価

VOICEPEAKで作成する自作音声による話者分離テストは、[VOICEPEAK話者分離テスト原稿](../test-data/voicepeak/README.md)を参照してください。
生成した10本のWAVに対し、Whisperを使わず話者分離だけを連続評価できます。

```powershell
$env:SPEECH2MD_VOICEPEAK_DIR = (Resolve-Path "samples\voicepeak")
cargo test -p speech2md-runtime --test voicepeak_eval --locked -- --ignored --nocapture
Remove-Item Env:SPEECH2MD_VOICEPEAK_DIR
```

テストはケースごとの検出話者数、クラスタごとの発話時間、処理時間、実時間係数を1個のJSONとして標準出力へ書きます。
期待話者数と検出話者数の不一致は観測値としてJSONへ記録し、それだけでテストを失敗させません。
本文の欠落と異常終了は後段のLLMで回復できないため、テストの失敗条件です。

## 通常のテスト

モデルとネットワークを使わないテストは、Windowsのローカル環境とLinuxのGitHub Actionsで実行します。

```console
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

実行順への依存は、nightlyのシャッフル機能で検査します。

```console
cargo +nightly test --workspace --all-features --locked -- -Z unstable-options --shuffle --test-threads=1
```

話者割り当てのfuzz targetは、時刻付きトークンと話者区間の任意の組み合わせに対し、本文が欠落せず時刻順に保たれることを検査します。

```console
cargo +nightly fuzz run assign_speakers
```

Windowsで`STATUS_DLL_NOT_FOUND`が発生する場合は、Visual StudioのMSVC x64ディレクトリにある`clang_rt.asan_dynamic-x86_64.dll`を`PATH`から参照できるDeveloper PowerShellで実行します。

`speech2md-core`と`speech2md-formats`は、行と関数のカバレッジを100%に保ちます。
runtimeとCLIでは、ネイティブエンジン、OSエラー、プロセス終了タイミングなどの外部境界を実装内テストだけで網羅できないため、fake engine、ignored実モデルテスト、外部評価を併用します。

## SRV-DBの利用範囲

外部評価には、電気通信大学 高橋弘太研究室が公開する[話速バリエーション型音声データベース（SRV-DB）](https://www.it.cei.uec.ac.jp/SRV-DB/)のデータセット4と5を使用します。

- データセット4: 声優2名の台詞を連結したストーリー音声
- データセット5: 同じ原稿を台詞ごとに分割した音声

公式ページでは、音声研究や信号処理技術の発展を目的とする利用と、その成果の発表・配布を認めています。
公序良俗に反する利用と、原稿と異なる意味へ意図的に編集する行為は禁止されています。
利用条件は変更される可能性があるため、ダウンロード時に公式ページを確認してください。

音声、原稿、正解ラベル、推論本文はGit、CI、Actionsのcacheやartifact、評価ログへコピーしません。
標準出力へ出すのは数値指標とケースIDだけです。

## ローカル配置

配布ファイルを`test-data/srv-db/`以下へ展開します。
このディレクトリは`.gitignore`に含まれます。

```text
test-data/srv-db/
├── evaluation.json
├── dataset4/
│   ├── yuki_mono_VM00_VF00_0476.wav
│   └── yuki_0476.txt
└── dataset5/
    ├── VM00_0476_001.wav
    └── VM00_0476_001.txt
```

正解テキストは各音声に対応するUTF-8プレーンテキストとしてローカルに用意します。
役名、行番号、注記は除き、発話された文字だけを残します。
データセット5では、単一話者のファイルを選びます。
上のファイル名は配置例です。
展開後の名前を維持しても、ローカルで分かりやすい名前へ変更しても構いませんが、`evaluation.json`と一致させます。

`evaluation.json`は次の形式です。

```json
{
  "cases": [
    {
      "id": "dataset4-vm00-vf00-0476",
      "dataset": 4,
      "audio": "dataset4/yuki_mono_VM00_VF00_0476.wav",
      "reference": "dataset4/yuki_0476.txt",
      "speech_rate_mora_per_second": 4.76,
      "expected_speakers": 2
    },
    {
      "id": "dataset5-vm00-0476-001",
      "dataset": 5,
      "audio": "dataset5/VM00_0476_001.wav",
      "reference": "dataset5/VM00_0476_001.txt",
      "speech_rate_mora_per_second": 4.76,
      "expected_speakers": 1
    }
  ]
}
```

パスは`SRV_DB_DIR`からの相対パスに限ります。
親ディレクトリへの移動や絶対パスは拒否します。

## 評価の実行

エンジンと既定モデルを導入してから、絶対パスを環境変数へ設定します。

```powershell
speech2md engine install
speech2md model install
$env:SRV_DB_DIR = (Resolve-Path "test-data\srv-db")
cargo test -p speech2md-runtime --test srv_db --locked -- --ignored --nocapture
Remove-Item Env:SRV_DB_DIR
```

`SPEECH2MD_ENGINE_DIR`と`SPEECH2MD_MODEL_DIR`で既定以外の保存先を使用している場合は、その絶対パスも設定します。
`SRV_DB_DIR`が未設定の場合、テストは`status`が`skipped`のJSONを出力し、モデルやネットワークへアクセスしません。

## 指標

評価は一つのJSONオブジェクトを標準出力へ書きます。

- `aggregate_cer`: 全ケースの編集距離合計を正解文字数合計で割った文字誤り率
- `cer_by_speech_rate`: 話速ごとに集計した文字誤り率
- `detected_speakers`: 話者分離が返した話者IDの種類数
- `expected_speakers`: 評価manifestで指定した既知の話者数
- `label_consistency`: データセット5の単一話者ファイルで、支配的な話者ラベルが占める話者区間時間の比率
- `real_time_factor`: 処理時間を音声時間で割った値。1.0未満なら音声時間より短く、1.0超なら音声時間より長い
- `peak_memory_bytes`: WindowsではプロセスのPeak Working Set、Linuxでは`VmHWM`。取得できないOSでは`null`

CERでは空白だけを除外し、句読点や文字種を正解文字として数えます。
`label_consistency`は話者名の同定精度ではなく、単一話者音声が同じラベルに保たれた割合です。
データセット4では`null`になります。
SRV-DBには時間付きの話者正解区間がないため、DERは算出しません。

## 1時間と3時間の性能測定

機密情報を含まない評価音声を1時間版と3時間版で用意し、WindowsのreleaseバイナリとWhisper baseを使用します。
音声や生成されたCommonMarkはコミットしません。

```powershell
New-Item -ItemType Directory -Force test-data\srv-db-results | Out-Null
speech2md transcribe test-data\performance\meeting-1h.wav --whisper base --output test-data\srv-db-results\meeting-1h.md
speech2md transcribe test-data\performance\meeting-3h.wav --whisper base --output test-data\srv-db-results\meeting-3h.md
```

CPU名、論理CPU数、音声時間、処理時間、実時間係数、Peak Working Set、一時ディスクの最大使用量を記録します。
実時間係数が1.0を超えても値を補正しません。
READMEの性能表には同じ条件で再測定した結果だけを掲載します。

## データ境界の確認

評価後に追跡対象を確認します。

```console
git status --short
git check-ignore test-data/srv-db/evaluation.json
git check-ignore test-data/srv-db-results/metrics.json
```

SRV-DBを使用した評価結果を公表する場合は、公式ページの要望に従い、「話速バリエーション型音声データベース（SRV-DB）を利用した」と明記します。
