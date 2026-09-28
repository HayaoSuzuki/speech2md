# モデル削除と失敗時クリーンアップ Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** モデルをコマンドで安全に削除でき、通常のinstall失敗では部分ファイルを残さないモデルライフサイクルを実装する。

**Architecture:** モデルごとのfile lockをinstallとremoveの排他操作、transcribeの共有leaseに使う。installはoffset 0から一度だけ取得し、全体検証後にだけ`.part`を確定名へrenameする。Leanは抽象状態を証明し、OSのlock、unlink、renameはRustテストで検査する。

**Tech Stack:** Rust 1.98.1、reqwest 0.12、fs4 0.13、Clap、Lean 4.34.1、GitHub Actions

**Spec:** `docs/superpowers/specs/2026-09-28-model-remove-clean-install-design.md`

## Global Constraints

- HTTP Range、ETag、Last-Modified、sidecar、validator、publish backup、取得再開を実装しない。
- downloadが必要な場合はoffset 0から開始し、同じinstall内で再接続しない。
- 通常の失敗では`.part`を削除する。削除にも失敗した場合は両方の原因を保持する。
- SIGKILLまたは電源断で残った`.part`は次回installまたはremoveが削除する。
- 全体のサイズとSHA-256を検証してから確定名へ配置する。
- removeは使用中のモデルを待たずに`ModelInUse`で拒否する。
- エラーとログへモデルURLと完全なローカルパスを出さない。
- lock fileは操作後も残して再利用する。
- GitHub Issue本文の変更やコメント投稿を行わない。
- 各Taskで要件、状態・安全性、実装品質の3巡を行い、既存のself-review文書へ記録する。

## Review Focus

- verified finalとstale partialが併存してpartial削除に失敗してもfinalを保持する。Task 3で検査する。
- removeのpartial削除に失敗した場合はfinalを削除しない。Task 2で検査する。
- lock競合だけを`ModelInUse`とし、lock setupのI/O失敗は`Lock`にする。Task 2で検査する。
- body read中のCtrl+Cとread timeoutはどちらもpartialを削除する。Task 3で検査する。
- symlinkや非空directoryを追跡・再帰削除しない。Task 2で検査する。

---

### Task 1: Leanモデルと安全性証明

**Files:**
- Create: `formal/Yasumaro/ModelLifecycle.lean`
- Create: `formal/Yasumaro/ModelLifecycleProofs.lean`
- Modify: `formal/YasumaroTests.lean`
- Modify: `formal/README.md`
- Modify: `docs/superpowers/reviews/2026-09-28-model-remove-clean-install-self-review.md`

**Interfaces:**
- Consumes: 設計書の5状態成分とcleanup・removeの契約。
- Produces: `State`、`Event`、`Safe`、`step`、`run`、一般定理。後続Taskは定理名を実装対応表で参照する。

- [ ] **Step 1: REDとなるLean契約を追加する**

  `YasumaroTests.lean`から未作成の`Yasumaro.ModelLifecycleProofs`をimportし、`publishedInitial`、`stalePartialInitial`、`busyRemoveInitial`と後述の定理を参照するassertionを追加する。

- [ ] **Step 2: REDを確認する**

  Run: `lake -d formal build`

  Expected: `unknown module 'Yasumaro.ModelLifecycleProofs'`でFAIL。

- [ ] **Step 3: 状態と遷移を実装する**

  ```lean
  structure State where
    published : Bool
    publishedVerified : Bool
    partial : Bool
    readers : Nat
    writer : Bool

  inductive Event
    | beginInstall | createPartial | publishVerified
    | abort (cleanupSucceeded : Bool)
    | acquireLease | releaseLease | remove

  def Safe (state : State) : Prop
  def step (state : State) : Event → State
  def run (state : State) (events : List Event) : State
  ```

  `Safe`は`published → publishedVerified`と`writer → readers = 0`を保持する。`publishVerified`はRust側の全体検証とrenameをまとめた抽象イベントであり、SHA-256計算自体を証明対象にしない。abortはpublicationを変えず、cleanup成功時だけpartialを消す。removeはwriterがなくreadersが0の場合だけpublicationとpartialを消す。

