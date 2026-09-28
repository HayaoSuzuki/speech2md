# Portable Whisper Process Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Windows、macOS、Linuxで、検証済みの `whisper-cli` を明示的に導入し、日本語音声を外部プロセスで文字起こしできるようにする。

**Architecture:** `yasumaro-runtime` にエンジン成果物の取得と検証、WAV生成、プロセス制御、JSON変換を分離して置く。`Transcriber` はOSや `whisper.cpp` の型を公開せず、CLIはエンジン管理だけをランタイムへ委譲する。

**Tech Stack:** Rust 2024、Rust 1.85、serde、serde_json、reqwest blocking、sha2、zip、flate2、tar、fs4、hound、send_ctrlc 0.6.0、wait-timeout、proptest、cargo-fuzz。

**Spec:** `docs/superpowers/specs/2026-09-25-yasumaro-design.md`

## 2026-09-28 配布対象の改訂

公式ReleaseとGitHub Actionsは、Windows x86-64、Linux x86-64、macOS Apple Siliconの3対象に限定する。`macos-x86_64`のプラットフォーム判定とビルドスクリプトは、Intel Mac上で利用者がローカルビルドするために残す。Intel Mac用の成果物は、公式マニフェストへの登録、CIでの検証、GitHub Releaseへの掲載を行わない。

## Global Constraints

- 公式成果物は `windows-x86_64`、`macos-aarch64`、`linux-x86_64` とする。`macos-x86_64` はローカルビルドだけを維持する。
- Windows x86-64を主なローカル検証環境とし、公式3対象は各OSのGitHub-hosted runnerで検証する。
- `transcribe` はエンジンもモデルも暗黙に取得せず、文字起こし中はネットワークへ接続しない。
- エンジン成果物は本プロジェクトのGitHub ReleasesからHTTPSで取得し、サイズとSHA-256を検証する。
- アーカイブ内の絶対パス、親ディレクトリ参照、シンボリックリンクを拒否する。
- 初期プロンプト本文を子プロセスの引数、環境変数、標準出力、標準エラー出力、通常ログへ含めない。
- キャンセルでは終了要求後5秒待ち、未終了なら強制終了して必ずwaitする。
- 標準出力と標準エラー出力はそれぞれ末尾64 KiBだけを保持する。
- 通常の `cargo test --workspace` はモデル、エンジン、ネットワークを要求しない。
- テストは順序に依存させず、純粋変換では100%カバレッジを目標とする。
- リポジトリへモデル、上流バイナリ、SRV-DBの音声または原稿をコミットしない。

## Review Focus

- 破損または途中までのアーカイブ：確定ディレクトリを残さず、既存の正常な版を置換しない。Task 2で固定する。
- 悪意あるアーカイブパス：一時展開先の外へ一切書き出さない。Task 2で固定する。
- 機密プロンプト：子プロセスの引数、環境変数、診断末尾へ本文を漏らさない。Task 4で固定する。
- キャンセルとハング：終了要求、5秒の猶予、強制終了、wait、一時削除をすべて実行する。Task 4で固定する。
- 3時間音声とディスク不足：追加必要容量を事前計算し、WAV作成前に拒否する。Task 3で固定する。

---

### Task 1: エンジンマニフェストとプラットフォーム解決

**Files:**
- Create: `engines/manifest.json`
- Create: `crates/yasumaro-runtime/src/engine_artifact/mod.rs`
- Create: `crates/yasumaro-runtime/src/engine_artifact/manifest.rs`
- Create: `crates/yasumaro-runtime/src/engine_artifact/platform.rs`
- Modify: `crates/yasumaro-runtime/src/lib.rs`
- Test: `crates/yasumaro-runtime/src/engine_artifact/manifest.rs`
- Test: `crates/yasumaro-runtime/src/engine_artifact/platform.rs`

**Interfaces:**
- Consumes: 埋め込みJSON、またはテスト用の `Vec<EngineSpec>`、`std::env::consts::{OS, ARCH}` 相当の文字列。
- Produces: `EngineManifest::embedded() -> Result<Self, EngineArtifactError>`、`EngineManifest::select(Platform) -> Result<&EngineSpec, EngineArtifactError>`、`Platform::from_target(os: &str, arch: &str) -> Result<Self, EngineArtifactError>`。

- [ ] **Step 1: マニフェスト検証とプラットフォーム選択の失敗テストを書く**

`selects_each_supported_platform`、`rejects_duplicate_platforms`、`rejects_non_https_url`、`rejects_invalid_sha256_or_zero_size`、`rejects_unknown_platform`を追加する。4対象の `archive_name` と `executable_path` が完全一致することを表形式のテストで固定する。

