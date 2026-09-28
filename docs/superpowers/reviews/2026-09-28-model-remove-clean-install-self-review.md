# モデル削除と失敗時クリーンアップ セルフレビュー

## 対象

- `docs/superpowers/specs/2026-09-28-model-remove-clean-install-design.md`

## 第1巡: 要件レビュー

確認対象は、会話で承認された次の決定である。

- 再開機能を実装しない。
- 通常の失敗では部分ファイルを削除する。
- `model remove`を追加する。
- 使用中のremoveは待たずにエラーを返す。
- Leanで状態管理の安全性を証明する。

設計書はRange、sidecar、validator、backup、同一実行内の再接続を対象外とした。install、remove、transcribeの競合と、SIGKILLや電源断では同期的なcleanupを保証できない境界も記載した。未承認機能の追加は見つからなかった。

指摘: 「installは毎回先頭から取得する」という目的は、有効な確定モデルがあれば通信せず成功する処理と文面上矛盾していた。

修正: 「取得が必要な場合にoffset 0から開始する」と限定し、再開しない契約と既存モデルの再利用を両立させた。

## 第2巡: 状態・安全性レビュー

installの開始、既存finalの検査、stale partialの削除、取得、全体検証、公開、通常失敗、cleanup失敗、強制終了を順に確認した。検証前のpartialを確定名へ移す経路はない。invalid finalを通信前に削除するため、install失敗後に不正なfinalを再び利用する経路もない。

removeは非待機の排他ロック取得後だけファイルを削除する。transcribeは共有leaseを保持するため、使用中のモデルをremoveが削除する経路はない。cleanup失敗ではpartialが残り得るが、未検証のpartialを確定名へ移さず、次回のinstallまたはremoveが再度削除を試みる。既存の検証済みfinalと外部要因で残ったpartialが同時に存在する場合、partialのcleanup失敗は既存finalを変更しない。

Leanの定理は抽象状態の不変条件に限定し、OSのrename、unlink、file lockを証明対象に含めていない。これらをRustテストへ割り当てたため、形式証明の主張範囲と実装検証の役割は一致している。

指摘1: install手順が既存の不正finalをいつ削除するかを曖昧にすると、失敗後に不正finalが残る。

修正1: stale partialの削除と有効finalの検証後、不正finalを通信前に安全に削除する順序を明記した。

指摘2: 「cleanup失敗でpartialが残ってもpublishedはfalse」という定理案は、既存の検証済みfinalとstale partialが同時に存在し、stale partialの削除に失敗する状態には成り立たない。

修正2: cleanup失敗はpublicationを変更せず、未検証モデルを公開しないという定理へ変更した。既存の検証済みfinalを保持する状態も表現できる。

指摘3: removeが確定モデルを先に削除すると、その後のpartial削除失敗によってpartialだけが残る。

修正3: partialを先に削除し、成功した場合だけ確定モデルを削除する順序に固定した。removeのOS操作失敗はRustテストで確認する。

## 第3巡: 実装品質レビュー

保存形式、公開API、エラー分類、テスト、利用者文書、CIを確認した。旧設計の生成corpus、oracle、bounded audit、監査報告は対象外として明記されている。決定的な `.part` とモデル別lock fileだけを追加するため、cleanup対象を列挙できる。

指摘: Windowsのrenameが既存finalを置換できない問題をbackupで解決すると、旧設計の複雑さが戻る。

修正: 有効finalは再取得せず、不正finalは通信前に削除する設計とした。公開時には確定パスが存在しないため、backupを使わない。

placeholder、TBD、未定の選択肢、設計内部の矛盾は残っていない。設計対象はモデル削除、失敗時cleanup、必要なロック、限定したLean証明に収まっている。

## 実装計画書

対象は `docs/superpowers/plans/2026-09-28-model-remove-clean-install.md` である。

### 第1巡: 要件レビュー

設計書の完了条件を6個のTaskへ対応づけた。モデル削除と非待機の使用中判定はTask 2と5、失敗時cleanupと再開機構の不在はTask 3、transcribe中のleaseはTask 4、Lean証明はTask 1、利用者文書とCIはTask 6が扱う。各Taskは失敗テスト、実装、成功確認、3巡のレビュー、コミットの順に進む。

指摘: URLと完全なローカルpathを表示しない制約はGlobal Constraintsにあったが、検出するテストがTask 3になかった。

修正: `errors_and_logs_do_not_expose_url_or_full_path`をTask 3へ追加し、reqwest errorは通信段階の分類、filesystem errorは操作種別と`ErrorKind`へ正規化する手順を明記した。

### 第2巡: 状態・型レビュー

install、remove、leaseの状態変化をTask間で追跡した。Task 2がlockとleaseを定義し、Task 3が同じ排他lockの内側でcleanup、検証、公開を実行する。Task 4はtranscriberとdiarizerにleaseを所有させ、Task 5はCLIへremoveとcancellationを接続する。この順序では、後続Taskが未定義の型やAPIへ依存しない。

指摘1: `publishVerified`というLean eventは、SHA-256計算までLeanで証明するように読めた。

修正1: このeventはRust側で行う全体検証とrenameをまとめた抽象遷移であり、ハッシュ計算とOS操作はRustテストの範囲だと明記した。

指摘2: Task 2から6の一部に変更対象の略記があり、Task 4のconstructorには引数名と戻り値がなかった。

修正2: 変更対象をすべてリポジトリ相対pathで列挙し、`ModelLease::path`とWhisper、Sherpaのconstructorを完全なsignatureで記載した。

### 第3巡: 実行可能性と文書量のレビュー

6個のTaskにRED、GREEN、3巡レビュー、コミットの各段階がある。3種類のレビュー手順は各Taskに1個ずつ、合計18個ある。Review Focusの5項目には、それぞれTask 2または3のテストが対応する。

