# モデル削除と失敗時クリーンアップ 設計書

## 背景

Issue #5 は、モデル取得の中断後に HTTP Range で取得を再開する機能を求めていた。しかし、再開用メタデータ、配布物の validator、公開用backup、クラッシュ復旧を組み合わせると、永続状態と異常終了境界が増える。利用者が必要としている操作を再確認した結果、再開機能を取り下げ、モデルを明示的に削除できることと、通常の失敗後に部分ファイルを残さないことを新しい要件とする。

この設計は、旧ブランチ `issue-5-resumable-model-download` を変更しない。基点 `de0ec73` から作成した `issue-5-model-remove-clean-install` に必要な機能だけを実装する。

## 目的

- `yasumaro model remove <model>...` で指定モデルを削除できる。
- install は取得が必要な場合にoffset 0から開始し、再開用の永続状態を作らない。
- 通信、キャンセル、検証、公開の通常の失敗では `.part` を削除する。
- 接続待ちと応答待ちの間もCtrl+Cを監視し、キャンセルを短時間で終了コード130へ変換する。
- サイズと SHA-256 を検証したモデルだけを文字起こし処理へ渡す。
- install、remove、transcribe の競合をモデル単位のロックで調停する。
- 検証、キャンセル、公開の順序をLeanで証明し、Leanが生成した期待値をRust実装と照合する。

## 対象外

- HTTP Range、ETag、Last-Modified、`If-Range`
- 取得済みバイト列を次回の install で再利用する処理
- 同じ install 内での自動再接続
- sidecar、公開用backup、backup復旧
- SIGKILLや電源断の瞬間に `.part` の削除を完了させる保証
- 深さを増やす有限状態の網羅探索と監査報告生成
- `engine install` の変更
- `transcribe` による暗黙のモデル取得

## 保存するファイル

モデル `ggml-base.bin` には次の三つのパスだけを使う。

```text
ggml-base.bin            検証済みの確定モデル
ggml-base.bin.part       install中だけ使う部分ファイル
.locks/whisper-base.lock モデル単位のロック
```

lock file はロック対象として再利用するため、installやremoveの完了後も残す。`.part` は再開情報ではない。通常の失敗では削除し、強制終了後に残っていれば次のinstallまたはremoveが削除する。

確定モデルと `.part` は通常ファイルだけを受理する。symlink はリンク自体を削除し、directory は空の場合だけ非再帰で削除する。非空directoryやその他の削除失敗はstorage errorとし、再帰削除は行わない。

## ロックとモデルlease

`ModelStore` はモデルIDごとのlock fileを作る。installは排他ロックを待って取得する。transcribeは対象モデルの共有leaseを処理終了まで保持する。

removeは非待機の排他ロックを一度だけ試す。共有leaseまたはinstallの排他ロックと競合した場合は、状態を変更せず`ModelInUse`を返す。ファイルシステムがロックを利用できない場合やlock fileを開けない場合は、競合と区別して`Lock`を返す。

Whisperの子プロセスを起動して終了を待つ間、Whisperモデルのleaseを保持する。Sherpaのネイティブ資源がモデルを参照する間、話者区間モデルと話者埋め込みモデルのleaseを保持する。ネイティブ資源を破棄した後にleaseを解放する。

## Install

モデルごとに次の順序で処理する。

1. モデルルートとlock directoryを作り、排他ロックを取得する。
2. 前回の強制終了で残った `.part` を削除する。削除できなければ通信を始めない。
3. 確定パスがマニフェストのサイズとSHA-256に一致する通常ファイルなら、通信せず成功する。
4. 確定パスに不正なファイル、symlink、空directoryがあれば安全に削除する。非空directoryはエラーにする。
5. URLへ無条件GETを送る。配布元のredirectは一回の取得試行の中で最大10回まで追跡し、最初のURLがHTTPSならHTTPへのdowngradeを拒否する。失敗した転送を再試行せず、Range headerも送らない。HTTP処理には非同期clientを使うが、`ModelInstaller`の公開APIは同期のままにする。
6. 接続と各body readを、25ミリ秒間隔のキャンセル監視と競合させる。接続は30秒、データを受信しないreadは60秒で失敗させる。
7. 決定的な `.part` パスを新規作成し、応答本文を書き込む。既存entryを削除した後に新規作成するため、symlinkを追跡しない。
8. 期待サイズを超えた時点で停止する。本文終了後に `.part` を同期し、実ファイル長と全体のSHA-256を検証する。
9. キャンセルフラグを最後に確認し、falseなら公開を許可する。検証済み `.part` を確定名へrenameする。確定名は手順4で存在しないため、Windowsでも置換用backupを必要としない。
10. 成功ログを出し、排他ロックを解放する。

接続、HTTP status、read、write、同期、サイズ、SHA-256、rename、キャンセルのいずれかで失敗した場合は、`.part` の削除を試みる。削除に成功すれば元のエラーを返す。削除にも失敗した場合は、元のエラーとcleanup errorの両方を持つ`CleanupFailed`を返す。

## キャンセルと強制終了

