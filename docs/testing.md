# テストと外部評価

ビルドとリリースの手順は[開発ガイド](development.md)を参照してください。

VOICEPEAKで作成する自作音声による話者分離テストは、[VOICEPEAK話者分離テスト原稿](../test-data/voicepeak/README.md)を参照してください。
生成した10本のWAVに対し、Whisperを使わず話者分離だけを連続評価できます。

```powershell
$env:YASUMARO_VOICEPEAK_DIR = (Resolve-Path "samples\voicepeak")
cargo test -p yasumaro-runtime --test voicepeak_eval --locked -- --ignored --nocapture
Remove-Item Env:YASUMARO_VOICEPEAK_DIR
```

テストはケースごとの検出話者数、クラスタごとの発話時間、処理時間、実時間係数を1個のJSONとして標準出力へ書きます。
期待話者数と検出話者数の不一致は観測値としてJSONへ記録し、それだけでテストを失敗させません。
本文の欠落と異常終了は後段のLLMで回復できないため、テストの失敗条件です。

## 実CLIの縦断テスト

自作の4話者音声、導入済みエンジン、3種類の既定モデルを使い、音声入力からCommonMark出力までを検証します。
次の例はWindowsの既定保存先を使用します。

```powershell
$env:YASUMARO_ENGINE_DIR = (Resolve-Path (Join-Path $env:LOCALAPPDATA "yasumaro\data\engines"))
$env:YASUMARO_MODEL_DIR = (Resolve-Path (Join-Path $env:LOCALAPPDATA "yasumaro\data\models"))
$env:YASUMARO_DIARIZATION_FIXTURE = (Resolve-Path "samples\voicepeak\balanced-4speakers.wav")
cargo test -p yasumaro-cli --test real_cli --locked -- --ignored --nocapture
Remove-Item Env:YASUMARO_ENGINE_DIR, Env:YASUMARO_MODEL_DIR, Env:YASUMARO_DIARIZATION_FIXTURE
```

このテストは文字起こしの成功、2個以上かつ指定数以下の話者ラベル、時刻順、非空本文、CommonMark構造を検査します。
話者数の完全一致は保証しません。
話者分離の品質変化は、前節のVOICEPEAK評価が出力するケース別の検出話者数で確認します。

## 通常のテスト

モデルとネットワークを使わないworkspaceテストは、GitHub ActionsでWindows x86-64、Linux x86-64、macOS Apple Silicon／Intelの4構成で実行します。

