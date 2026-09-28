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