- [ ] **Step 4: 一般定理を証明する**

  `step_preserves`、`run_preserves`、`cleanup_success_clears_partial`、`cleanup_failure_preserves_publication`、`busy_remove_noop`、`successful_remove_clears_artifacts`、`remove_idempotent`を任意の`State`について証明する。`sorry`、`admit`、`native_decide`、`maxHeartbeats 0`を使わない。

- [ ] **Step 5: GREENを確認する**

  Run: `lake -d formal build && lake -d formal exe YasumaroTests`

  Expected: PASS。

- [ ] **Step 6: 要件レビューを記録する**

  設計書の各定理を対応づけ、Range、backup、bounded search、fixture生成がないことを確認する。

- [ ] **Step 7: 状態・安全性レビューを記録する**

  verified finalとstale partialの併存、cleanup成功・失敗、busy remove、writer中のlease取得、remove冪等性を確認する。

- [ ] **Step 8: 実装品質レビューを記録する**

  定理が具体例だけでなく任意状態を扱い、READMEがLeanの証明範囲をOS操作まで広げていないことを確認する。

- [ ] **Step 9: 再検証してコミットする**

  Run: `lake -d formal build && lake -d formal exe YasumaroTests && git diff --check`

  Commit: `formal: prove model lifecycle safety`

### Task 2: ModelStoreのlock、lease、remove

**Files:**
- Create: `crates/yasumaro-runtime/tests/model_lifecycle.rs`
- Modify: `crates/yasumaro-runtime/src/model/store.rs`
- Modify: `crates/yasumaro-runtime/src/model/mod.rs`
- Modify: `crates/yasumaro-runtime/src/lib.rs`
- Modify: `docs/superpowers/reviews/2026-09-28-model-remove-clean-install-self-review.md`

**Interfaces:**
- Consumes: `fs4::fs_std::FileExt`。`try_lock_exclusive`の`Ok(false)`だけを競合とする。
- Produces: `ModelStore::acquire(ModelId) -> Result<ModelLease, ModelError>`、`remove(ModelId)`、内部`lock_exclusive`、`paths`、`remove_entry`、`ModelError::{Lock, ModelInUse}`。

- [ ] **Step 1: storeの失敗テストを書く**

  次を追加する。

  - `acquire_holds_a_shared_lease_for_a_regular_model`
  - `remove_is_idempotent_and_deletes_partial_before_final`
  - `remove_returns_model_in_use_without_waiting`
  - `remove_preserves_final_when_partial_cleanup_fails`
  - `remove_distinguishes_busy_from_lock_setup_failure`
  - `remove_rejects_nonempty_partial_directory`
  - Unix: `remove_unlinks_symlink_without_following_target`

- [ ] **Step 2: REDを確認する**

  Run: `cargo test -p yasumaro-runtime --test model_lifecycle --locked`

  Expected: `acquire`と`remove`がなくFAIL。

- [ ] **Step 3: store APIを実装する**

  ```rust
  pub fn acquire(&self, id: ModelId) -> Result<ModelLease, ModelError>;
  pub fn remove(&self, id: ModelId) -> Result<(), ModelError>;
  pub(super) fn lock_exclusive(&self, id: ModelId) -> Result<File, ModelError>;
  pub(super) fn paths(&self, id: ModelId) -> ModelPaths;
  pub(super) fn remove_entry(path: &Path) -> Result<(), ModelError>;
  pub struct ModelLease { /* path, shared-lock file */ }
  impl ModelLease { pub fn path(&self) -> &Path; }
  pub(super) struct ModelPaths { pub final_path: PathBuf, pub partial_path: PathBuf }
  ```

  suffixは`OsString`へ追加する。`require`と`acquire`はregular fileだけを受理する。removeは非待機lockの後、partial、finalの順に削除する。symlinkはリンク自体、directoryは空の場合だけ非再帰で削除する。