指摘1: 初稿は650行あり、同じ制約と確認項目を複数の節で繰り返していた。

修正1: 共通事項をGlobal ConstraintsとReview Focusへ集約し、計画を477行へ縮めた。各Taskには対象file、interface、test名、command、commit境界を残した。

指摘2: formal workflowに指定していたLean Action `v1.6.0`は、公式READMEで推奨されている指定と一致しなかった。

修正2: `leanprover/lean-action@v1`を使い、`formal`をLake package directoryとしてbuildだけを実行した後、`lake exe YasumaroTests`を明示的に実行する構成へ変更した。

計画書にはplaceholder、TBD、未決定の分岐がない。実装時に新しい要件が見つかった場合は計画へ無断で追加せず、承認済み設計との差として記録する。

## Task 1: Leanモデルと安全性証明

### 第1巡: 要件レビュー

`ModelLifecycle.lean`は、確定モデルの存在と検証状態、部分ファイル、reader数、writerの5成分だけを保持する。eventはinstall開始、部分ファイル作成、検証済み公開、cleanup成否、lease取得・解放、removeに限定した。HTTP Range、validator、backup、再試行、bounded search、ライフサイクル用fixture生成は含まない。既存の`TestVectors.lean`は話者割り当て用であり、今回の状態機械には接続していない。

指摘: Leanでは`partial`が予約語であり、設計書どおりのfield名をそのまま宣言できなかった。

修正: fieldをescaped identifierの`«partial»`として宣言した。Leanが表示する外部名と状態成分の意味は`partial`のままである。

### 第2巡: 状態・安全性レビュー

`step_preserves`は全eventを場合分けし、`published = true`なら`publishedVerified = true`、`writer = true`なら`readers = 0`という二つの不変条件を保持する。`run_preserves`は任意のevent列へこの結果を拡張する。cleanup成功・失敗、busy remove、remove成功、remove冪等性は任意の`State`について個別の定理で確認した。

指摘: `run_preserves`は初期状態の安全性を仮定するが、検証済みfinalと任意のpartial・reader数を持つ初期状態が`Safe`であることを接続する定理がなかった。

修正: `verified_initial_safe`を追加し、partialの有無とreader数を制限せず、writerが存在しない検証済み初期状態が`Safe`であることを証明した。`YasumaroTests.lean`から定理を参照し、定理追加前の失敗と追加後の成功を確認した。

### 第3巡: 実装品質レビュー

各定理は固定fixtureではなく、引数で受け取った任意の状態またはevent列を扱う。`sorry`、`admit`、`native_decide`、`maxHeartbeats 0`は使用していない。`lake build`は警告なしで成功し、`lake exe YasumaroTests`は初期状態、cleanup、busy remove、remove成功、冪等性の実行例を検査する。

指摘: 最初の証明では未使用のsimp引数、不要な`simpa`、deprecatedな`if_pos`による警告が出た。

修正: 明示的な場合分けは維持し、簡約手順だけを整理した。READMEにはLeanが証明する抽象状態と、Rustテストで検査するハッシュ計算、rename、unlink、OS lockの境界を記載した。

## Task 2: ModelStoreのlock、lease、remove

### 第1巡: 要件レビュー

`ModelStore`は確定モデル、`.part`、`.locks/<model>.lock`だけを作成・削除対象にする。`remove`は`try_lock_exclusive`を一度だけ呼び、`Ok(false)`だけを`ModelInUse`へ分類する。lock directoryの作成、lock fileのopen、lock APIの失敗は`Lock`となる。lock fileはremove後も残し、次の操作で再利用する。

指摘: Task 3だけが使うblocking版`lock_exclusive`をTask 2で実装するとdead-code警告が発生し、公開API化またはallow属性が必要になる。

修正: lock fileの作成とerror正規化はTask 2で実装し、blocking wrapper本体だけTask 3のinstaller接続時に追加する判断をledgerへ記録した。利用者向けAPIを計画上の都合で広げていない。

### 第2巡: 状態・安全性レビュー

`acquire`は共有lock取得後に`symlink_metadata`で確定モデルを再検査し、regular fileだけを`ModelLease`へ格納する。`remove`は排他lock取得後にpartial、finalの順で処理する。partialが非空directoryで削除できないテストでは、finalが保持される。lease中のremoveは1秒未満で`ModelInUse`を返し、状態を変更しない。symlinkはリンク自体を削除し、外部targetを保持する。directoryは`remove_dir`だけを使うため再帰削除しない。

指摘: UnixとWindowsではdirectory symlinkの削除APIが異なる。`symlink_metadata`の`is_dir()`だけで分岐すると、Windowsのdirectory symlinkを`remove_file`へ渡す。

修正: platform-independentな削除分類を追加し、Windowsでは`FileTypeExt::is_symlink_dir`を使って`remove_dir`へ分類する。分類testを実装前に失敗させ、実装後に成功させた。

### 第3巡: 実装品質レビュー

partial pathは`OsString`へ`.part`を追加するため、非UTF-8のmodel rootを文字列へ変換しない。lockとstorage errorは操作名と`ErrorKind`だけを保持し、完全なローカルpathを含めない。publicな`acquire`、`remove`、`ModelLease::path`には契約とerror条件を記載した。Clippyの`filetype_is_file`は、symlinkと特殊fileを拒否する要件により意図的に許可し、理由を属性へ記載した。

Windows targetのcross-checkは、crate本体へ到達する前にローカル環境へWindows SDK headerがないため`ring`のbuild scriptで停止した。この結果はWindows成功として扱わない。Windows固有分岐は公式の`FileTypeExt`に限定し、最終判断はWindows CIへ残す。現在のplatformでは全target・全featureのClippyとruntime testが成功した。

