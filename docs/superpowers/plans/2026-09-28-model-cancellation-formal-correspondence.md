# モデルキャンセルと形式対応 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** モデル取得のネットワーク待機をキャンセル可能にし、検証、キャンセル、公開の順序をLean生成fixtureとRust実装の対応テストで固定する。

**Architecture:** `ModelInstaller`の同期APIを維持し、内部のHTTP処理だけをTokioと非同期`reqwest::Client`へ置き換える。Leanではpartial検証、キャンセル要求、公開許可、公開を別イベントにし、生成したJSONをRust adapterが公開API経由で検査する。

**Tech Stack:** Rust 1.98.1、Tokio、reqwest 0.12、Lean 4.34.1、Lake、Serde、GitHub Actions

**Spec:** `docs/superpowers/specs/2026-09-28-model-remove-clean-install-design.md`

## Global Constraints

- installはoffset 0から開始し、Range、ETag、Last-Modified、`If-Range`、sidecar、backup、再開処理を追加しない。
- `ModelInstaller::{install, install_with_cancellation}`の同期公開APIを維持する。
- 接続待ちと各body readは25ミリ秒間隔でキャンセルを監視する。接続timeoutは30秒、read timeoutは60秒とする。
- 公開許可前のキャンセルは`.part`を削除して`Cancelled`を返す。公開許可後のキャンセルは許可を取り消さない。
- writeと`sync_all`の実行中は中断せず、各呼び出しが戻った直後にキャンセルを確認する。
- 通常失敗時は`.part`を削除し、cleanup失敗時は元のerrorとcleanup errorを保持する。
- error、log、oracle出力にURL、完全なlocal path、HTTP clientの内部error文字列を含めない。
- GitHub Issue本文とコメントは変更しない。branch完成時にコメント案だけを提示する。
- 各Taskは要件、状態・安全性、実装品質の3巡レビューを記録してからコミットする。

## Review Focus

- TCP接続後にserverがresponse headerを返さない場合、Ctrl+Cから250ミリ秒以内に`Cancelled`となる。Task 3で検査する。
- response bodyが途中で停止した場合、server側の送信再開を待たずに`Cancelled`となり、partialを残さない。Task 3で検査する。
- read timeoutより短い間隔でbodyが進む場合、総時間がread timeoutを超えても成功する。Task 3で検査する。
- 全体検証後かつ公開許可前のキャンセルはfinalを作らない。Task 4で検査する。
- 公開許可後のキャンセルはrename成功時にinstall成功となる。Task 4で検査する。

---

### Task 1: 検証、キャンセル、公開許可を分離したLean状態機械

**Files:**
- Modify: `formal/Yasumaro/ModelLifecycle.lean`
- Modify: `formal/Yasumaro/ModelLifecycleProofs.lean`
- Modify: `formal/YasumaroTests.lean`
- Modify: `docs/superpowers/reviews/2026-09-28-model-remove-clean-install-self-review.md`

**Interfaces:**
- Consumes: 設計書の`partialVerified`、`publishAuthorized`、`cancelRequested`と公開許可規則。
- Produces: `State`の新field、`Event::{verifyPartial, requestCancel, authorizePublish, publish}`、`Safe`、`brokenAuthorizeStep : State → Event → State`、後続Taskが参照する定理。

- [ ] **Step 1: 新しい定理名と実行例を先に追加する**

`formal/YasumaroTests.lean`へ次の検査を追加する。

```lean
#check unverified_partial_cannot_be_authorized
#check cancel_before_authorization_blocks_publication
#check cancel_after_authorization_preserves_authorization
#check broken_authorization_violates_safety
```

実行例は、検証なしの公開許可がno-op、許可前キャンセルが公開を阻止、許可後キャンセルが公開を阻止しないことを`assertEqual`で検査する。

- [ ] **Step 2: REDを確認する**

Run: `lake -d formal build`

Expected: FAIL。新しい定理またはeventが未定義であることを示す。

- [ ] **Step 3: `State`、`Event`、`step`、`Safe`を更新する**

`State`へ次を追加する。

```lean
partialVerified : Bool
publishAuthorized : Bool
cancelRequested : Bool
```

`Event`は`publishVerified`を削除し、`verifyPartial`、`requestCancel`、`authorizePublish`、`publish`へ分ける。`authorizePublish`はwriter、partial、partialVerified、未キャンセルを要求する。`publish`はwriter、partial、partialVerified、publishAuthorizedを要求する。`Safe`は次の四条件の積とする。

