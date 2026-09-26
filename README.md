# speech2md

speech2mdは、日本語の会議音声をローカルで文字起こしし、話者と時刻を含むCommonMark文書を生成するCLIです。

## 対象環境

Windows x86-64、Linux x86-64（glibc）、macOS（Apple Silicon／Intel）に対応しています。
GPUは使用せず、CPUだけで処理します。

## 対応する入力と出力

入力はWAV、MP3、AAC-LCを格納したM4Aです。
iPhoneのボイスメモで作成した一般的なM4Aを想定しています。
FFmpegは必要ありません。
動画ファイルはまだ受け付けません。

出力はUTF-8のCommonMarkです。
プレーンテキスト、HTML、独自形式はまだ選択できません。

## インストール

[GitHub Releases](https://github.com/HayaoSuzuki/speech2md/releases/latest)のAssetsから、使用するOSとCPUに合うCLIアーカイブをダウンロードしてください。
RustやPythonのインストールは不要です。

ファイル名の`<version>`は、`v0.1.0`のようなReleaseのバージョンに置き換えてください。

| 環境 | CLIアーカイブ |
|---|---|
| Windows x86-64 | `speech2md-<version>-windows-x86_64.zip` |
| Linux x86-64（glibc） | `speech2md-<version>-linux-x86_64.tar.gz` |
| macOS Apple Silicon | `speech2md-<version>-macos-aarch64.tar.gz` |
| macOS Intel | `speech2md-<version>-macos-x86_64.tar.gz` |

アーカイブを任意のディレクトリへ展開し、中にある`bin`ディレクトリを`PATH`へ追加します。
Windowsの実行ファイルは`bin\speech2md.exe`、LinuxとmacOSは`bin/speech2md`です。

例えば、macOS Apple Silicon版の`v0.1.0`をダウンロードした場合は、保存先で次のように展開して起動できます。

```sh
tar -xzf speech2md-v0.1.0-macos-aarch64.tar.gz
./bin/speech2md --help
```

`PATH`へ追加した後は、どのディレクトリからでも次のコマンドで起動を確認できます。

```console
speech2md --version
```

推論エンジンとモデルはCLIアーカイブに含まれません。続けて初回セットアップを行ってください。

## 初回セットアップ

推論エンジンとモデルは、文字起こしを始める前に明示的に導入します。

```console
speech2md engine install
speech2md model install
speech2md doctor
```

`engine install`はGitHub Releases、`model install`はモデルの配布元へ接続します。
ダウンロードしたファイルはサイズとSHA-256を検査してから配置します。
`transcribe`がエンジンやモデルを暗黙にダウンロードすることはありません。

導入状態は次のコマンドで確認できます。

```console
speech2md engine list
speech2md engine verify
speech2md model list
```

## 基本操作

出力先を省略すると、入力ファイルと同じ場所に同名の`.md`ファイルを書き込みます。

```console
speech2md transcribe meeting.m4a
speech2md transcribe meeting.wav --speakers 3 --prompt "Rust, Kubernetes, PostgreSQL"
speech2md transcribe meeting.mp3 --whisper small --output minutes.md
```

既存ファイルは上書きしません。
置き換える場合だけ`--force`を付けます。

```console
speech2md transcribe meeting.wav --output minutes.md --force
```

## 話者数

話者分離は常に実行します。
話者数を省略すると自動推定し、人数が分かっている場合は`--speakers`で正の整数を指定します。

```console
speech2md transcribe meeting.wav --speakers 4
```

`Speaker 1`などの番号は一つの録音内だけで有効です。
別の録音に現れる同じ番号が同一人物を示すわけではありません。

## 文字起こしモデルとプロンプト

既定の文字起こしモデルは`base`です。
`small`は保存容量と処理時間が増える代わりに、精度が改善する場合があります。

```console
speech2md transcribe meeting.wav --whisper small
speech2md transcribe meeting.wav --prompt "Rust, Kubernetes, PostgreSQL"
speech2md transcribe meeting.wav --prompt-file prompt.txt
```

長いプロンプトや機密性のある用語集には、UTF-8の`--prompt-file`を使用してください。
`--prompt`と`--prompt-file`は同時に指定できません。

## 出力例

出力はCommonMarkの見出し、話者ラベル、開始時刻、発話本文で構成されます。

```markdown
# meeting

**Speaker 1**（00:00:12）

今回のリリースについて確認します。

**Speaker 2**（00:00:18）

API側の変更は完了しています。
```

## LLMによる後処理

speech2mdは、音声のまま外部サービスへ渡す代わりに、ローカルで文字起こししたCommonMarkを作ります。
これにより、後段のLLMへ渡すデータをテキストに限定できます。
実際のトークン数は使用するLLMとトークナイザーによって異なります。

話者ラベルは後処理の手掛かりであり、人物の同定や正確な話者数を保証するものではありません。
自作の評価音声では、10ケース中7ケースで期待話者数と検出話者数が一致しました。
話者数が一致しない場合も本文は出力し、LLMまたは利用者が話者名、見出し、要約、フィラーを整える運用を想定しています。
機密情報を外部のLLMへ渡すかどうかは、社内ルールと利用するサービスのデータ取り扱い条件に従って判断してください。

## オフライン処理と保存場所

音声デコード、文字起こし、話者分離、CommonMark生成はすべてローカルで行います。
ネットワークへ接続するのは、利用者が`engine install`または`model install`を実行したときだけです。
ログには音声、文字起こし本文、モデルURL、完全なファイルパスを記録しません。

エンジンとモデルの既定の保存先は次のとおりです。

| OS | エンジン | モデル |
|---|---|---|
| Windows | `%LOCALAPPDATA%\speech2md\data\engines` | `%LOCALAPPDATA%\speech2md\data\models` |
| macOS | `~/Library/Application Support/speech2md/engines` | `~/Library/Application Support/speech2md/models` |
| Linux | `$XDG_DATA_HOME/speech2md/engines` | `$XDG_DATA_HOME/speech2md/models` |

Linuxで`XDG_DATA_HOME`が未設定の場合は`~/.local/share/speech2md`以下を使用します。
絶対パスの`SPEECH2MD_ENGINE_DIR`と`SPEECH2MD_MODEL_DIR`で保存先を変更できます。

## モデルの容量とライセンス

`model install`を引数なしで実行すると、`whisper-base`と話者分離用の2モデルを導入します。
`whisper-small`は使用する場合だけ個別に導入します。

```console
speech2md model install whisper-small
```

| モデル | 用途 | ダウンロードサイズ | ライセンス |
|---|---|---:|---|
| `whisper-base` | 文字起こし | 148 MB | MIT |
| `whisper-small` | 文字起こし | 488 MB | MIT |
| `speaker-segmentation` | 話者区間検出 | 6 MB | MIT |
| `speaker-embedding` | 話者特徴量 | 40 MB | Apache-2.0 |

サイズは10進MBでの概算です。
導入中は部分ファイルと完成ファイルが一時的に併存するため、表の合計より多い空き容量を確保してください。

## 処理時間と空き容量

処理時間はCPU、音声時間、モデル、話者数、話速によって変わります。
通常の対象は最大1時間、追加用途の上限は3時間ですが、処理時間の保証値ではありません。
長時間音声の処理時間とピークメモリは未測定です。

変換中の一時ファイルには、1時間の音声で約346 MB以上、3時間で約1.04 GB以上の空き容量が必要です。
これは音声変換に必要なファイルサイズの理論値です。入力音声、出力文書、エンジン、モデルの保存容量は別途確保してください。

## 制約

- 日本語の文字起こしだけを対象とし、英語への翻訳は行いません。
- 話者名は推定せず、録音内の番号だけを割り当てます。
- 話者が重なって発話する区間では、話者割り当てが不安定になる場合があります。
- 要約、言い換え、フィラー除去、推測による誤認識修正は行いません。
- 変換中は一時ファイルを作成し、正常終了時と処理失敗時に削除します。

## トラブルシューティング

最初に診断結果を確認します。

```console
speech2md doctor
```

`engine: not installed`の場合は`speech2md engine install`、モデルが不足している場合は`speech2md model install`を実行します。
出力先が存在するエラーでは、既存ファイルを確認してから必要な場合だけ`--force`を指定します。
無効な`RUST_LOG`を設定している場合は、値を修正するか環境変数を削除します。

詳細ログは標準エラーへ出力され、CommonMarkには混ざりません。

```powershell
$env:RUST_LOG = "speech2md_runtime=debug,speech2md_cli=info"
speech2md doctor
Remove-Item Env:RUST_LOG
```

開発に参加する場合は[開発ガイド](https://github.com/HayaoSuzuki/speech2md/blob/main/docs/development.md)を参照してください。