## Task 3: Fresh downloadと失敗時cleanup

### 第1巡: 要件レビュー

installerはモデルごとの排他lock内でstale partialを削除し、既存finalの全体サイズとSHA-256を検証する。有効なfinalは通信せず再利用し、不正finalは通信前に削除する。取得が必要な場合は無条件GETを一度だけ送り、`create_new`で作成した決定的な`.part`へoffset 0から書く。Range、validator、sidecar、backup、retry loopは追加していない。旧`model_store.rs`のnetwork testは`model_download.rs`へ分離した。

指摘1: reqwest 0.12.28のblocking builderには`read_timeout`がなく、計画のAPI名をそのまま実装できなかった。

修正1: blocking responseの各`Read`を期限付きにする`ClientBuilder::timeout`へread timeout値を渡し、connect timeoutは`connect_timeout`で別に設定した。この判断と、requestの他段階も同じ期限の対象になる差をledgerへ記録した。

指摘2: reqwestの既定redirect追従は、同じinstallで複数requestを発生させる。

修正2: 302応答で二つ目のrequestを観測するテストを先に失敗させた。redirect policyを`none`にし、2xx以外を明示的に`Download`へ分類した後、request数が1でGREENになることを確認した。

### 第2巡: 状態・安全性レビュー

stale cleanup、valid final、invalid final、接続失敗、body中断、read timeout、midstream cancellation、過大応答、hash不一致、publish失敗を個別に検査した。すべての通常失敗は共通cleanup関数を通り、partialを削除する。valid finalとstale partialが併存し、stale partialが非空directoryで削除できない場合は通信せず、finalを保持する。同時installは同じ排他lockで直列化され、server requestは一度だけになる。

指摘: 元のpublish errorとcleanup errorを両方保持するtestはhelperと同時に追加したため、失敗検出力を観測していなかった。

修正: cleanup errorを捨てて元errorだけ返す変異を一時的に入れ、`publish_and_cleanup_failure_preserves_both_errors`が失敗することを確認した。実装を戻した後は`CleanupFailed`の`source`と`cleanup`を個別に検査して成功した。

### 第3巡: 実装品質レビュー

production timeoutはconnect 30秒、blocking read 60秒である。testでは30ミリ秒のread timeoutとchannelで制御したmidstream cancellationを使い、`Ordering::Release`で書いたflagを`Ordering::Acquire`で読む。ローカルserverのsocketは2秒で期限切れになり、全server testの固定上限も検査する。HTTP errorはtimeout、connect、status、bodyへ分類し、filesystem errorは操作名と`ErrorKind`へ正規化するため、URLと完全なローカルpathを表示しない。

指摘: 旧testは、manifestと一致しない既存finalをdownload失敗後も保持する契約だった。この契約では次のtranscribeが不正finalを使い得る。

修正: 旧testを削除し、不正finalを通信前に削除してdownload失敗後も残さない契約へ置き換えた。検証済みfinalを保持する経路は、manifestと一致するbytesを使う別testで確認する。

## Task 4: Transcribe中のモデルlease

### 第1巡: 要件レビュー

WhisperとSherpaのconstructorはraw pathではなく`ModelLease`を所有する。Whisperは選択されたモデル、Sherpaは話者区間モデルと話者埋め込みモデルを保持する。production、test-support、ignored評価testの全constructor呼び出しを`ModelStore::acquire`へ変更した。`model list`と`doctor`は従来どおりlocal snapshotの`require`を使い、暗黙downloadは追加していない。

`whisper_transcriber_holds_the_model_lease_until_drop`は、transcriberが存在する間のremoveが`ModelInUse`となり、drop後のremoveが成功することを実際のfile lockで確認する。承認済み設計との差は見つからなかった。

### 第2巡: 状態・安全性レビュー

Whisperはchild processの起動から終了確認まで`self.model`を借用するため、その全期間を含むtranscriberの生存中はleaseが解放されない。Sherpaはnative engineを最初のfield、二つのleaseを後続fieldに置いた。Rustのfield破棄順により、native engineを破棄した後にleaseを解放する。

指摘: constructorが引数を所有した後に失敗する経路ではRustのdropによりleaseが解放されるが、この条件を回帰testが固定していなかった。また、Sherpaのfield順序が安全条件であることがコードから読み取りにくかった。

修正: Whisperは一時directory不正、Sherpaはthread数不正でconstructorを失敗させ、その直後に両モデルをremoveできるtestを追加した。Sherpaにはnative fieldをleaseより先に置く理由をコメントした。

### 第3巡: 実装品質レビュー

`rg`で`WhisperProcessTranscriber::new`、`new_for_test`、`SherpaDiarizer::new`の全呼び出しを列挙した。いずれも`acquire`で得たleaseを渡し、bare model pathを渡す箇所は残っていない。保持専用のSherpa fieldは`_segmentation`と`_embedding`とし、native configの再設定に必要なUTF-8 path文字列とは役割を分けた。

`cargo check --workspace --all-targets --all-features --locked`はignored評価testを含めて成功した。公開constructorのerror説明、import、未使用field、完全pathのerror露出に新たな不整合は見つからなかった。

## Task 5: CLI remove、install cancellation、error表示

### 第1巡: 要件レビュー

`model remove`はClapで一つ以上のmodelを必須とする。入力を`ModelId`へ変換し、deriveした全順序でsortして重複を除き、一つずつ`ModelStore::remove`へ渡す。引数なしはexit 2となる。重複指定の成功表示は処理した一意model数を使う。

removeは最初のerrorをそのまま返し、後続modelを処理しない。実fileと別processの共有leaseを使うtestで、成功したprefixを戻さず、busy modelを変更せず、後続modelも残すことを確認した。installはtranscribeと同じ`cancellation_flag`を使い、flagを`install_with_cancellation`へ渡す。要件から外れるrollback、待機remove、暗黙installは追加していない。