```text
published → publishedVerified
partialVerified → partial
publishAuthorized → partialVerified ∧ writer
writer → readers = 0
```

cleanup成功はpartial、partialVerified、publishAuthorizedを消去する。cleanup失敗はpartialとpartialVerifiedを保持し、publishAuthorizedとwriterだけを消去する。remove成功は全artifact状態を消去する。

- [ ] **Step 4: 保存定理と壊れた遷移のwitnessを実装する**

`step_preserves`と`run_preserves`を新しい`Safe`へ更新する。次の定理を追加する。

```lean
theorem unverified_partial_cannot_be_authorized ...
theorem cancel_before_authorization_blocks_publication ...
theorem cancel_after_authorization_preserves_authorization ...
theorem broken_authorization_violates_safety ...
```

`brokenAuthorizeStep`は未検証またはキャンセル済みpartialにも`publishAuthorized := true`を設定する。固定状態を使い、正常遷移は安全条件を保ち、壊れた遷移だけが安全条件を破ることを証明する。

- [ ] **Step 5: GREENを確認する**

Run: `lake -d formal build`

Expected: PASS。`sorry`、`admit`、`native_decide`を追加しない。

Run: `lake -d formal exe YasumaroTests`

Expected: PASS。新しい公開境界の実行例を含む。

- [ ] **Step 6: 3巡レビューを記録する**

要件レビューでは再開用状態を追加していないこと、状態・安全性レビューでは全eventが`Safe`を保存すること、実装品質レビューでは壊れた遷移のwitnessが正常遷移と異なることを確認し、レビュー文書へ記録する。

- [ ] **Step 7: Commit**

```bash
git add formal/Yasumaro/ModelLifecycle.lean formal/Yasumaro/ModelLifecycleProofs.lean formal/YasumaroTests.lean docs/superpowers/reviews/2026-09-28-model-remove-clean-install-self-review.md
git commit -m "formal: model verified cancellable publication"
```

### Task 2: Lean生成fixture

**Files:**
- Create: `formal/Yasumaro/ModelLifecycleTestVectors.lean`
- Create: `formal/ModelLifecycleTestGen.lean`
- Create: `crates/yasumaro-runtime/tests/fixtures/lean-model-lifecycle.json`
- Modify: `formal/lakefile.toml`
- Modify: `formal/YasumaroTests.lean`
- Modify: `docs/superpowers/reviews/2026-09-28-model-remove-clean-install-self-review.md`

**Interfaces:**
- Consumes: Task 1の`State`、`Event`、`run`、`brokenAuthorizeStep`。
- Produces: `modelLifecycleCases`、`modelLifecycleTestVectorsJson`、`model-lifecycle-testgen --output|--check`、schema version 1のfixture。

- [ ] **Step 1: 生成caseの契約を先に追加する**

`formal/YasumaroTests.lean`から未作成の`ModelLifecycleTestVectors`をimportし、case名、mode、正常期待状態、broken期待状態を検査する。caseは次の6件とsensitivity 1件に固定する。

```text
verified-publish               strict / success
unverified-publish             strict / hash-mismatch
cancel-before-authorization    internal-fixture / cancelled
cancel-after-authorization     internal-fixture / success
busy-remove                    strict / model-in-use
remove-success                 strict / success
broken-authorization           model-only / broken-sensitivity
```

- [ ] **Step 2: REDを確認する**

Run: `lake -d formal build`

Expected: FAIL。`Yasumaro.ModelLifecycleTestVectors`が存在しないことを示す。

- [ ] **Step 3: case定義とJSON rendererを実装する**

各caseは`name`、`kind`、`mode`、`scenario`、`start`、`events`、`expected`、`expectedResult`、`brokenExpected`を持つ。`expected`は`run start events`から計算し、broken sensitivityだけ`brokenAuthorizeStep`を使った結果も出力する。mode文字列は`strict`、`internal-fixture`、`model-only`に限定する。

- [ ] **Step 4: generatorとLake targetを実装する**

`model-lifecycle-testgen`は次のinterfaceを持つ。

```text
lake exe model-lifecycle-testgen
lake exe model-lifecycle-testgen -- --output <path>
lake exe model-lifecycle-testgen -- --check <path>
```

`--check`は生成結果と既存fileが異なる場合にexit 1、引数不正はexit 2を返す。

- [ ] **Step 5: fixtureをLeanから生成する**

Run: `lake -d formal exe model-lifecycle-testgen -- --output ../crates/yasumaro-runtime/tests/fixtures/lean-model-lifecycle.json`

Expected: PASS。schema version 1と7 caseを持つJSONを作る。

