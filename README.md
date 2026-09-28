# yasumaro

yasumaroは、日本語の会議音声をローカルで文字起こしし、話者と時刻を含むCommonMark文書を生成するCLIです。
名前は、『古事記』の編纂者・太安万侶に由来します。

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

[GitHub Releases](https://github.com/HayaoSuzuki/yasumaro/releases/latest)のAssetsから、使用するOSとCPUに合うCLIアーカイブをダウンロードしてください。
RustやPythonのインストールは不要です。

ファイル名の`<version>`は、`v0.1.0`のようなReleaseのバージョンに置き換えてください。

| 環境 | CLIアーカイブ |
|---|---|
| Windows x86-64 | `yasumaro-<version>-windows-x86_64.zip` |
| Linux x86-64（glibc） | `yasumaro-<version>-linux-x86_64.tar.gz` |
| macOS Apple Silicon | `yasumaro-<version>-macos-aarch64.tar.gz` |
| macOS Intel | `yasumaro-<version>-macos-x86_64.tar.gz` |

アーカイブを任意のディレクトリへ展開し、中にある`bin`ディレクトリを`PATH`へ追加します。
Windowsの実行ファイルは`bin\yasumaro.exe`、LinuxとmacOSは`bin/yasumaro`です。

例えば、macOS Apple Silicon版をダウンロードした場合は、保存先で次のように展開して起動できます。

```sh
tar -xzf yasumaro-*-macos-aarch64.tar.gz
./bin/yasumaro --help
```

`PATH`へ追加した後は、どのディレクトリからでも次のコマンドで起動を確認できます。

```console
yasumaro --version
```

推論エンジンとモデルはCLIアーカイブに含まれません。続けて初回セットアップを行ってください。

## speech2mdからの移行

コマンド名を`yasumaro`へ変更しました。既存のスクリプトやショートカットも変更してください。
環境変数は`YASUMARO_ENGINE_DIR`と`YASUMARO_MODEL_DIR`を使用します。
旧名の`SPEECH2MD_ENGINE_DIR`と`SPEECH2MD_MODEL_DIR`は参照しません。
`RUST_LOG`でモジュールを指定する場合も、`yasumaro_runtime`や`yasumaro_cli`へ変更してください。

既定の保存先も`speech2md`から`yasumaro`へ変わります。旧保存先のデータは自動で移動・削除しません。
導入済みモデルを再利用する場合は、`YASUMARO_MODEL_DIR`に旧モデルディレクトリの絶対パスを指定できます。
エンジンは`yasumaro engine install`で導入し直してください。

過去のReleaseには旧名の配布ファイルが残ります。`yasumaro-`で始まるCLIアーカイブを使用してください。

## 初回セットアップ

推論エンジンとモデルは、文字起こしを始める前に明示的に導入します。

```console
yasumaro engine install
yasumaro model install
yasumaro doctor
```

`engine install`はGitHub Releases、`model install`はモデルの配布元へ接続します。
ダウンロードしたファイルはサイズとSHA-256を検査してから配置します。
`transcribe`がエンジンやモデルを暗黙にダウンロードすることはありません。

導入状態は次のコマンドで確認できます。

```console
yasumaro engine list
yasumaro engine verify
yasumaro model list
```

## 基本操作

出力先を省略すると、入力ファイルと同じ場所に同名の`.md`ファイルを書き込みます。

```console
yasumaro transcribe meeting.m4a
yasumaro transcribe meeting.wav --speakers 3 --prompt "Rust, Kubernetes, PostgreSQL"
yasumaro transcribe meeting.mp3 --whisper small --output minutes.md
```

既存ファイルは上書きしません。
置き換える場合だけ`--force`を付けます。

```console
yasumaro transcribe meeting.wav --output minutes.md --force
```

## 話者数

話者分離は常に実行します。
話者数を省略すると自動推定し、人数が分かっている場合は`--speakers`で正の整数を指定します。

```console
yasumaro transcribe meeting.wav --speakers 4
```

`Speaker 1`などの番号は一つの録音内だけで有効です。
別の録音に現れる同じ番号が同一人物を示すわけではありません。

## 文字起こしモデルとプロンプト

既定の文字起こしモデルは`base`です。
`--whisper`で`base`、`small`、`medium`、`large-v3`、`large-v3-turbo`を選択できます。
大きなモデルでは精度が改善する場合がありますが、必要なメモリと処理時間も増えます。
モデルをインストールしただけでは切り替わらないため、実行時にも指定してください。

```console
yasumaro transcribe meeting.wav --whisper small
yasumaro transcribe meeting.wav --whisper medium
yasumaro transcribe meeting.wav --whisper large-v3
yasumaro transcribe meeting.wav --whisper large-v3-turbo
yasumaro transcribe meeting.wav --prompt "Rust, Kubernetes, PostgreSQL"
yasumaro transcribe meeting.wav --prompt-file prompt.txt
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

yasumaroは、音声のまま外部サービスへ渡す代わりに、ローカルで文字起こししたCommonMarkを作ります。
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
| Windows | `%LOCALAPPDATA%\yasumaro\data\engines` | `%LOCALAPPDATA%\yasumaro\data\models` |
| macOS | `~/Library/Application Support/yasumaro/engines` | `~/Library/Application Support/yasumaro/models` |
| Linux | `$XDG_DATA_HOME/yasumaro/engines` | `$XDG_DATA_HOME/yasumaro/models` |

Linuxで`XDG_DATA_HOME`が未設定の場合は`~/.local/share/yasumaro`以下を使用します。
絶対パスの`YASUMARO_ENGINE_DIR`と`YASUMARO_MODEL_DIR`で保存先を変更できます。

## モデルの容量とライセンス

`model install`を引数なしで実行すると、`whisper-base`と話者分離用の2モデルを導入します。
`small`以上のモデルは、使用するものだけ個別に導入します。

```console
yasumaro model install whisper-small
yasumaro model install whisper-medium
yasumaro model install whisper-large-v3
yasumaro model install whisper-large-v3-turbo
```

| モデル | 用途 | ダウンロードサイズ | ライセンス |
|---|---|---:|---|
| `whisper-base` | 文字起こし | 148 MB | MIT |
| `whisper-small` | 文字起こし | 488 MB | MIT |
| `whisper-medium` | 文字起こし | 1,534 MB | MIT |
| `whisper-large-v3` | 文字起こし | 3,095 MB | MIT |
| `whisper-large-v3-turbo` | 文字起こし | 1,625 MB | MIT |
| `speaker-segmentation` | 話者区間検出 | 6 MB | MIT |
| `speaker-embedding` | 話者特徴量 | 40 MB | Apache-2.0 |

サイズは10進MBでの概算です。
導入中は部分ファイルと完成ファイルが一時的に併存するため、表の合計より多い空き容量を確保してください。

## モデルの削除と再導入

不要になったモデルは、`model remove`に一つ以上のモデル名を指定して削除します。

```console
yasumaro model remove whisper-small
yasumaro model remove whisper-medium whisper-large-v3
```

文字起こしまたは導入処理がモデルを使用している場合、removeは待機せず、例えば`model whisper-small is in use`と表示して失敗します。
処理の完了後に同じ`model remove`を再実行してください。

`model install`は、取得が必要なモデルを途中から再開せず、毎回先頭から取得します。
ダウンロード中の内容は、確定モデルと区別できる`.part`ファイルへ書き込みます。
通信エラー、検証エラー、キャンセルなどの通常の失敗では、この`.part`ファイルを削除します。
強制終了や電源断では`.part`ファイルが残ることがあります。その場合は、次回の`model install`または`model remove`が削除を試みます。

`model install`は、接続待ちと応答待ちでも25ミリ秒間隔でキャンセルフラグを確認します。
公開許可前にCtrl+Cを検出した場合は、`.part`ファイルの削除を試みます。削除に成功すれば終了コード130、削除にも失敗した場合はストレージエラーとして終了します。
通常ファイルへの書き込みまたは同期を実行中の場合は、そのファイルシステム処理が戻ってから`.part`ファイルを削除します。

サイズとSHA-256の検証後、`model install`はキャンセルフラグを最後に確認してから確定名への変更を許可します。
この確認より前にキャンセルを検出した場合は、確定モデルを作りません。
確認後にCtrl+Cが届いた場合はrenameを続け、renameに成功すればinstall成功として扱います。

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
yasumaro doctor
```

`engine: not installed`の場合は`yasumaro engine install`、モデルが不足している場合は`yasumaro model install`を実行します。
出力先が存在するエラーでは、既存ファイルを確認してから必要な場合だけ`--force`を指定します。
無効な`RUST_LOG`を設定している場合は、値を修正するか環境変数を削除します。

詳細ログは標準エラーへ出力され、CommonMarkには混ざりません。

```powershell
$env:RUST_LOG = "yasumaro_runtime=debug,yasumaro_cli=info"
yasumaro doctor
Remove-Item Env:RUST_LOG
```

開発に参加する場合は[開発ガイド](https://github.com/HayaoSuzuki/yasumaro/blob/main/docs/development.md)を参照してください。