### 第2巡: 状態・安全性レビュー

入力順がmedium、small、baseでも、処理順は`ModelId`のbase、small、mediumになる。smallのleaseを保持した状態ではbaseの削除だけが確定し、smallで`ModelInUse`となり、mediumは未処理になる。重複IDはremove前に除くため、同じ状態遷移を二度実行しない。

Ctrl+Cはinstallとtranscribeの双方で`Release` storeする同じ生成関数を使う。runtime側は`Acquire` loadし、installのpartial cleanup後に`Cancelled`を返す。直接の`ModelError::Cancelled`だけをexit 130とし、cleanup自体にも失敗して`CleanupFailed`となった場合はstorage/model errorのexit 4を維持する。状態遷移と終了コードの矛盾は見つからなかった。

### 第3巡: 実装品質レビュー

Clapが表示する`model remove <MODELS>...`と全model choiceを確認した。`ModelInUse`のhelpは対象IDを含む再実行commandと待つべき処理を示す。`CleanupFailed`は対象IDのremove、`Cancelled`は対象IDのinstallを案内する。既存のengine、output、runtime errorのexit codeとhelpは変更していない。helpはmodel IDだけを組み立て、URLや完全なlocal pathを追加しない。

指摘: 親の`model --help`へremoveが掲載されることは目視確認だけで、回帰testに固定されていなかった。

修正: 既存のmodel help testへremoveの掲載確認を追加した。全`ModelError`はCancelledの個別分類またはmodel errorの包括分岐で処理され、match漏れはない。

検証時にClippyがmodel cancellationとruntime cancellationの同一結果を別armにした点を検出したため、両patternを一つのexit 130 armへ統合した。分類の意味は変えていない。

## Task 6: 文書、formal CI、全体検証

### 第1巡: 要件レビュー

READMEは`model remove`に一つ以上のmodel名が必要であること、使用中は待機せず失敗すること、取得が必要なinstallはoffset 0から始めることを説明する。通常の通信、検証、cancel失敗では`.part`を削除し、強制終了や電源断で残った場合だけ次回のinstallまたはremoveが削除を試みる。Range再開、sidecar、backupを利用者機能として記載していない。

READMEのcommand例はClap parser test、ライフサイクル説明は本文契約testで固定した。設計書と実装計画書へのlinkを開発文書へ追加した。Issue本文や外部状態は変更していない。

### 第2巡: 状態・安全性レビュー

Leanの`step_preserves`と`run_preserves`は、検証前の公開禁止とreader・writer排他を任意のevent列へ拡張する。cleanup、busy remove、remove成功、冪等性の個別定理をRustの`model_download`、`model_lifecycle`、`model_cli` testへ対応づけた。LeanがSHA-256計算、rename、unlink、symlink、OS lock、強制終了cleanupを証明しないことをformal READMEとtesting文書の双方に明記した。

指摘: 初稿の対応表はreader・writer排他に同時install testを挙げていたが、このtestが検査するのは二つのwriterの直列化であり、readerとの競合ではなかった。

修正: 共有lease中はinstallがHTTP request前で待機し、lease解放後に取得と置換を完了する`install_waits_for_an_active_model_lease`を追加した。対応表をlease中removeとlease中installの二つへ修正し、証明と実装testの対応を過大に表現しないようにした。

### 第3巡: 実装品質レビュー

formal workflowは`pull_request`、`merge_group`、`workflow_dispatch`で起動し、Ubuntu 22.04、20分上限、read-only contents permissionを使う。`leanprover/lean-action@v1`へ`formal` package、auto-config無効、build有効、testとlint無効を渡し、その後に`formal`をworking directoryとして`lake exe YasumaroTests`を実行する。

開発文書とtesting文書の相対linkは実在するfileを指す。README parser testは新しいremove例を含む。今回のmodel lifecycle実装に`If-Range`、`Content-Range`、`part.json`、publish backupはなく、該当語は設計・計画の対象外説明または無関係な出力fileのatomic writeに限られる。placeholderや未決定事項は追加していない。

## 最終ブランチレビュー

### 第1巡: 要件レビュー

基点`de0ec73`からの8 commitを設計書の完了条件へ対応づけた。設計・計画は`d8d2f8d`と`04223b3`、Lean状態機械は`3740993`、lock・lease・removeは`b3a8082`、fresh downloadとcleanupは`7296474`、transcribe leaseは`969b780`、CLI removeとcancelは`b06a492`、文書とformal CIは`6728722`にある。

fresh reviewerは、remove、cleanup、lock分類、非再帰削除、lease drop順、CLIのsort・dedup・途中停止、URLとpathの非露出を要件どおりと判定した。一方、`ModelStore::acquire`がregular fileだけを検査しており、マニフェストと異なるfileを文字起こしへ渡せるというImportantを指摘した。

修正: `ModelStore::acquire`は`ModelSpec`を要求し、共有lock取得後に全体サイズとSHA-256を検証してからだけ`ModelLease`を返す。Whisper、話者区間、話者埋め込みの3経路をverified leaseへ変更した。`acquire_rejects_corrupt_regular_files_for_all_transcription_models`は旧実装で型不一致のRED、新実装でGREENになった。

### 第2巡: 状態・安全性レビュー

fresh reviewerは、CLIがCtrl+Cをflagへ変換した後もinstallがblocking lock取得で待ち続け、valid finalの検証中と成功return前にcancelを確認しないImportantを指摘した。この状態では長いtranscribeの終了までcancelが反映されず、その後に成功を返す可能性があった。