- [ ] **Step 4: errorとexportを追加する**

  `ModelError::Lock { id, message }`と`ModelError::ModelInUse { id }`を追加し、`ModelLease`をruntime crateからexportする。

- [ ] **Step 5: GREENを確認する**

  Run: `cargo test -p yasumaro-runtime --test model_lifecycle --locked`

- [ ] **Step 6: 要件レビューを記録する**

  final、partial、lock以外の永続pathがなく、busy時に待機しないことを確認する。

- [ ] **Step 7: 状態・安全性レビューを記録する**

  partial削除失敗時のfinal保持、busy無変更、lease後のfile再検査、symlinkとnonempty directoryを確認する。

- [ ] **Step 8: 実装品質レビューを記録する**

  非UTF-8 path、Windows directory symlink、lock再利用、errorの機密情報、公開docを確認する。

- [ ] **Step 9: 再検証してコミットする**

  Run: `cargo fmt --all -- --check && cargo clippy -p yasumaro-runtime --all-targets --all-features --locked -- -D warnings && cargo test -p yasumaro-runtime --all-features --locked && git diff --check`

  Commit: `feat: add locked model removal`

### Task 3: Fresh downloadと失敗時cleanup

**Files:**
- Create: `crates/yasumaro-runtime/tests/model_download.rs`
- Modify: `crates/yasumaro-runtime/src/model/download.rs`
- Modify: `crates/yasumaro-runtime/src/model/mod.rs`
- Modify: `crates/yasumaro-runtime/tests/model_store.rs`
- Modify: `docs/superpowers/reviews/2026-09-28-model-remove-clean-install-self-review.md`

**Interfaces:**
- Consumes: Task 2の`lock_exclusive`、`paths`、`remove_entry`。
- Produces: `install_with_cancellation(&[ModelId], &AtomicBool)`、test-supportの`new_for_test`、`ModelError::{Cancelled, CleanupFailed}`。

- [ ] **Step 1: network testを分離し、失敗テストを書く**

  既存network testを`model_download.rs`へ移し、次を追加する。

  - `fresh_download_sends_no_range_and_creates_no_sidecar`
  - `valid_final_skips_network_and_removes_stale_partial`
  - `valid_final_survives_stale_partial_cleanup_failure`
  - `invalid_final_is_removed_before_failed_download`
  - `interrupted_response_cleans_partial`
  - `read_timeout_cleans_partial`
  - `midstream_cancellation_cleans_partial`
  - `oversized_response_cleans_partial`
  - `hash_mismatch_cleans_partial`
  - `publish_failure_cleans_partial`
  - `publish_and_cleanup_failure_preserves_both_errors`
  - `concurrent_installs_issue_one_request_and_reuse_verified_final`
  - `errors_and_logs_do_not_expose_url_or_full_path`

- [ ] **Step 2: REDを確認する**

  Run: `cargo test -p yasumaro-runtime --features test-support --test model_download --locked`

- [ ] **Step 3: timeoutとcancellation APIを実装する**

  productionはconnect 30秒、read 60秒とし、test constructorと共通の`with_timeouts`でreqwestの`connect_timeout`と`read_timeout`を設定する。

  ```rust
  pub fn install_with_cancellation(
      &self, ids: &[ModelId], cancelled: &AtomicBool,
  ) -> Result<(), ModelError>;
  #[cfg(feature = "test-support")]
  pub fn new_for_test(
      manifest: ModelManifest, store: ModelStore,
      connect_timeout: Duration, read_timeout: Duration,
  ) -> Result<Self, ModelError>;
  ```

  body readの前後とread error時にflagを確認し、cancelledなら`Cancelled`を返す。既存の`install`は常にfalseの内部flagを使って同じ処理へ委譲する。