- [ ] **Step 2: 失敗を確認する**

Run: `cargo test -p yasumaro-runtime engine_artifact`
Expected: FAIL with unresolved `EngineManifest` and `Platform`.

- [ ] **Step 3: 値型と検証を実装する**

`Platform` は4対象だけを持つenum、`EngineSpec` は `version`、`platform`、`url`、`size`、`sha256`、`archive_name`、`executable_path` を持つ。
`EngineManifest::new(Vec<EngineSpec>)` は重複、空値、URL、サイズ、hash、相対実行パスを検証する。

- [ ] **Step 4: 埋め込みマニフェストを追加する**

実URL、サイズ、hashが未確定の段階では架空値を入れない。
Task 6で成果物を作るまで `engines/manifest.json` は `artifacts: []` とし、`embedded()` は空マニフェストを許可する一方、`select` は `UnsupportedPlatform` または `MissingArtifact` を返す。

- [ ] **Step 5: テストを通す**

Run: `cargo test -p yasumaro-runtime engine_artifact`
Expected: PASS.

- [ ] **Step 6: コミットする**

```bash
git add engines crates/yasumaro-runtime
git commit -m "feat: resolve portable whisper engine artifacts"
```

### Task 2: エンジンの安全な取得、展開、保存

**Files:**
- Create: `crates/yasumaro-runtime/src/engine_artifact/store.rs`
- Create: `crates/yasumaro-runtime/src/engine_artifact/install.rs`
- Create: `crates/yasumaro-runtime/src/engine_artifact/archive.rs`
- Modify: `crates/yasumaro-runtime/src/engine_artifact/mod.rs`
- Modify: `crates/yasumaro-runtime/Cargo.toml`
- Test: `crates/yasumaro-runtime/tests/engine_store.rs`

**Interfaces:**
- Consumes: `EngineSpec`、明示した保存ルート、`EngineArchiveSource::download(&self, spec: &EngineSpec, destination: &mut dyn Write) -> Result<(), EngineArtifactError>`。
- Produces: `EngineStore::require(&self, spec: &EngineSpec) -> Result<InstalledEngine, EngineArtifactError>`、`InstalledEngine::acquire(&self) -> Result<EngineLease, EngineArtifactError>`、`EngineInstaller<S: EngineArchiveSource>::install(&self, spec: &EngineSpec) -> Result<InstalledEngine, EngineArtifactError>`、`EngineStore::prune(&self, keep: &EngineSpec) -> Result<PruneReport, EngineArtifactError>`。

- [ ] **Step 1: 保存と失敗原子性のテストを書く**

正常なZIPとtar.gzをテスト内で生成し、検証後だけ `<root>/<version>/<platform>/` が確定することを確認する。
サイズ不一致、hash不一致、途中Reader、展開失敗では `.part` と一時展開先が消え、既存の正常版が不変であることを確認する。

- [ ] **Step 2: アーカイブ攻撃のテストを書く**

絶対パス、`..`、Windowsプレフィックス、symlink、hardlink、マニフェスト外の実行パスを含むアーカイブをすべて拒否し、保存ルート外にファイルが作られないことを確認する。

- [ ] **Step 3: ロックとpruneのテストを書く**

`InstalledEngine::acquire()` が共有ロックを保持している間は `prune` がその版を残し、解放後は選択版以外を削除することを確認する。
削除対象は解決後の絶対パスがエンジンルート配下にある場合だけとする。

- [ ] **Step 4: 失敗を確認する**

Run: `cargo test -p yasumaro-runtime --test engine_store`
Expected: FAIL with unresolved store and installer APIs.

- [ ] **Step 5: 保存、検証、展開を実装する**

`EngineRootResolver::resolve()` は `YASUMARO_ENGINE_DIR` の絶対パスを優先し、未指定時はプラットフォームのデータディレクトリ配下の `engines` を返す。
本番の `HttpEngineArchiveSource` だけが `reqwest::blocking::Client` を所有し、テストではReaderを返すfake sourceを使う。
取得は既存のモデルinstallerと同じtimeout、64 KiB buffer、`sync_all`、rename規則を使う。
ZIPとtar.gzの各entryは正規化前後に検査し、通常ファイルとディレクトリ以外を拒否する。
確定先と一時展開先を正規化してエンジンルート配下であることを確認し、Unixではマニフェストが指す実行物だけへ実行権限を付ける。

- [ ] **Step 6: テストを通す**

Run: `cargo test -p yasumaro-runtime --test engine_store`
Expected: PASS without network.

- [ ] **Step 7: コミットする**