修正: installは25ミリ秒間隔の非待機排他lock試行へ変更し、各試行前、lock取得直後、既存finalの各hash chunk、valid finalの成功return前にflagを確認する。待機中のcancelでは他processのpartialへ触れず`Cancelled`を返す。`cancelled_install_stops_waiting_for_an_active_model_lease`と`pre_cancelled_install_does_not_reuse_a_valid_final`はいずれも旧実装でRED、新実装でGREENになった。通常のinstallがlease解放後に成功する既存testも再確認した。

Leanの`publishedVerified`とRustのlease境界は、確定名の存在ではなくmanifestとの全体一致で接続された。外部processがlockを無視してlease取得後にfileを書き換える操作は、設計書どおり状態機械の対象外である。

### 第3巡: 実装品質レビュー

fresh reviewerのCriticalは0件だった。Minorとして、HTTP client構築errorがrequest前のため常に`whisper-base`を表示する点が残った。これは通常のHTTP・filesystem error分類やmodel lifecycle状態を変えず、client builderが失敗する稀な初期化経路の表示精度に限られるため、今回の一回のfix passには含めない。

reviewerが判断を留保したWindows固有のlock・rename・directory symlink、SIGKILL時点のdurability、実native inference、disk-full・permission・`sync_all`の実faultは、ローカル成功として扱わない。SIGKILL時点のcleanupは承認済み対象外であり、次回cleanupだけを保証する。実model依存の5 testはignoredのままである。Windows、Linux、macOSのCIはbranchをremoteへ送っていないため未実行であり、merge前の必須確認として残す。

修正後の`cargo fmt`、全target・全featureの厳格Clippy、workspace全testは成功した。workspace testは179件成功、実model・fixture依存の5件だけがignoredだった。Lean buildと`YasumaroTests`も成功し、禁止したresume、sidecar、backup用実装は見つからなかった。

## キャンセル境界と形式モデルの設計改訂

### 第1巡: 要件レビュー

改訂後もinstallはoffset 0から開始し、Range、validator、sidecar、backupを使わない。追加する状態は実行中の検証、キャンセル、公開許可を表すメモリ上の値だけであり、再開用の永続状態ではない。remove、非待機busy、通常失敗時cleanup、強制終了後の次回cleanupという承認済み要件も変更していない。

接続待ちとbody停止中のキャンセルを監視対象へ加えた。通常ファイルへのwriteと`sync_all`は安全に中断できないため、各呼び出しが戻った直後にflagを確認する。この制約を明記し、すべての処理を250ミリ秒以内に止めるという実装不能な契約にはしていない。

### 第2巡: 状態・安全性レビュー

初稿では`partialVerified`と`publishAuthorized`を追加した一方、初期状態の安全条件へ両者を含めていなかった。安全条件を`partialVerified → partial`、`publishAuthorized → partialVerified ∧ writer`まで拡張し、プロセス開始時は検証済みpartial、公開許可、キャンセル要求をfalseと定義した。

キャンセルとrenameの競合には公開許可を置いた。公開許可前のキャンセルはcleanupへ進み、許可後のキャンセルは許可を取り消さない。Leanでは検証、キャンセル要求、公開許可、公開を別eventにし、未検証または許可前キャンセル済みのpartialを公開できないことを証明する。壊れた公開許可遷移の固定witnessも残す。

### 第3巡: 実装品質レビュー

HTTP処理だけを非同期化し、`ModelInstaller`の同期公開APIは維持する。production関数内の公開許可前後に`test-support`の同期点を置き、Rust adapterが同じ処理を観測できるようにした。Lean生成fixtureは`strict`、`internal-fixture`、`model-only`を区別し、期待値をRust側へ重複記述しない。

HTTP clientと非同期runtimeの構築失敗はモデルを選ぶ前に起きるため、モデルIDを持たない`HttpClientInitialization`へ分類した。内部error文字列、URL、完全pathは表示しない。GitHub Issue本文の変更は外部操作として実装範囲に含めず、branch完成時にコメント案だけを提示する。

## キャンセル境界と形式対応の実装計画

### 第1巡: 要件レビュー

計画の5 Taskを設計書の完了条件へ対応づけた。Task 1は状態機械と証明、Task 2はLean生成fixture、Task 3はネットワーク待機のキャンセル、Task 4は公開許可とRust oracle、Task 5はCIと文書を担当する。Range、validator、sidecar、backup、再開処理、GitHub Issueの外部変更を実装するstepはない。

### 第2巡: 状態・安全性レビュー

Task間のinterfaceは`State → Event → State`の壊れた遷移、schema version 1のfixture、公開許可前後のcheckpoint、oracleの3 modeで固定した。公開許可前と許可後のキャンセルを別testにし、同じflag変化を異なる期待結果へ対応づけた。未検証partialはLeanのnegative caseとRustのhash mismatch caseの双方でfinalを作らない。

### 第3巡: 実装品質レビュー

Review Focusの5項目には、それぞれTask 3またはTask 4のtest名と実行commandがある。read timeoutより短い間隔で進むresponseは旧実装でも成功するため、REDではなく非同期化前後のcharacterization testとして記録した。各Taskはtest追加、RED確認、最小実装、GREEN確認、3巡レビュー、commitの順になっている。placeholder、未定義の後続判断、期待値をRustへ重複記述するstepはない。

## 検証、キャンセル、公開許可を分離したLean状態機械

### 第1巡: 要件レビュー

状態には`partialVerified`、`publishAuthorized`、`cancelRequested`だけを追加し、HTTP Range、validator、sidecar、backup、再開offsetなどの永続状態は追加していない。公開は検証成功、未キャンセルの公開許可、公開の三段階に分けた。公開許可前のキャンセルでは許可と公開がno-opになり、公開許可後のキャンセルでは許可が維持される。

指摘: 公開許可前キャンセルの最初の実行例は`published` fieldだけを比較していたため、誤って他の状態を変更する遷移を検出できなかった。