- [ ] **Step 4: fresh-only installを実装する**

  排他lock下で、stale partial削除、valid final検証、不正final削除、無条件GET、`create_new`によるpartial作成、64 KiB単位の保存、size/hash検証、renameの順に処理する。retry loopとresume関連header・fileを作らない。

- [ ] **Step 5: cleanup errorを構造化する**

  ```rust
  ModelError::Cancelled { id: ModelId }
  ModelError::CleanupFailed {
      id: ModelId,
      source: Box<ModelError>,
      cleanup: String,
  }
  ```

  download、検証、publishの失敗を共通cleanup helperへ渡す。reqwest errorはtimeout、connect、status、bodyの分類へ、filesystem errorは操作種別と`ErrorKind`へ正規化し、URLと完全なpathを転記しない。private publish helperを使い、rename失敗とcleanup失敗を単体テストする。

- [ ] **Step 6: GREENを確認する**

  Run: `cargo test -p yasumaro-runtime --features test-support --test model_download --locked`

- [ ] **Step 7: 要件レビューを記録する**

  requestが一度、offsetが0、resume関連機構がなく、全通常失敗がcleanup helperを通ることを確認する。

- [ ] **Step 8: 状態・安全性レビューを記録する**

  stale cleanup、valid/invalid final、partial作成前、body途中、sync後、検証後、rename失敗の各境界を確認する。

- [ ] **Step 9: 実装品質レビューを記録する**

  過大応答、read timeout、memory ordering、元error保持、機密情報、local serverの終了上限を確認する。

- [ ] **Step 10: 再検証してコミットする**

  Run: `cargo fmt --all -- --check && cargo clippy -p yasumaro-runtime --all-targets --all-features --locked -- -D warnings && cargo test -p yasumaro-runtime --all-features --locked && git diff --check`

  Commit: `feat: clean failed model downloads`

### Task 4: Transcribe中のモデルlease

**Files:**
- Modify: `crates/yasumaro-runtime/src/engine/whisper.rs`
- Modify: `crates/yasumaro-runtime/src/engine/sherpa.rs`
- Modify: `crates/yasumaro-runtime/src/pipeline.rs`
- Modify: `crates/yasumaro-cli/src/commands.rs`
- Modify: `crates/yasumaro-runtime/tests/whisper_process.rs`
- Modify: `crates/yasumaro-runtime/tests/whisper_model.rs`
- Modify: `crates/yasumaro-runtime/tests/diarization_model.rs`
- Modify: `crates/yasumaro-runtime/tests/srv_db.rs`
- Modify: `crates/yasumaro-runtime/tests/voicepeak_eval.rs`
- Modify: `docs/superpowers/reviews/2026-09-28-model-remove-clean-install-self-review.md`

**Interfaces:**
- Consumes: Task 2の`ModelLease`と`ModelStore::acquire`。
- Produces: model pathではなくleaseを所有するWhisperとSherpaのconstructor。

- [ ] **Step 1: Whisper leaseの失敗テストを書く**

  `whisper_transcriber_holds_the_model_lease_until_drop`を追加し、transcriber保持中のremoveが`ModelInUse`、drop後が成功になることをassertする。

- [ ] **Step 2: REDを確認する**

  Run: `cargo test -p yasumaro-runtime --features test-support --test whisper_process whisper_transcriber_holds_the_model_lease_until_drop --locked`

- [ ] **Step 3: constructorをlease所有へ変更する**

  ```rust
  pub fn WhisperProcessTranscriber::new(
      engine: EngineLease,
      model: ModelLease,
      temp_root: PathBuf,
  ) -> Result<Self, EngineError>;
  pub fn WhisperProcessTranscriber::new_for_test(
      engine: EngineLease,
      model: ModelLease,
      temp_root: PathBuf,
      available_temp_bytes: u64,
      cancellation_grace: Duration,
  ) -> Result<Self, EngineError>;
  pub fn SherpaDiarizer::new(
      segmentation: ModelLease,
      embedding: ModelLease,
      num_threads: usize,
  ) -> Result<Self, EngineError>;
  ```

  commandとnative configには`lease.path()`を渡す。Sherpaのnative fieldをlease fieldsより前に宣言する。production、test-support、ignored evaluationの全call siteを`acquire`へ変更する。