```bash
git add crates/yasumaro-runtime Cargo.lock
git commit -m "feat: install and verify whisper engines"
```

### Task 3: Whisper JSON変換とWAVステージング

**Files:**
- Rewrite: `crates/yasumaro-runtime/src/engine/whisper.rs`
- Create: `crates/yasumaro-runtime/src/engine/whisper_json.rs`
- Create: `crates/yasumaro-runtime/src/engine/whisper_wav.rs`
- Modify: `crates/yasumaro-runtime/src/engine/mod.rs`
- Modify: `crates/yasumaro-runtime/Cargo.toml`
- Test: `crates/yasumaro-runtime/src/engine/whisper_json.rs`
- Test: `crates/yasumaro-runtime/src/engine/whisper_wav.rs`
- Create: `crates/yasumaro-runtime/tests/fixtures/whisper-output.json`
- Modify: `fuzz/Cargo.toml`
- Create: `fuzz/fuzz_targets/whisper_json.rs`

**Interfaces:**
- Consumes: `&[u8]`のJSON、または16 kHz mono `&[f32]` と空き容量値。
- Produces: `parse_whisper_json(bytes: &[u8]) -> Result<Vec<TranscribedSegment>, EngineError>`、`required_whisper_temp_bytes(sample_count: usize) -> Result<u64, EngineError>`、`write_whisper_wav(samples: &[f32], path: &Path) -> Result<(), EngineError>`。

- [ ] **Step 1: JSON変換の失敗テストを書く**

固定した `whisper-cli` のJSON fixtureからミリ秒の半開区間と本文を変換するテストを書く。
負時刻、逆転、オーバーフロー、必須フィールド欠落、空でない未知フィールド、空区間列を個別に固定する。

- [ ] **Step 2: JSONプロパティテストを書く**

正しい区間列では順序と本文が保存されること、任意の `Vec<u8>` でパニックしないことを `proptest` で確認する。

- [ ] **Step 3: WAVと容量計算の失敗テストを書く**

16,000 samplesが44-byte headerを含む32,044 bytesの16-bit mono WAVになること、`NaN` と無限大を拒否すること、範囲外振幅をclampすること、`usize`からの計算overflowを拒否することを確認する。
追加必要容量は `max(wav_bytes * 2, 64 MiB)` と固定する。

- [ ] **Step 4: 失敗を確認する**

Run: `cargo test -p yasumaro-runtime engine::whisper`
Expected: FAIL with unresolved parser and WAV APIs.

- [ ] **Step 5: JSON変換とWAV生成を実装する**

JSON用serde型はprivateにし、未知フィールドを無視する。
WAV生成には `hound` を使い、`WavWriter::finalize` でヘッダーを確定してから成功を返す。
`whisper-rs` を依存とlockfileから削除し、`EngineError` に `InvalidJson`、`InvalidSegment`、`InvalidSample`、`InsufficientTempSpace` を追加する。

- [ ] **Step 6: ファズターゲットを追加して検証する**

Run: `cargo fuzz check whisper_json`
Expected: PASS without running an unbounded fuzz session.

- [ ] **Step 7: 通常テストを通す**

Run: `cargo test -p yasumaro-runtime engine::whisper`
Expected: PASS.

- [ ] **Step 8: コミットする**

```bash
git add crates/yasumaro-runtime fuzz Cargo.lock
git commit -m "feat: stage whisper input and parse JSON"
```

### Task 4: 外部プロセス文字起こしとキャンセル

**Files:**
- Create: `crates/yasumaro-runtime/src/engine/process.rs`
- Modify: `crates/yasumaro-runtime/src/engine/whisper.rs`
- Modify: `crates/yasumaro-runtime/src/engine/mod.rs`
- Modify: `crates/yasumaro-runtime/Cargo.toml`
- Test: `crates/yasumaro-runtime/tests/whisper_process.rs`
- Rewrite: `crates/yasumaro-runtime/tests/whisper_model.rs`
- Create: `crates/yasumaro-runtime/src/bin/fake_whisper.rs`

**Interfaces:**
- Consumes: `EngineLease`、Whisperモデルパス、16 kHz mono PCM、`TranscriptionRequest { prompt, threads, cancelled }`。
- Produces: `WhisperProcessTranscriber::new(engine: EngineLease, model_path: PathBuf, temp_root: PathBuf) -> Result<Self, EngineError>` と既存の `Transcriber` 実装。

`fake_whisper` binは `test-support` featureをrequired featureとして `Cargo.toml` に宣言し、テスト時だけビルドする。

- [ ] **Step 1: fake実行物によるコマンド契約テストを書く**

