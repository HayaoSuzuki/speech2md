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