CLIはinstall開始前にCtrl+C handlerを登録し、installerへcancellation flagを渡す。installerはロック待機、接続待ち、各body read、書き込み、同期、検証、公開許可の前後でflagを確認する。接続とbody readでは非同期処理と25ミリ秒間隔の監視を競合させる。キャンセルを検出したら `.part` を削除し、`Cancelled`を返す。CLIは終了コード130と再実行手順を表示する。

DNS解決を含むblocking taskがキャンセル時に残っても、installerはruntimeの停止待機を100ミリ秒で打ち切る。blocking task自体は中断できずresolverが戻るまで残ることがあるが、キャンセル結果の返却を待たせない。transcribeの共有lock待機とモデル全体検証も25ミリ秒間隔またはhash chunk境界で同じcancellation flagを確認する。

公開許可は、rename直前に行うキャンセルフラグの最終確認で確定する。最終確認でtrueを読んだ場合はcleanupへ進む。falseを読んだ後のキャンセルは現在のモデルの公開を止めず、renameが成功すればinstall成功として扱う。この規則により、同時に起きたキャンセルと公開の結果を一意に決める。

通常ファイルへのwriteと`sync_all`は処理中に安全に中断できない。installerは各呼び出しの直後にキャンセルを確認するため、キャンセル完了は実行中のファイルシステム呼び出しが戻るまで遅れることがある。ネットワーク待機にはこの制約を適用しない。

SIGKILLや電源断ではプロセスがcleanupを実行できない。この場合だけ `.part` が残り得る。次回のinstallは通信前に削除し、removeも削除対象に含める。残存 `.part` を再開には使わない。

## Remove

`yasumaro model remove <model>...` は一つ以上のモデル指定を要求する。指定値をモデルID順にsortし、重複を除いてから一つずつ処理する。

各モデルでは非待機の排他ロックを取得し、`.part`、確定モデルの順に安全に削除する。partialの削除に失敗した場合は確定モデルを変更しない。確定モデルの削除に失敗した場合もpartialは既に存在しないため、確定モデルだけを保持する状態になる。両方が存在しなくても成功する。使用中なら`ModelInUse`で即座に失敗する。

複数モデルの途中で失敗した場合、完了済みの削除は元に戻さず、後続モデルを処理しない。CLIは成功時に削除した一意なモデル数を表示する。

## エラー

`ModelError`へ次を追加する。

- `Cancelled { id }`: 利用者がinstallを中断した。
- `HttpClientInitialization`: モデルを選ぶ前にHTTP clientまたは非同期runtimeの初期化に失敗した。内部エラーの文字列は利用者へ表示しない。
- `ModelInUse { id }`: removeが非待機の排他ロックを取得できなかった。
- `Lock { id, message }`: lock directory、lock file、またはロックAPIで競合以外の失敗が起きた。
- `CleanupFailed { id, source: Box<ModelError>, cleanup }`: installの元の失敗に `.part` 削除失敗が重なった。`source`に元の分類を保持し、`cleanup`に削除失敗を保持する。

エラー表示にURLと完全なローカルパスを含めない。`ModelInUse`は文字起こしまたはinstallの完了後に再実行するよう案内する。`CleanupFailed`は`model remove <model>`による清掃を案内する。

## Lean状態機械

Leanの状態は次の値だけを持つ。

```text
published          確定名のモデルが存在する
publishedVerified  確定モデルが全体検証済みである
partial            .partが存在する
partialVerified    .partのサイズとSHA-256が検証済みである
publishAuthorized  最後のキャンセル確認を通過し、公開を許可した
cancelRequested    現在のinstallにキャンセル要求が届いた
readers            共有lease数
writer             installまたはremoveが排他ロックを持つ
```

eventはinstall開始、partial作成、検証成功、キャンセル要求、公開許可、公開、通常失敗とcleanup成功、cleanup失敗、lease取得・解放、remove試行に限定する。install開始は`cancelRequested = false`の場合だけwriterを取得する。検証成功は`partialVerified`を設定する。公開許可は`partialVerified = true`かつ`cancelRequested = false`の場合だけ`publishAuthorized`を設定し、公開は許可済みの場合だけ確定モデルを作る。remove試行はロックを取得できる場合だけ状態を消去し、使用中なら状態を変えない。

`published`は、この状態機械が検証後に公開した確定モデルを表す。外部から置かれた不正なfinal entryは`published`として扱わず、Rust実装が通信前に検査して削除する。安全条件は`published → publishedVerified`、`partialVerified → partial`、`publishAuthorized → partialVerified ∧ writer`、`writer → readers = 0`とする。プロセス開始時の初期状態は、partialの有無を制限せず、`partialVerified`、`publishAuthorized`、`cancelRequested`をfalseにする。これにより、強制終了で残った未検証partialと検証済みfinalが併存する状態も対象にする。インストール後に別プロセスがモデル内容を書き換える操作は状態機械の対象外とする。

次の定理を証明する。