- [ ] **Step 6: GREENとfreshnessを確認する**

Run: `lake -d formal build`

Expected: PASS。

Run: `lake -d formal exe YasumaroTests`

Expected: PASS。

Run: `lake -d formal exe model-lifecycle-testgen -- --check ../crates/yasumaro-runtime/tests/fixtures/lean-model-lifecycle.json`

Expected: PASS。fixtureを書き換えない。

- [ ] **Step 7: 3巡レビューを記録する**

要件レビューではcaseが承認済み状態だけを扱うこと、状態・安全性レビューでは期待状態をLeanの`run`から生成すること、実装品質レビューではmode、schema、case名とgeneratorのexit codeを確認する。

- [ ] **Step 8: Commit**

```bash
git add formal/Yasumaro/ModelLifecycleTestVectors.lean formal/ModelLifecycleTestGen.lean formal/lakefile.toml formal/YasumaroTests.lean crates/yasumaro-runtime/tests/fixtures/lean-model-lifecycle.json docs/superpowers/reviews/2026-09-28-model-remove-clean-install-self-review.md
git commit -m "formal: generate model lifecycle expectations"
```

### Task 3: 接続待ちとbody待ちのキャンセル

**Files:**
- Modify: `crates/yasumaro-runtime/Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/yasumaro-runtime/src/model/mod.rs`
- Modify: `crates/yasumaro-runtime/src/model/download.rs`
- Modify: `crates/yasumaro-runtime/tests/model_download.rs`
- Modify: `docs/superpowers/reviews/2026-09-28-model-remove-clean-install-self-review.md`

**Interfaces:**
- Consumes: 既存の`ModelInstaller`同期API、`AtomicBool`、30秒connect timeout、60秒read timeout。
- Produces: 非同期`reqwest::Client`、current-thread Tokio runtime、`ModelError::HttpClientInitialization`、キャンセル可能なsendとbody read。

- [ ] **Step 1: ネットワーク待機境界の失敗testを書く**

`model_download.rs`へ次のtestを追加する。

```rust
fn cancellation_stops_waiting_for_response_headers()
fn cancellation_stops_a_stalled_response_body_without_server_progress()
fn progressing_response_is_not_limited_by_the_read_timeout_total()
fn http_client_initialization_error_has_no_model_id_or_internal_message()
```

最初の2件はworker結果をCtrl+C相当のflag設定から250ミリ秒以内に受け取る。body停止testは結果を受け取るまでserverへ続きを送らない。progressing responseは各chunk間隔をread timeout未満、総時間をread timeoutより長くする。初期化errorの表示はmodel ID、URL、内部errorを含まない固定文言とする。

- [ ] **Step 2: REDを確認する**

Run: `cargo test -p yasumaro-runtime --features test-support --test model_download cancellation_stops --locked`

Expected: FAIL。blocking sendまたはreadが250ミリ秒以内に終了しない。

Run: `cargo test -p yasumaro-runtime --features test-support --test model_download http_client_initialization_error_has_no_model_id_or_internal_message --locked`

Expected: FAIL。`HttpClientInitialization`が未定義である。

Run: `cargo test -p yasumaro-runtime --features test-support --test model_download progressing_response_is_not_limited_by_the_read_timeout_total --locked`

Expected: PASS。これは旧実装が持つreadごとのtimeoutを固定し、非同期化で総request deadlineへ変わることを防ぐcharacterization testである。

- [ ] **Step 3: 非同期HTTP clientとruntimeを追加する**

runtime crateへTokioの`rt`、`time` featureを直接追加し、reqwestの`blocking` featureを外す。`ModelInstaller`は`reqwest::Client`とcurrent-thread runtimeを所有する。runtimeまたはclientの構築失敗は`HttpClientInitialization`へ正規化する。

- [ ] **Step 4: sendとbody readをキャンセル可能にする**

private async関数は`tokio::select!`でHTTP futureと25ミリ秒間隔の`AtomicBool`監視を競合させる。clientには`connect_timeout`と`read_timeout`を別々に設定する。chunk受信後、file write後、`sync_all`後、size/hash検証後にも`check_cancelled`を呼ぶ。reqwest errorは既存のtimeout、connect、status、body分類を維持する。

- [ ] **Step 5: GREENを確認する**

Run: `cargo test -p yasumaro-runtime --features test-support --test model_download --locked`

Expected: PASS。stalled server testを含め、各server threadが固定上限内に終了する。

Run: `cargo test -p yasumaro-runtime --all-features --locked`

Expected: PASS。

- [ ] **Step 6: 3巡レビューを記録する**