修正: 未公開の初期状態からイベント列を実行し、未公開、検証済みpartial、未許可、キャンセル済み、writer保持中という最終状態全体を比較するようにした。

### 第2巡: 状態・安全性レビュー

`Safe`は、検証済みfinal、partial検証とpartial存在、公開許可とpartial検証・writer保持、writerとreader排他の四条件を持つ。`step_preserves`は全11種類のeventと成立・不成立分岐を扱い、`run_preserves`が任意のevent列へ保存結果を拡張する。cleanup成功はpartial関連状態を消去し、cleanup失敗はpartialと検証状態を保持したまま公開許可とwriterを消去する。remove成功は全artifact状態を消去する。

指摘: 自動簡約だけに依存した初稿では、Bool条件が過剰に書き換えられ、no-op分岐と状態更新分岐の対応が証明項から読み取りにくかった。

修正: 各条件を`ite_eq_left`または`ite_eq_right`で明示的に選び、更新分岐ごとに四つの安全条件を構成した。未検証partialとキャンセル済みpartialを許可する`brokenAuthorizeStep`には固定witnessを置き、正常遷移の安全性と壊れた遷移の非安全性を同じ定理で確認した。

### 第3巡: 実装品質レビュー

定理名は後続fixtureが参照する契約どおりで、実行例は検証なし、許可前キャンセル、許可後キャンセルを別々に検査する。`sorry`、`admit`、`native_decide`は追加していない。旧`publishVerified` eventは残しておらず、検証と公開を一段で通過する経路もない。

指摘: 証明の初回GREENにはdeprecatedな`if_pos`と`if_neg`の警告が残った。

修正: Lean 4.34の`ite_eq_left`と`ite_eq_right`へ置き換え、警告なしのbuildを完了条件とした。

## Lean生成fixture

### 第1巡: 要件レビュー

fixtureは正常公開、hash不一致、公開許可前キャンセル、公開許可後キャンセル、busy remove、remove成功の6 production対応caseと、壊れた公開許可の1 sensitivity caseだけを含む。modeは`strict`、`internal-fixture`、`model-only`に限定し、Range、再開、validator、backupに対応するcaseは追加していない。schema versionは1、case順序と期待結果は`YasumaroTests`で固定した。

指摘: 計画のgenerator commandは`lake -d formal`を実行すればcwdも`formal/`へ移る前提で`../crates/...`を指定していたが、Lakeはrepository rootのcwdを維持した。

修正: repository rootから実行するcommandは`crates/yasumaro-runtime/tests/fixtures/lean-model-lifecycle.json`へ統一した。generatorはcallerの相対pathをそのまま扱い、特殊なpath解決規則を持たせていない。この差は実行台帳へRulingとして記録した。

### 第2巡: 状態・安全性レビュー

各caseの`expected`はprivateな`makeCase`が`run start events`から計算し、呼出側が独立した期待状態を書けない。Lean実行testでも全caseについて`expected = run start events`を再確認する。壊れた期待状態はsensitivity caseだけに存在し、同じstartとeventsを`brokenAuthorizeStep`へ渡して生成する。JSONにはstart、event列、正常期待状態、壊れた期待状態をすべて出力する。

指摘: 初稿のテストはfield値を確認したが、rendererが不正なJSONを生成する変異を検出できなかった。

修正: `Lean.Json.parse modelLifecycleTestVectorsJson`の成功を実行testへ追加し、生成後は`jq`でもschema version 1、7 case、mode、期待結果を確認した。

### 第3巡: 実装品質レビュー

generatorは標準出力、`--output`、`--check`を持ち、`--check`の一致は0、不一致または読取失敗は1、不正引数は2を返す。不正引数と存在しないfixtureを実行し、それぞれexit 2と1を観測した。読取・書込errorは固定文言へ正規化し、指定pathを出力しない。

指摘: 最初の`--output`失敗は`IO.FS.writeFile`の未処理例外により完全pathを表示した。

修正: 書込も専用関数で捕捉し、固定文言とexit 1へ正規化した。存在しない親directoryを指定する実行testで、path非露出と終了コードを確認した。生成fixtureのfreshness checkはfileを書き換えずに成功した。

## 接続待ちとbody待ちのキャンセル

### 第1巡: 要件レビュー

`ModelInstaller::{install, install_with_cancellation}`は同期公開APIのまま維持し、モデル取得だけを非同期reqwest clientとcurrent-thread Tokio runtimeへ移した。productionのconnect timeoutは30秒、read timeoutは60秒、キャンセル監視は25ミリ秒間隔である。requestは従来どおり無条件GETを一度だけ送り、redirect、Range、validator、sidecar、backup、再試行を追加していない。

指摘: reqwestの`blocking` featureを計画どおりcrateから外すと、対象外の`engine_artifact::install`がcompileできなかった。

修正: `model::download`はasync `reqwest::Client`だけをimportして使い、crate-wide featureは既存engine installのため保持した。engine実装を今回の範囲へ巻き込まない判断を実行台帳へ記録した。

### 第2巡: 状態・安全性レビュー

response header待機と各body chunk待機は、HTTP futureとキャンセル監視futureを`tokio::select!`で競合させる。headerまたはchunkが先に完了した場合も、直後にflagを再確認する。chunk受信、file write、EOF、`sync_all`、metadata、size/hash検証の境界でも確認し、検出したerrorは従来の共通cleanupを通ってpartialを削除する。排他lockはsend、body取得、書込、検証、cleanupの全期間で保持する。

指摘: 最初のbody停止testはserverが先頭chunkを書いた時点でflagを設定しており、clientがchunkを処理する前なら旧blocking実装でも次のreadに入らず成功した。