fake childが記録したargvと環境変数を検査し、日本語、非翻訳、JSON、thread数、モデル、WAV、出力prefix、`--prompt-file`だけが渡ることを確認する。
sentinelを含むprompt本文がargv、環境変数、保持診断へ現れず、一時prompt fileだけに存在することを確認する。

- [ ] **Step 2: 出力と診断の失敗テストを書く**

成功JSON、非zero終了、結果欠落、不正JSON、64 KiBを超えるstdout/stderrをfake childで作る。
診断は各末尾64 KiBだけを保持し、本文を利用者向けエラーまたは `tracing` fieldへ含めないことを確認する。

- [ ] **Step 3: キャンセルの失敗テストを書く**

終了要求で止まるfake childと終了要求を無視するfake childを使う。
後者が5秒後に強制終了され、wait済みで、一時ディレクトリとエンジン共有ロックが残らないことを停止時計で確認する。
テスト用のgrace durationだけはconstructorへ注入し、通常値を5秒に固定する。

- [ ] **Step 4: 容量不足と事前キャンセルの失敗テストを書く**

空き容量probeを注入し、不足時はWAVも子プロセスも作らないことを確認する。
開始前に `cancelled=true` の場合も同じとする。

- [ ] **Step 5: 失敗を確認する**

Run: `cargo test -p yasumaro-runtime --features test-support --test whisper_process`
Expected: FAIL with unresolved process transcriber.

- [ ] **Step 6: bounded captureとprocess runnerを実装する**

`std::process::Command` へ引数を個別に追加し、`send_ctrlc::InterruptibleCommand::spawn_interruptible` でshellを介さず起動する。
stdoutとstderrは別threadで読み続ける固定長tail bufferへ渡し、pipe詰まりを防ぐ。
キャンセル時は `send_ctrlc::Interruptible::terminate`、`wait_timeout(Duration::from_secs(5))`、未終了なら `Child::kill`、最後に `wait` の順とする。
`EngineError` に `Spawn`、`ProcessExit`、`MissingOutput`、`Cleanup` を追加し、キャンセルには既存の `Cancelled` を使う。
子プロセス由来の本文を各variantへ格納しない。

- [ ] **Step 7: `WhisperProcessTranscriber`を実装する**

一時ディレクトリ、WAV、prompt file、JSON prefixを一回の呼出しだけ所有する。
成功時だけJSONを解析し、すべての終了経路で子プロセスをreapして一時ディレクトリを削除する。
ログにはsample count、threads、engine version、duration、segment countだけを記録する。

- [ ] **Step 8: ignored実モデルテストを更新する**

`YASUMARO_ENGINE_DIR` と `YASUMARO_MODEL_DIR` が設定され、自作の `japanese-short.wav` がある場合だけ実行する。
区間順、非空本文、日本語設定を確認し、取得処理は呼び出さない。

- [ ] **Step 9: テストを通す**

Run: `cargo test -p yasumaro-runtime --features test-support --test whisper_process`
Expected: PASS without engine, model, or network.

Run: `cargo test -p yasumaro-runtime`
Expected: PASS.

- [ ] **Step 10: コミットする**

```bash
git add crates/yasumaro-runtime Cargo.lock
git commit -m "feat: transcribe with managed whisper process"
```

### Task 5: エンジン管理CLI

**Files:**
- Create: `crates/yasumaro-cli/src/args.rs`
- Create: `crates/yasumaro-cli/src/commands.rs`
- Modify: `crates/yasumaro-cli/src/main.rs`
- Modify: `crates/yasumaro-cli/Cargo.toml`
- Test: `crates/yasumaro-cli/tests/engine_cli.rs`

**Interfaces:**
- Consumes: Task 1と2のmanifest、store、installer、prune API。
- Produces: `yasumaro engine install`、`engine list`、`engine verify`、`engine prune` と安定した終了コード。

- [ ] **Step 1: assert_cmdによるCLIテストを書く**

未対応platform、未公開artifact、導入済み、破損、使用中prune、network失敗をfake installer/storeで検証する。
`list` と `verify` がネットワークへ接続しないこと、診断に `error:` と操作可能な `help:` が含まれることを確認する。

- [ ] **Step 2: 失敗を確認する**

Run: `cargo test -p yasumaro-cli --test engine_cli`
Expected: FAIL because engine subcommands are absent.

- [ ] **Step 3: clap引数とdispatchを実装する**

`EngineCommand::{Install,List,Verify,Prune}` を追加する。
installerは `Install` 分岐だけで生成し、ほかの分岐へHTTP clientを渡さない。
成功0、使用法2、設定3、エンジン4を返す。

- [ ] **Step 4: テストとhelpを通す**