- 到達可能な状態で`published → publishedVerified`が成り立つ。
- `partialVerified → partial`と`publishAuthorized → partialVerified ∧ writer`が成り立つ。
- 未検証partialは公開を許可されない。
- 公開許可前のキャンセル要求は公開を阻止する。
- 公開許可後のキャンセル要求は許可を取り消さない。
- 通常失敗でcleanupに成功した後は`partial = false`である。
- cleanup失敗は`published`と`publishedVerified`を変更せず、未検証モデルを公開しない。
- 使用中のremove試行は状態を変えない。
- remove成功後は`published = false`かつ`partial = false`である。
- `writer = true`なら`readers = 0`である。

Leanは抽象化した状態遷移を証明する。ファイル削除、rename、OSロックAPIの動作はRustテストで確認する。`publishAuthorized`を未検証またはキャンセル済みのpartialにも設定する壊れた遷移を別に定義し、安全条件を破る固定witnessを残す。網羅探索は行わない。

## Lean生成fixtureとRust oracle

Leanは正常公開、未検証partial、公開許可前のキャンセル、公開許可後のキャンセル、busy remove、remove成功をJSON fixtureとして生成する。各caseはイベント列、最終状態の期待値、`strict`、`internal-fixture`、`model-only`のいずれかのmodeを持つ。fixtureの期待値をRust側で書き直さず、CIはLeanの再生成結果とコミット済みfixtureが一致することを検査する。

Rust adapterは`ModelInstaller::install_with_cancellation`と`ModelStore::remove`を呼び、確定モデル、partial、エラー分類を観測する。公開境界のcaseに限り、`test-support` featureで公開許可の直前と直後に停止できる同期点をinstallerへ渡す。この同期点は同じproduction関数内で動き、通常buildでは何もしない。adapterはcaseを個別に実行し、結果を`match`、`mismatch`、`infrastructure error`のいずれかに分類する。`strict`と`internal-fixture`の不一致はテストを失敗させ、`model-only`はLean内の検出能力だけを確認する。

## Rustテスト

ローカルHTTPサーバーと一時ディレクトリを使い、次を検査する。

- fresh downloadはRange headerを送らず、sidecarを作らない。
- 通信切断、read timeout、Ctrl+C、過大応答、サイズ不一致、SHA-256不一致、rename失敗後に `.part` が残らない。
- 接続応答待ちとbody停止中のCtrl+Cが250ミリ秒以内に`Cancelled`を返し、`.part`を残さない。
- 全体検証後かつ公開許可前のCtrl+Cは確定モデルを作らず、公開許可後のCtrl+Cはrename成功時にinstall成功となる。
- cleanup自体の失敗では元の失敗とcleanup失敗を報告し、確定モデルを作らない。
- 強制終了相当の古い `.part` を次回installが通信前に削除する。
- 検証済みモデルだけを確定名へ配置する。
- 有効な確定モデルがある場合は通信しない。
- 同時installを排他ロックで直列化する。
- removeは確定モデルと `.part` を冪等に削除する。
- removeのpartial削除が失敗した場合は確定モデルを保持する。
- lease保持中のremoveは短時間で`ModelInUse`を返し、lease解放後は成功する。
- symlinkを追跡せず、非空directoryを再帰削除しない。
- CLIはモデル指定を必須とし、重複排除、途中失敗、終了コード130、利用者向け案内を契約どおり処理する。

既存のworkspaceテスト、format、全target・全featureのClippy、Lean build、Lean theorem tests、生成fixtureのfreshness、Rust oracleのstrict modeを完了条件に含める。Windows固有のロックとrenameは既存のWindows CIで確認する。

## 文書

READMEへmodel remove、使用中エラー、失敗時cleanup、強制終了後の次回cleanupを記載する。Range再開、sidecar、validator、backupに関する説明は追加しない。

Issue #5の元の再開要件を実装しない判断と、この設計への変更理由を設計書に残す。GitHub Issue本文の変更やコメント投稿は、この実装には含めない。branch完成時に、要件変更を説明するIssueコメント案を利用者へ提示する。

## セルフレビュー

設計書、実装計画書、各実装段階について次の3巡を別々に行う。

1. 要件レビューでは、承認済みの削除、失敗時cleanup、非待機remove、対象外事項と照合する。
2. 状態・安全性レビューでは、全失敗経路、ロック、強制終了境界、Leanとの対応を確認する。
3. 実装品質レビューでは、差分、テストの検出能力、OS差、エラーと文書の整合を確認する。

レビューで追加機能の案が出ても、その場では実装しない。承認済み契約に必要な修正だけを行い、追加機能は残存事項として記録する。

## 完了条件

- 利用者がコマンドで一つ以上のモデルを削除できる。
- 使用中のモデルは状態を変えず、待たずにエラーになる。
- 通常のinstall失敗後に `.part` が残らない。
- ネットワーク待機中のキャンセルが短時間で終了コード130になる。
- 強制終了で残った `.part` を次回installまたはremoveが削除する。
- 未検証ファイルを確定モデルとして使用しない。
- 実装にRange、sidecar、validator、backup、再開処理が含まれない。
- Leanが未検証公開とキャンセル後の公開許可を拒否し、壊れた遷移の固定witnessが安全条件の検出能力を確認する。
- Lean生成fixtureとRust oracleが、検証、キャンセル、公開、removeの対応をstrict modeで確認する。