修正: partial fileへ先頭chunkが書き込まれたことを固定上限付きで観測してからflagを設定するようにした。これにより旧実装ではheader・bodyの両testが250ミリ秒以内に完了せずRED、非同期化後はserverを再開させる前に`Cancelled`となりGREENになった。

### 第3巡: 実装品質レビュー

current-thread runtimeはtime driverに加えてI/O driverを明示的に有効化し、Tokio dependencyは`macros`、`net`、`rt`、`time`だけを指定した。`read_timeout`は各readの無進捗時間へ適用し、40ミリ秒ごとに進む総時間200ミリ秒のresponseが80ミリ秒のread timeoutでも成功するtestを通した。stalled serverはchannelと2秒上限で必ず解放され、header testは解放後にsocketへ書かず接続を閉じるため、client側cancel後のbroken pipeに依存しない。

指摘: 初回runtimeはtime driverだけを有効にしていたため、async reqwestがsocketを作る時点でpanicした。またheader完了とcancelが同時の場合、HTTP statusをflagより先に分類していた。

修正: `enable_io()`とTokio `net` featureを追加し、send完了直後はstatus分類前に`check_cancelled`を呼ぶようにした。runtime/client構築errorは内部error、model ID、URLを持たない`HttpClientInitialization`へ正規化した。runtime全target・全featureの厳格Clippyと全testは成功した。

## 公開許可境界とRust oracle

### 第1巡: 要件レビュー

公開処理は同じproduction関数内でBeforeAuthorization observer、最後のキャンセル確認、AfterAuthorization observer、renameの順に実行する。通常buildのobserverはno-opで、checkpoint enum、observer field、専用constructorの外部公開は`test-support` featureに限定した。許可前testはflag設定後に`Cancelled`、final不在、partial cleanupを確認し、許可後testはflag設定後も成功してmanifest検証済みfinalを取得できることを確認した。

oracleはコミット済みLean fixtureを`include_str!`で読み、公開`ModelInstaller::install_with_cancellation`、`ModelStore::acquire`、`ModelStore::remove`とtest-support checkpointだけを使用する。resume、Range、validator、backup用scenarioは持たない。

### 第2巡: 状態・安全性レビュー

6件のproduction対応は次のように接続した。verified-publishはinstall成功、unverified-publishはhash不一致とcleanup、cancel-beforeはBeforeAuthorizationでflag設定、cancel-afterはAfterAuthorizationでflag設定、busy-removeは共有lease保持中の非待機remove、remove-successはfinalとpartialの削除である。初期artifact、cancel flag、reader lease数はfixtureの`start`から準備し、期待状態と期待結果はfixtureの`expected`と`expectedResult`だけから比較する。

final存在はregular file、検証状態は`ModelStore::acquire`、partial存在はfilesystem entry、reader数は実際に取得したlease数で観測する。readerがないcaseでは観測後の冪等removeが成功するかを使ってwriter lock解放も確認する。`partialVerified`と`publishAuthorized`は完了後のproduction APIに露出しないため、公開境界testとartifact終状態を組み合わせてfalseへの遷移を検査する。model-only caseはLean生成の正常期待状態と壊れた期待状態が異なることだけを確認し、production adapterでは実行しない。

### 第3巡: 実装品質レビュー

各caseは独立した一時directory、local HTTP server、installer、cancel flagを持つ。accept、socket read/write、checkpoint待機、observer再開、worker結果には固定上限があり、timeoutなしのchannel受信はない。oracleが表示するのはcase名、mode、event名、結果分類、Bool/Nat状態だけで、URL、完全path、内部error文字列を含まない。schema version、mode、unknown case、field差分のunit testを追加した。

指摘: 初稿の状態観測は`ModelStore::acquire(...).is_ok()`で予期しないlock/storage errorまで未検証状態へ丸め、writerを常にfalseと仮定していた。また厳格ClippyはLean schemaのBool数、複雑な環境tuple、regular-file判定、標準出力を指摘した。

修正: manifest不一致と基盤errorを分離し、readerなしcaseはpublicなremoveの成否でwriter解放を観測する。環境tupleは専用structへ変更し、regular-file判定とLean schema・oracle出力だけに理由付きallowを限定した。strictは単独実行と5回連続実行ですべて6件match、reportは7件すべてmatchとなった。

## CI、文書、全体検証

### 第1巡: 要件レビュー

Issue #5から承認後に確定した要件を、基点`de0ec73`以降のcommitへ対応づけた。`b3a8082`と`b06a492`が非待機の`model remove`、`7296474`がoffset 0からの取得と通常失敗時cleanup、`969b780`と`373bbd4`が使用中モデルのleaseと検証、`ec18a3f`がキャンセルを含むLean状態機械、`369865d`と`dc35fa5`がLean生成fixtureとRust oracle、`d0e8af8`がネットワーク待機中のキャンセルを実装する。再開用のRange、validator、sidecar、backup、永続offsetは追加していない。testとoracle内の`resume`という局所名は、停止させたtest threadを再開するchannelであり、download再開処理ではない。

READMEは削除、通常失敗時cleanup、強制終了後の次回cleanup、25ミリ秒間隔のネットワーク待機監視、公開許可前後の競合規則を説明する。Issue本文とコメントは変更していない。

指摘: 公開許可前のキャンセルを常に`.part`削除と終了コード130になると記載すると、cleanup自体も失敗した経路を説明できない。

修正: `.part`削除に成功した場合は130、削除にも失敗した場合はストレージエラーになると明記した。

### 第2巡: 状態・安全性レビュー

Leanの`Safe`は、確定モデルの検証、検証済みpartialの存在、公開許可時の検証済みpartialとwriter保持、readerとwriterの排他を表す。fixtureの7 caseはLeanの`run`から期待状態を生成し、Rust oracleは6件のproduction対応caseを公開APIとtest-support checkpointで観測する。壊れた許可遷移の1件は検出感度だけに使い、production対応として数えない。