要件レビューではfresh GETとno-resumeを確認する。状態・安全性レビューではキャンセル時のpartial cleanupとlock保持範囲を確認する。実装品質レビューではread timeoutが総request deadlineになっていないこと、server thread、error/logの情報非露出を確認する。

- [ ] **Step 7: Commit**

```bash
git add Cargo.lock crates/yasumaro-runtime/Cargo.toml crates/yasumaro-runtime/src/model/mod.rs crates/yasumaro-runtime/src/model/download.rs crates/yasumaro-runtime/tests/model_download.rs docs/superpowers/reviews/2026-09-28-model-remove-clean-install-self-review.md
git commit -m "fix: cancel stalled model downloads"
```

### Task 4: 公開許可境界とRust oracle

**Files:**
- Create: `crates/yasumaro-runtime/examples/model_lifecycle_oracle.rs`
- Modify: `crates/yasumaro-runtime/src/model/download.rs`
- Modify: `crates/yasumaro-runtime/src/model/mod.rs`
- Modify: `crates/yasumaro-runtime/src/lib.rs`
- Modify: `crates/yasumaro-runtime/tests/model_download.rs`
- Modify: `docs/superpowers/reviews/2026-09-28-model-remove-clean-install-self-review.md`

**Interfaces:**
- Consumes: Task 2のfixture、Task 3の非同期installer。
- Produces: `PublishCheckpoint::{BeforeAuthorization, AfterAuthorization}`、test-support用observer constructor、`model_lifecycle_oracle --strict|--report|--case`。

- [ ] **Step 1: 公開許可境界の失敗testを書く**

test-supportで次のinterfaceを使用するtestを先に書く。

```rust
pub enum PublishCheckpoint {
    BeforeAuthorization,
    AfterAuthorization,
}

pub fn new_for_test_with_publish_observer(
    manifest: ModelManifest,
    store: ModelStore,
    connect_timeout: Duration,
    read_timeout: Duration,
    observer: Arc<dyn Fn(PublishCheckpoint) + Send + Sync>,
) -> Result<ModelInstaller, ModelError>;
```

`cancellation_before_publish_authorization_cleans_partial`はBeforeAuthorizationで停止し、flagを設定してから再開して`Cancelled`とfinal不在を確認する。`cancellation_after_publish_authorization_keeps_success`はAfterAuthorizationで停止し、flagを設定してから再開して成功と検証済みfinalを確認する。

- [ ] **Step 2: REDを確認する**

Run: `cargo test -p yasumaro-runtime --features test-support --test model_download publish_authorization --locked`

Expected: FAIL。checkpoint APIが未定義である。

- [ ] **Step 3: 公開許可をproduction関数へ実装する**

同じprivate関数内でBeforeAuthorization observer、最後の`check_cancelled`、AfterAuthorization observer、`publish_verified`の順に呼ぶ。AfterAuthorization以後はキャンセルを再確認しない。observer fieldとconstructorは`test-support`だけで公開し、通常constructorでは同じ呼び出し位置をno-opにする。

- [ ] **Step 4: 公開境界testをGREENにする**

Run: `cargo test -p yasumaro-runtime --features test-support --test model_download publish_authorization --locked`

Expected: PASS。

- [ ] **Step 5: Rust oracleをREDで作る**

exampleはTask 2のfixtureを読み、schemaとmodeを検査する。最初はscenario adapterを未実装として`infrastructure error`を返す。

Run: `cargo run -p yasumaro-runtime --example model_lifecycle_oracle --features test-support -- --strict`

Expected: FAIL。最初のstrict caseでproduction adapterが未実装であることを示す。

- [ ] **Step 6: fixtureのscenario adapterを実装する**

adapterは`ModelInstaller::install_with_cancellation`、`ModelStore::{acquire,remove}`、公開checkpoint observerを使う。Rust側で期待値を定義せず、fixtureの`expected`と`expectedResult`へ実観測を比較する。`--strict`は`strict`と`internal-fixture`を実行し、`model-only`は実行しない。`--case <name>`は1件を再現し、`--report`は全caseのmode、events、`match`、`mismatch`、`infrastructure error`をMarkdownで標準出力へ出す。

- [ ] **Step 7: oracleをGREENにする**

Run: `cargo test -p yasumaro-runtime --example model_lifecycle_oracle --features test-support --locked`

Expected: PASS。schema、mode、field差分、unknown caseの単体testを含む。

Run: `cargo run -p yasumaro-runtime --example model_lifecycle_oracle --features test-support -- --strict`