```console
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

## Leanによるモデル検証

話者割り当て規則とモデルライフサイクルの抽象状態は、[`formal`](../formal/README.md)のLean packageで検証します。

```console
lake -d formal build
lake -d formal exe YasumaroTests
lake -d formal exe model-lifecycle-testgen -- --check crates/yasumaro-runtime/tests/fixtures/lean-model-lifecycle.json
cargo test -p yasumaro-runtime --example model_lifecycle_oracle --features test-support --locked
cargo run -p yasumaro-runtime --example model_lifecycle_oracle --features test-support -- --strict
```

fixture freshnessの検査では、Leanが生成したschema version 1の期待状態とコミット済みJSONを比較します。strict oracleは`strict`と`internal-fixture`の6 caseを実行し、Rust側の観測結果をfixtureの期待状態・期待結果と比較します。壊れたLean遷移だけを扱う`model-only` caseはstrict実行から除外します。

1 caseを再現する場合は、case名を指定します。

```console
cargo run -p yasumaro-runtime --example model_lifecycle_oracle --features test-support -- --case cancel-before-authorization
```

モデルライフサイクルの定理とRust側の主な検査は次のように対応します。

| Leanの性質 | Rust側の検査 |
|---|---|
| 検証前のモデルを公開しない | `model_download`のsize、SHA-256、publish失敗test |
| 公開許可前のキャンセルは公開を阻止する | `model_download`のBeforeAuthorization checkpoint testとoracleの`cancel-before-authorization` |
| 公開許可後のキャンセルは許可を取り消さない | `model_download`のAfterAuthorization checkpoint testとoracleの`cancel-after-authorization` |
| writerとreaderは同時に存在しない | `model_lifecycle`のlease中removeと`model_download`のlease中install待機test |
| 通常cleanup後にpartialが存在しない | `model_download`の通信、timeout、cancel、検証失敗test |
| busy removeは状態を変更しない | `model_lifecycle`と`model_cli`の`ModelInUse` test |
| remove成功後はfinalとpartialが存在しない | `model_lifecycle`と`model_cli`のremove test |

Leanの証明対象は抽象状態です。SHA-256の計算、rename、unlink、symlink、OSのfile lock、強制終了時のcleanupはRustの統合テストと各OSのCIで検査します。oracleは両者の結果を対応づけますが、Rustプログラム全体を形式証明するものではありません。

共有lock待機中とモデル検証中のtranscribe cancellationは`model_lifecycle`と`model::store`のtestで検査します。DNSのblocking taskが残る場合のinstaller破棄上限は`model::download`のunit testで検査します。

実行順への依存は、nightlyのシャッフル機能で検査します。

```console
cargo +nightly test --workspace --all-features --locked -- -Z unstable-options --shuffle --test-threads=1
```

話者割り当てのfuzz targetは、複数セグメント、トークンの有無、長さ0の区間、`u64`上限付近の時刻、可変の重複閾値を生成します。本文・話者ID・区間の保持と、話者区間の入力順に結果が依存しないことを検査します。セグメント間の順序は入力順、セグメント内のトークンは時刻順です。

```console
cargo +nightly fuzz run assign_speakers
```

Windowsで`STATUS_DLL_NOT_FOUND`が発生する場合は、Visual StudioのMSVC x64ディレクトリにある`clang_rt.asan_dynamic-x86_64.dll`を`PATH`から参照できるDeveloper PowerShellで実行します。

`yasumaro-core`と`yasumaro-formats`は、行と関数のカバレッジを100%に保ちます。
runtimeとCLIでは、ネイティブエンジン、OSエラー、プロセス終了タイミングなどの外部境界を実装内テストだけで網羅できないため、fake engine、ignored実モデルテスト、外部評価を併用します。

## Property-based testing

proptestは通常のworkspaceテストで実行します。主な検査は次のとおりです。

- core: `u64`上限付近と長さ0の区間、話者割り当ての閾値直前・一致・直後、同率時の話者ID選択、複数セグメントとトークンなしの本文保持。
- 正規化: Unicodeの文字数と結合間隔の境界、重複区間の接触・重なり、同一・異なる・未知の話者。一意な本文では保持と冪等性も検査します。
- formats: 複数発話、時刻の桁上がり、話者ID上限、Markdown構造の混入、表示文字の欠落や二重エスケープ。空白・改行は表示文字比較から除き、NULはCommonMarkの置換文字として扱います。
- JSON解析: `u64`全域の時刻とUnicode本文の保持、各階層の未知フィールド、正常な文書の途中に挿入された不正な数値・型・必須フィールド欠落の拒否。
- 音声: 6種類のサンプルレート、チャンク境界前後の長さと無音、生成したmono/stereo WAVの変換後の長さ・有限値・16 kHzでのサンプル値。

core・formatsとJSON解析の生成件数は、`PROPTEST_CASES`で増やせます。通常はproptest既定の256ケースです。音声変換は実行時間を抑えるため、リサンプルの各propertyを64ケース、生成WAVを32ケースに固定しています。

```console
PROPTEST_CASES=1024 cargo test -p yasumaro-core -p yasumaro-formats --test properties --locked
PROPTEST_CASES=1024 cargo test -p yasumaro-runtime --lib engine::whisper_json --locked
```

PowerShellでは、実行前に`$env:PROPTEST_CASES = "1024"`を設定し、上記の`cargo`以降を実行します。終了後は`Remove-Item Env:PROPTEST_CASES`で解除できます。

失敗時にproptestが保存する`*.proptest-regressions`はコミット対象です。生成器を変更すると同じseedから得られる入力も変わり得るため、修正した不具合は縮小後の具体的な入力を使う通常の回帰テストにも残します。

## Fuzzing

[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz)で次の7 targetを検査します。

| Target | 入力と検査内容 |
|---|---|
| `decode_audio` | 任意のファイル内容。異常終了と、成功時のPCM出力の基本条件を検査 |
| `decode_wav` | 6種類のサンプルレート・mono/stereoの整数WAVを生成。変換後の長さ、有限値、16 kHz時のサンプル値、無音の保持を検査 |
| `whisper_json` | 任意のバイト列。成功時の区間の妥当性と、JSONへの再変換後の解析結果の一致を検査 |
| `whisper_json_structured` | 正常なJSONを生成し、本文・時刻・件数の保持、未知フィールドの許容、不正な区間を含む文書の拒否を検査 |
| `assign_speakers` | 本文と話者IDを、実装の重複計算・閾値判定関数を使わず計算した期待値と照合 |
| `render_commonmark` | 発話数・時刻・話者ID・本文を変化させ、許可した構造だけが生成されることと表示文字の保持を検査 |
| `normalize_utterances` | マージ・重複除去・未知話者・Unicode文字数上限・時刻順を検査。一意な本文では保持と冪等性も検査 |

CommonMarkの表示文字比較では、空白と改行を除外し、NULを置換文字として扱います。空白の完全な往復一致を保証する検査ではありません。任意の音声ファイルは浮動小数点のNaNやInfinityを含む場合があるため、有限値の検査は整数WAVを生成する`decode_wav`で行います。

通常のpre-commitとPRのCIには含めません。GitHub Actionsの[`fuzz.yml`](../.github/workflows/fuzz.yml)は毎日と手動実行で全targetを実行します。定期実行は各60秒、手動実行は各30・60・300秒から選択できます。探索したcorpusはブランチ別のcacheに保存し、クラッシュ・タイムアウトなどの再現入力はartifactへ14日間保存します。既存の`heavy.yml`のコンパイル確認も利用できます。

```console
rustup toolchain install nightly
cargo +stable install cargo-fuzz --version 0.13.2 --locked
cargo +nightly fuzz check
python scripts/fuzz.py --seconds 60
```

`scripts/fuzz.py`はPython 3.11以降を使います。管理対象の自作音声fixtureとスクリプト内の人工データからseedを生成するため、空のcheckoutでも解析・デコードの成功経路から探索を開始できます。構造化入力のseedは`arbitrary` 1.4の形式を使うため、依存を更新する際には再生も確認します。`decode_audio`のseedには、`stts`のentry countを実際の大きさより膨らませたMP4と、`stsz`のsample sizeを約4 GiBにしたM4Aも含めます。各入力のタイムアウトは10秒、メモリ上限は2 GiB、入力サイズ上限は`decode_audio`が1 MiB、それ以外が64 KiBです。構造化targetでは配列や文字列の処理量も制限しています。

音声統合テストは、先頭に別のatomまたはID3メタデータがあるMP4、親atomの境界を越える子atom、atomに見えるPCMを含むWAV、FIFOからの入力を対象にしています。`cargo test -p yasumaro-runtime --test audio`で実行できます。

```console
# 単一targetを実行
python scripts/fuzz.py --target assign_speakers --seconds 60
# seedを生成し、既存corpusを変異なしで再生
python scripts/fuzz.py --replay
# fuzz crateはworkspace外なので別途整形・lintを確認
cargo fmt --manifest-path fuzz/Cargo.toml --check
cargo clippy --manifest-path fuzz/Cargo.toml --all-targets --locked -- -D warnings
```

失敗入力を再現し、最小化する例です。`<artifact>`には実際に保存された入力ファイルのパスを指定します。

```console
cargo +nightly fuzz run assign_speakers <artifact>
cargo +nightly fuzz tmin assign_speakers <artifact>
```

修正時には最小化した入力を通常の回帰テスト、またはseed生成スクリプトの人工データとして残します。利用者の音声やSRV-DBのデータをcorpusに追加しないでください。CIがcache・artifactへ保存するのは、管理対象のfixtureと人工データから生成した入力だけです。

WindowsではVisual StudioのMSVC C++ x64/x86ビルドツール、C++ AddressSanitizer、Windows 11 SDKが必要です。
「x64 Native Tools Command Prompt」で`where link`を実行し、使用するVisual Studioの`Hostx64\x64`以下にあるリンカーが先頭に表示されることを確認してください。
クラッシュ入力は`fuzz/artifacts/`へ保存され、Gitの管理対象には含まれません。

## GitHub Actionsの構成

通常CIは用途ごとにファイルを分けています。各ワークフローは`main`宛てのPR、マージキューの検査要求（`merge_group`）、手動実行で起動します。
`main`へのpushでは起動しないため、PRをマージした直後に同じCIを繰り返しません。

| ファイル | 検査内容 |
|---|---|
| [`ci.yml`](../.github/workflows/ci.yml) | Rustfmt、default-featureの全target check、全featureの厳格なClippyを実行 |
| [`tests.yml`](../.github/workflows/tests.yml) | 4構成のworkspaceテスト |
| [`formal.yml`](../.github/workflows/formal.yml) | Leanのbuild、実行テスト、fixture freshness、Rust oracleのstrict対応検査 |
| [`shuffled-tests.yml`](../.github/workflows/shuffled-tests.yml) | nightlyで実行順をランダム化した逐次テストを3回実行 |
| [`coverage.yml`](../.github/workflows/coverage.yml) | coreとformatsそれぞれの行・関数カバレッジ100%を検査 |
| [`release-automation.yml`](../.github/workflows/release-automation.yml) | タグ採番・競合・再実行・配布情報・バージョン反映のテスト |

[`release.yml`](../.github/workflows/release.yml)はPRでプレビュー成果物を生成し、マージ時にはタグ付けと正式版のビルド・公開を行います。
workspaceテストとリリーススクリプトのテストは上表へ移してあるため、通常CIとの重複はありません。
正式版には採番したバージョンとReleaseのURLを埋め込む必要があるため、マージ後にもビルドします。新しく生成したエンジンの実推論・キャンセル、CLIの展開後の起動、チェックサムはその成果物に対して検証します。

[`fuzz.yml`](../.github/workflows/fuzz.yml)は定期・手動のfuzzing実行を担当します。[`heavy.yml`](../.github/workflows/heavy.yml)のfuzz targetのコンパイルとworkspace全体のカバレッジ計測は、引き続き手動実行です。

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
yasumaro engine install
yasumaro model install
$env:SRV_DB_DIR = (Resolve-Path "test-data\srv-db")
cargo test -p yasumaro-runtime --test srv_db --locked -- --ignored --nocapture
Remove-Item Env:SRV_DB_DIR
```