対応表は、未検証公開、公開許可前後のキャンセル、busy remove、remove成功を個別のRust testとoracle caseへ結びつける。Leanが扱わないSHA-256計算、rename、unlink、symlink、OS lock、強制終了時cleanupはRust統合testと各OSのCIへ割り当てた。oracleは抽象状態との対応を検査するが、Rust実装全体の形式証明ではないことを文書に明記した。

指摘: `lake -d formal`はrepository rootのcwdを維持する一方、workflowの`working-directory: formal`はcwdを変更する。同じfixtureへの相対pathを一つに統一すると、どちらかが失敗する。

修正: rootから実行する開発文書では`crates/...`、`formal`内で実行するworkflowとformal READMEでは`../crates/...`を使用した。両方のfreshness commandが同じfixtureを検査することを実行確認した。

### 第3巡: 実装品質レビュー

formal workflowはRust 1.98.1を設定し、Lean build、Lean実行test、fixture freshness、oracle単体test、strict対応検査を直列実行する。依存解決を固定するため、Rustのtestとrunには`--locked`を指定した。YAML検査、Rustfmt、全target・全featureの厳格Clippy、workspace全test、Lean buildと実行test、fixture freshness、oracle単体testとstrict実行、`git diff --check`を最終検証項目とした。

指摘: workflowのoracle testには`--locked`があったが、続くstrict実行にはなく、CI中にlockfileとの差を許す指定になっていた。

修正: strict実行にも`--locked`を追加した。

ローカルのworkspace testは失敗0件で、実model、ローカル音声、外部corpusを必要とする5件だけがignoredだった。macOSでの検証結果であり、WindowsとLinuxのlock、rename、directory symlink分岐、SIGKILL時点のdurability、disk-full、permission、実native inferenceは未実行である。branchをremoteへ送っていないためGitHub Actionsも未実行であり、ローカル成功として扱っていない。

## 最終fresh reviewの修正パス

### 第1巡: 要件レビュー

fresh reviewerはCritical 0件、Important 4件、Minor 0件と判定した。実配布URLの302拒否、DNS解決キャンセル後のruntime drop待機、transcribeの共有lock待機とhash検証、default-feature targetのcompile失敗である。いずれも利用者が通常のinstallまたはtranscribeで到達するため、4件ともImportantのまま一回の修正パスへ入れた。

指摘: 「GETを一度だけ送る」をredirectも禁止する意味に解釈した結果、Hugging FaceとGitHub Releasesの配布URLが返す302を拒否し、新規installが失敗する。

修正: 一回の取得試行の中で最大10回のredirectを許可し、HTTPSからHTTPへのdowngradeを拒否した。redirect先から失敗した転送を再試行せず、Range、validator、sidecar、backup、offset再開も追加していない。local serverのtestは旧実装で302を返してRED、修正後は初回とredirect先の2 requestだけで検証済みモデルを公開してGREENになった。

### 第2巡: 状態・安全性レビュー

redirectはpartial作成前のHTTP接続経路だけを変え、検証、公開許可、renameの順序を変更しない。redirect先のbytesも同じmanifest sizeとSHA-256で全体検証する。公開許可前後のLean状態とRust checkpointに変更はない。

指摘1: reqwestの既定resolverが作るblocking DNS taskはrequest futureのcancelでは停止せず、所有runtimeのdropが待ち続けるため、CLIが`Cancelled`を得ても終了が遅れる。

修正1: `ModelInstaller`がruntimeを明示的に所有し、drop時の停止待機を100ミリ秒に制限した。blocking resolverを注入したtestは旧実装で250ミリ秒の受信上限を超えてRED、修正後はresolverを解放する前に`Cancelled`を返してGREENになった。中断不能なresolver threadが戻るまで残り得る制約は設計書へ記載した。

指摘2: transcribeが追加した共有lock取得はblockingで、待機中とモデル全体hash中にCtrl+C flagを確認しない。

修正2: `acquire_with_cancellation`は共有lockを25ミリ秒間隔で試し、取得後の各hash chunk境界でもflagを確認する。writerを保持したtestと、最初のchunk読取後にflagを立てるtestは未定義APIでRED、実装後にGREENになった。CLIは話者分離用2モデル、pipelineはWhisperモデルの取得へ同じflagを渡す。

再確認で、話者モデル取得中の`Cancelled`をmodel installのerrorとして返すと、終了コード130でも再installを促す誤ったhelpが表示される経路を見つけた。transcribe用の取得では`RuntimeError::Cancelled`へ正規化し、130かつinstall helpなしを回帰testで固定した。

### 第3巡: 実装品質レビュー

指摘: `model_download` integration testと`model_lifecycle_oracle` exampleは`test-support`専用APIを無条件に参照し、default-featureの`cargo check --all-targets`がexit 101になった。

修正: 両targetへ`required-features = ["test-support"]`を指定した。再実行したdefault-feature全target checkは成功した。CIにもdefault-featureのworkspace全target checkを追加し、全feature Clippyだけではこの回帰を見落とす構成を解消した。

3巡の再確認では、redirect chainがboundedでHTTPS downgradeを拒否すること、キャンセル追加がremoveの非待機規則や公開許可後の成功規則を変えないこと、test-support APIが通常buildへ露出しないことを確認した。設計書、README、testing文書は実装と同じ境界へ更新した。

workspace再検証では、単独実行で成功するWhisper lease解放testが並列実行で2回続けて`ModelInUse`になった。Rust 1.98のinherentな`File::lock_shared`と、removeが使うfs4の排他lock APIが混在していたため、共有lockもfs4を明示して同じ実装へ統一した。修正後はworkspace全testが成功し、該当test binaryの並列実行も5回連続で成功した。