- [ ] **Step 4: GREENと全call siteを確認する**

  Run: `cargo test -p yasumaro-runtime --features test-support --test whisper_process whisper_transcriber_holds_the_model_lease_until_drop --locked`

  Run: `cargo check --workspace --all-targets --all-features --locked`

- [ ] **Step 5: 要件レビューを記録する**

  transcribeだけがleaseを持ち、listとdoctorはlocal snapshotのままで、暗黙downloadがないことを確認する。

- [ ] **Step 6: 状態・安全性レビューを記録する**

  child process終了までのWhisper lease、native drop後までのSherpa lease、constructor失敗時の解放を確認する。

- [ ] **Step 7: 実装品質レビューを記録する**

  `rg`で全constructor callを列挙し、bare model path、unused field、doc不整合がないことを確認する。

- [ ] **Step 8: 再検証してコミットする**

  Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features --locked -- -D warnings && cargo test --workspace --all-features --locked && git diff --check`

  Commit: `feat: lease models during transcription`

### Task 5: CLI remove、install cancellation、error表示

**Files:**
- Modify: `crates/yasumaro-runtime/src/model/manifest.rs`
- Modify: `crates/yasumaro-cli/src/args.rs`
- Modify: `crates/yasumaro-cli/src/commands.rs`
- Modify: `crates/yasumaro-cli/tests/model_cli.rs`
- Modify: `docs/superpowers/reviews/2026-09-28-model-remove-clean-install-self-review.md`

**Interfaces:**
- Consumes: Task 2のremove、Task 3のcancellation API、Task 4のlease constructor。
- Produces: `ModelCommand::Remove`、共通`cancellation_flag`、model errorのexit codeとhelp。

- [ ] **Step 1: CLIの失敗テストを書く**

  - `remove_requires_at_least_one_model`
  - `remove_deletes_final_and_partial_files`
  - `remove_deduplicates_models`
  - `remove_stops_after_first_failure_without_rollback`
  - `model_in_use_reports_retry_help`
  - `model_cancellation_uses_exit_code_130`
  - `cleanup_failure_reports_model_remove_help`

- [ ] **Step 2: REDを確認する**

  Run: `cargo test -p yasumaro-cli --test model_cli --locked`

- [ ] **Step 3: removeとcancellationを実装する**

  `ModelId`へ`Ord`と`PartialOrd`をderiveする。Removeは一つ以上のmodelを必須とし、sort・dedup後に順次removeする。最初の失敗で停止し、rollbackしない。transcribeのCtrl+C flag生成を共通関数へ移し、model installにも渡す。

- [ ] **Step 4: exit codeとhelpを実装する**

  `ModelError::Cancelled`だけexit 130、その他のmodel errorは4とする。helpを`Option<String>`へ変更し、`ModelInUse`、`CleanupFailed`、`Cancelled`へ対象modelを含む再実行手順を返す。

- [ ] **Step 5: GREENを確認する**

  Run: `cargo test -p yasumaro-cli --test model_cli --locked && cargo test -p yasumaro-cli --bin yasumaro --locked`

- [ ] **Step 6: 要件レビューを記録する**

  引数必須、sort・dedup、途中停止、busy非待機、install cancellationを照合する。

- [ ] **Step 7: 状態・安全性レビューを記録する**

  前半成功・中間失敗・後続未処理、重複ID、installとtranscribeのCtrl+C分類を確認する。

- [ ] **Step 8: 実装品質レビューを記録する**

  Clap help、stdoutの一意model数、stderrの機密情報、全error variant、既存exit codeを確認する。

- [ ] **Step 9: 再検証してコミットする**

  Run: `cargo fmt --all -- --check && cargo clippy -p yasumaro-cli --all-targets --locked -- -D warnings && cargo test -p yasumaro-cli --locked && git diff --check`

  Commit: `feat: add model remove command`

### Task 6: 文書、formal CI、全体検証

**Files:**
- Create: `.github/workflows/formal.yml`
- Modify: `README.md`
- Modify: `docs/development.md`
- Modify: `docs/testing.md`
- Modify: `crates/yasumaro-cli/tests/cli.rs`
- Modify: `crates/yasumaro-cli/tests/model_cli.rs`
- Modify: `docs/superpowers/reviews/2026-09-28-model-remove-clean-install-self-review.md`

**Interfaces:**
- Consumes: Tasks 1-5のCLI、error、Lean commands。
- Produces: 利用者向け契約、開発手順、PRで動くformal workflow。

- [ ] **Step 1: 文書契約テストを書く**

  README parser testへ`yasumaro model remove whisper-small`を追加する。`readme_explains_model_removal_and_cleanup`はremove、`.part`、失敗時削除、強制終了後の次回cleanup、使用中エラーをassertする。

- [ ] **Step 2: REDを確認する**

  Run: `cargo test -p yasumaro-cli --test cli readme_command_examples_are_accepted_by_the_argument_parser --locked`

  Run: `cargo test -p yasumaro-cli --test model_cli readme_explains_model_removal_and_cleanup --locked`

- [ ] **Step 3: 文書とworkflowを実装する**

  READMEへremove例、引数必須、busy即時失敗、通常失敗のpartial削除、強制終了後の次回cleanup、再開しない契約を書く。developmentとtestingへspec、plan、Lean model、実行commandを追加する。

  `formal.yml`は`pull_request`、`merge_group`、`workflow_dispatch`で起動し、Ubuntu 22.04、20分上限を設定する。公式推奨の`leanprover/lean-action@v1`へ`lake-package-directory: formal`、`auto-config: false`、`build: true`、`test: false`、`lint: false`を渡し、その後`formal`をworking directoryとして`lake exe YasumaroTests`を実行する。

- [ ] **Step 4: GREENを確認する**

  Step 2の2 commandを再実行してPASSを確認する。

- [ ] **Step 5: 要件レビューを記録する**

  README、help、spec、planが再開なし、通常cleanup、busy即時失敗で一致し、Issue更新を含まないことを確認する。

- [ ] **Step 6: 状態・安全性レビューを記録する**

  Lean theoremとRust testの対応を記録し、OS操作や強制終了について証明以上の保証を書いていないことを確認する。

- [ ] **Step 7: 実装品質レビューを記録する**

  workflow version、runner、timeout、文書link、README parser、placeholderを確認する。

- [ ] **Step 8: 全体検証を実行する**

  ```text
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
  cargo test --workspace --all-features --locked
  lake -d formal build
  lake -d formal exe YasumaroTests
  git diff --check
  ```

  実モデル・実engine・ローカル評価fixtureを要する既存testだけはignoredとする。

- [ ] **Step 9: 禁止した機構がないことを確認する**

  `rg`で`If-Range`、`Last-Modified`、`Content-Range`、`publish-backup`、`part.json`、`resumable`を検索する。spec、plan、reviewの対象外説明以外で今回のmodel lifecycle実装に一致しないことを確認する。

- [ ] **Step 10: コミットする**

  Commit: `docs: explain model removal and cleanup`

## 最終ブランチレビュー

- [ ] 設計書の完了条件をcommit、Rust test、Lean theoremへ一項ずつ対応づける。
- [ ] `de0ec73..HEAD`を読み、resume、backup、sidecar、未使用API、機密情報を含むlog/errorがないことを確認する。
- [ ] ブランチ全体へ要件、状態・安全性、実装品質の3巡を行い、review文書へ記録する。
- [ ] Windows、Linux、macOSのCI結果を統合前に確認する。未実行platformを成功扱いにしない。
- [ ] worktreeがcleanで、設計書、計画書、実装、review記録がcommit済みであることを確認する。