Expected: PASS。6件のproduction対応がmatchとなる。

Run: `cargo run -p yasumaro-runtime --example model_lifecycle_oracle --features test-support -- --case cancel-before-authorization`

Expected: PASS。期待状態と実状態を表示する。

- [ ] **Step 8: 3巡レビューを記録する**

要件レビューではadapterが公開APIを通り、resume用fixtureを持たないことを確認する。状態・安全性レビューでは6 scenarioのLean eventsとRust操作を一対一に照合する。実装品質レビューでは期待値の重複、case間の状態共有、timeoutなしのchannel待機、pathとURLの出力がないことを確認する。

- [ ] **Step 9: Commit**

```bash
git add crates/yasumaro-runtime/examples/model_lifecycle_oracle.rs crates/yasumaro-runtime/src/model/download.rs crates/yasumaro-runtime/src/model/mod.rs crates/yasumaro-runtime/src/lib.rs crates/yasumaro-runtime/tests/model_download.rs docs/superpowers/reviews/2026-09-28-model-remove-clean-install-self-review.md
git commit -m "test: compare model lifecycle with Lean"
```

### Task 5: CI、文書、全体検証

**Files:**
- Modify: `.github/workflows/formal.yml`
- Modify: `formal/README.md`
- Modify: `docs/testing.md`
- Modify: `docs/development.md`
- Modify: `README.md`
- Modify: `docs/superpowers/reviews/2026-09-28-model-remove-clean-install-self-review.md`

**Interfaces:**
- Consumes: Task 2のgenerator、Task 4のoracle。
- Produces: fixture freshnessとstrict correspondenceを必須にしたCI、利用者向けキャンセル契約、最終監査記録。

- [ ] **Step 1: 文書・CI契約のREDを確認する**

Run: `rg -n "model-lifecycle-testgen|model_lifecycle_oracle" .github/workflows/formal.yml formal/README.md docs/testing.md docs/development.md`

Expected: FAIL。新しいgeneratorまたはoracleの記載がない。

- [ ] **Step 2: formal workflowへfreshnessとstrict oracleを追加する**

Rust 1.98.1を設定し、Lean buildと`YasumaroTests`の後に次を逐次実行する。

```text
lake exe model-lifecycle-testgen -- --check ../crates/yasumaro-runtime/tests/fixtures/lean-model-lifecycle.json
cargo test -p yasumaro-runtime --example model_lifecycle_oracle --features test-support --locked
cargo run -p yasumaro-runtime --example model_lifecycle_oracle --features test-support -- --strict
```

- [ ] **Step 3: 文書を更新する**

READMEはネットワーク待機中のCtrl+C、通常失敗時cleanup、公開許可後の競合規則を利用者向けに説明する。formal README、testing、developmentはgenerator、fixture freshness、strict oracle、単一case再現の正確なcommandとLean/Rustの保証境界を記載する。

- [ ] **Step 4: 文書・CI契約をGREENにする**

Run: `rg -n "model-lifecycle-testgen|model_lifecycle_oracle" .github/workflows/formal.yml formal/README.md docs/testing.md docs/development.md`

Expected: PASS。各fileに実行commandがある。

- [ ] **Step 5: 全検証を実行する**

Run: `cargo fmt --all -- --check`

Expected: PASS。

Run: `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`

Expected: PASS。

Run: `cargo test --workspace --all-features --locked`

Expected: PASS。実model依存test以外をignoreしない。

Run: `lake -d formal build`

Expected: PASS。

Run: `lake -d formal exe YasumaroTests`

Expected: PASS。

Run: `lake -d formal exe model-lifecycle-testgen -- --check ../crates/yasumaro-runtime/tests/fixtures/lean-model-lifecycle.json`

Expected: PASS。

Run: `cargo run -p yasumaro-runtime --example model_lifecycle_oracle --features test-support -- --strict`

Expected: PASS。

Run: `git diff --check`

Expected: PASS。

- [ ] **Step 6: 最終3巡レビューを記録する**

要件レビューは設計書の完了条件をcommitへ対応づける。状態・安全性レビューはLean前提、fixture、Rust観測の対応表を更新する。実装品質レビューは全差分、ignored test、OS未実行範囲、禁止した再開関連実装、Issue本文との要件差を記録する。

- [ ] **Step 7: Commit**

```bash
git add .github/workflows/formal.yml README.md formal/README.md docs/testing.md docs/development.md docs/superpowers/reviews/2026-09-28-model-remove-clean-install-self-review.md
git commit -m "docs: verify cancellable model lifecycle"
```