Run: `cargo test -p yasumaro-cli --test engine_cli`
Expected: PASS.

- [ ] **Step 5: コミットする**

```bash
git add crates/yasumaro-cli Cargo.lock
git commit -m "feat: manage portable whisper engines"
```

### Task 6: 再現可能なエンジン成果物

**Files:**
- Create: `vendor/whisper.cpp-patches/0001-add-prompt-file.patch`
- Create: `scripts/build-whisper-engine.ps1`
- Create: `scripts/build-whisper-engine.sh`
- Create: `scripts/verify-whisper-engine.ps1`
- Create: `scripts/verify-whisper-engine.sh`
- Create: `engines/README.md`
- Modify: `engines/manifest.json`
- Create: `.github/workflows/heavy.yml`
- Modify: `README.md`

**Interfaces:**
- Consumes: 固定した上流commit、固定patch、対象OSのC/C++ toolchain。
- Produces: 規定名のZIPまたはtar.gz、ライセンス、build metadata、SHA-256、手動公開手順。

- [ ] **Step 1: パッチ契約テストを書く**

build scriptがclone後に固定commitをcheckoutし、patch適用失敗時に停止することをshell testで確認する。
`--prompt-file` がUTF-8を読み、`--prompt` と同時指定を拒否し、本文を出力しない上流CLIテストをpatchへ含める。

- [ ] **Step 2: ビルドスクリプトを書く**

出力先を必須引数にし、一時source/build directoryをOS temp配下へ作る。
成果物には実行物、必要共有ライブラリ、上流ライセンス、commit、patch hash、compiler、CMake設定を含む `build-metadata.json` だけを入れる。
モデルとソースツリーを成果物へ含めない。

- [ ] **Step 3: 検証スクリプトを書く**

アーカイブの許可ファイル、実行物の起動、共有ライブラリ、prompt file契約、短いfixture、オフライン実行、キャンセル後の残存プロセスを検査する。
公式3対象は各OSのGitHub-hosted runnerで実行する。macOS Intelは対象Mac上で利用者が検証する。

- [ ] **Step 4: 実成果物を作成してマニフェストを確定する**

公式3対象のURL、size、SHA-256はGitHub Releasesへアップロードした実ファイルから計測する。
`macos-x86_64` は公式マニフェストへ登録しない。
未作成対象を架空値で埋めず、その対象の `engine install` は `MissingArtifact` を返す状態を維持する。

- [ ] **Step 5: READMEへ利用手順を記載する**

`engine install|list|verify|prune`、保存場所、通信範囲、`--prompt-file`、ローカルビルド、ディスク使用量、各OSの検証手順を記載する。

- [ ] **Step 6: 通常CI相当を検証する**

Run: `cargo fmt --all -- --check`
Expected: PASS.

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
Expected: PASS.

Run: `cargo test --workspace`
Expected: PASS without network, model, or engine.

Run three times: `cargo +nightly test --workspace -- -Z unstable-options --shuffle --test-threads=1`
Expected: PASS three times; each invocation prints its shuffle seed.

- [ ] **Step 7: コミットする**

```bash
git add vendor scripts engines .github README.md
git commit -m "build: package portable whisper engines"
```

### Task 7: 親計画との統合

**Files:**
- Modify: `docs/superpowers/plans/2026-09-25-yasumaro-implementation.md`
- Modify: `.superpowers/sdd/2026-09-25-yasumaro-implementation/progress.md`

**Interfaces:**
- Consumes: Task 1〜6の公開APIと完了コミット。
- Produces: sherpa-onnx、オフラインpipeline、CLI、CI、README作業へ矛盾なく続く親計画。

- [ ] **Step 1: 親計画のTask 6を完了記録へ置き換える**

本計画へのリンク、完了コミット、`WhisperProcessTranscriber`、`EngineStore` を記録し、`whisper-rs` とabort callbackの手順を削除する。

- [ ] **Step 2: 後続Taskの依存を更新する**

pipelineは `InstalledEngine` を受け取るtranscriberを使い、CLIは既存のengine subcommandsへmodel、transcribe、doctorを追加する。
CIとREADMEは公式3対象の成果物を参照する。`macos-x86_64` はローカルビルド手順だけを記載する。

- [ ] **Step 3: 計画の残存矛盾を検査する**

Run: `rg -n "whisper-rs|abort callback|Rustプロセス内" docs/superpowers/plans/2026-09-25-yasumaro-implementation.md`
Expected: no matches.

- [ ] **Step 4: コミットする**

```bash
git add docs/superpowers/plans/2026-09-25-yasumaro-implementation.md
git commit -m "docs: align implementation plan with whisper process"
```