`YASUMARO_ENGINE_DIR`と`YASUMARO_MODEL_DIR`で既定以外の保存先を使用している場合は、その絶対パスも設定します。
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
yasumaro transcribe test-data\performance\meeting-1h.wav --whisper base --output test-data\srv-db-results\meeting-1h.md
yasumaro transcribe test-data\performance\meeting-3h.wav --whisper base --output test-data\srv-db-results\meeting-3h.md
```

CPU名、論理CPU数、音声時間、処理時間、実時間係数、Peak Working Set、一時ディスクの最大使用量を記録します。
実時間係数が1.0を超えても値を補正しません。
同じ条件で再測定した結果を次の表へ記録します。利用者向けに公開できる実測結果はREADMEにも反映します。

### 測定状況

| 測定環境 | 音声時間 | Whisper | 処理時間 | 実時間係数 | ピークメモリ | 一時ディスク |
|---|---:|---|---:|---:|---:|---:|
| AMD Ryzen 7 PRO 7840U、16論理CPU | 1時間 | base | 未測定 | 未測定 | 未測定 | 約346 MB以上 |
| AMD Ryzen 7 PRO 7840U、16論理CPU | 3時間 | base | 未測定 | 未測定 | 未測定 | 約1.04 GB以上 |

一時ディスクの値は16 kHz、mono、`float32` PCMと16-bit WAVの理論上の合計で、ファイルシステムなどの余白を含みません。
実測値は実モデルと評価音声を準備した後に記録します。

## データ境界の確認

評価後に追跡対象を確認します。

```console
git status --short
git check-ignore test-data/srv-db/evaluation.json
git check-ignore test-data/srv-db-results/metrics.json
```

SRV-DBを使用した評価結果を公表する場合は、公式ページの要望に従い、「話速バリエーション型音声データベース（SRV-DB）を利用した」と明記します。
