# speech2md Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Windows上でWAV、MP3、M4Aを日本語文字起こしし、話者とタイムスタンプを含むCommonMarkを生成する完全ローカルCLIを構築する。

**Architecture:** Cargo workspaceをI/O非依存の`core`、純粋な文書レンダラーの`formats`、音声、モデル、推論、保存を担う`runtime`、利用者との境界を担う`cli`に分ける。ランタイムがI/Oを駆動してエンジン固有値をコア値へ変換し、コアは時刻付き文字列だけを統合する。

**Tech Stack:** Rust 2024、clap 4.5、thiserror 2、Symphonia 0.6、rubato 5、whisper-rs 0.16、sherpa-onnx 1.13.8、reqwest 0.12 blocking、serde 1、sha2 0.10、memmap2 0.9、tempfile 3、pulldown-cmark 0.13、proptest 1、assert_cmd 2。

**Spec:** `docs/superpowers/specs/2026-09-25-speech2md-design.md`

## Global Constraints

- 対象環境はWindows x86-64、CPU実行とする。
- 入力はWAV、MP3、一般的なiPhoneボイスメモのAAC-LC/M4Aを扱う。
- 文字起こし言語は日本語とし、英語翻訳を行わない。
- `transcribe`はネットワークへ接続せず、通信は明示的な`model install`だけが行う。
- 音声、文字起こし本文、SRV-DBデータを通常ログ、リポジトリ、CIキャッシュへ保存しない。
- コアはパス、ファイル、HTTP、OS API、whisper.cpp、sherpa-onnxの型へ依存しない。
- 出力はCommonMarkとし、独自拡張構文を使わない。
- 既存出力を`--force`なしで上書きしない。
- Rustの最小バージョンは1.85、`Cargo.lock`をコミットする。
- 通常の`cargo test --workspace`はモデルとネットワークを要求しない。
- `speech2md-core`と`speech2md-formats`はline coverageとfunction coverageを100%にする。
- `speech2md-cli`とネイティブI/O以外の`speech2md-runtime`も100%を目標とし、未到達行は外部依存またはOS依存である理由と代替検証を`docs/testing.md`へ記録する。
- カバレッジ対象から除外するためだけの`cfg(coverage)`、到達不能化、ファイル除外は行わない。
- 各テストは専用の`TempDir`、fake、環境値を使い、別テストが作った状態や実行順序へ依存しない。
- CIでは通常の並列テストに加え、nightly libtestの`--shuffle --test-threads=1`を3回実行し、順序依存を検出する。
- shuffle失敗時はログに出たseedを`--shuffle-seed SEED`へ渡してローカル再現する。
- GitHub Actionsの通常CIと手動CIは`ubuntu-latest`だけを使い、GitHub-hosted Windows runnerを使わない。
- 通常CIはモデル、SRV-DB、長時間音声を取得せず、モデル不要のテストだけを実行する。
- 実モデル推論は`workflow_dispatch`の手動CI、SRV-DB、1時間と3時間の性能、Windowsバイナリはローカル検証として分離する。

## Review Focus

- 拡張子と内容が一致しない入力：内容をプローブし、実コーデックを処理するか具体的な未対応エラーを返す。Task 4で契約テストを追加する。
- 無音または音声ストリームのない入力：パニックや空の成功結果にせず、分類済みエラーを返す。Task 4とTask 8でテストする。
- 話者区間が重なるか欠落する入力：最大重複を決定的に選び、閾値未満を`Unknown`にする。Task 2でテストする。
- 破損または途中まで取得したモデル：SHA-256検証に失敗し、確定ファイルを残さない。Task 5でテストする。
- 出力先が既存、読み取り専用、または処理途中で失敗する場合：元ファイルを保持し、不完全なMarkdownを残さない。Task 8とTask 9でテストする。

---

## ファイル構成

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml
.gitignore
.github/workflows/ci.yml
crates/
  speech2md-core/
    Cargo.toml
    src/{lib,time,transcript,assign,normalize}.rs
    tests/{assign,normalize,properties}.rs
  speech2md-formats/
    Cargo.toml
    src/{lib,commonmark}.rs
    tests/commonmark.rs
  speech2md-runtime/
    Cargo.toml
    src/
      lib.rs
      error.rs
      audio/{mod,decode,pcm,resample}.rs
      model/{mod,manifest,store,download}.rs
      engine/{mod,whisper,sherpa}.rs
      pipeline.rs
      output.rs
    tests/{audio,model_store,pipeline,srv_db}.rs
    tests/fixtures/
  speech2md-cli/
    Cargo.toml
    src/{main,args,commands,diagnostic}.rs
    tests/cli.rs
models/manifest.json
docs/testing.md
README.md
```

`speech2md-core`は純粋なドメイン値と変換を持つ。`speech2md-formats`は文書から文字列への変換だけを持つ。`speech2md-runtime`はI/Oとネイティブ依存を閉じ込める。`speech2md-cli`は引数、表示、終了コードだけを持つ。

### Task 1: Workspaceと時刻付きドメイン型

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `.gitignore`
- Create: `crates/speech2md-core/Cargo.toml`
- Create: `crates/speech2md-core/src/lib.rs`
- Create: `crates/speech2md-core/src/time.rs`
- Create: `crates/speech2md-core/src/transcript.rs`
- Create: `crates/speech2md-formats/Cargo.toml`
- Create: `crates/speech2md-formats/src/lib.rs`
- Create: `crates/speech2md-runtime/Cargo.toml`
- Create: `crates/speech2md-runtime/src/lib.rs`
- Create: `crates/speech2md-cli/Cargo.toml`
- Create: `crates/speech2md-cli/src/main.rs`
- Test: `crates/speech2md-core/src/time.rs`
- Test: `crates/speech2md-core/src/transcript.rs`

**Interfaces:**
- Consumes: なし。
- Produces: `Timestamp::from_millis(u64)`, `TimeSpan::new(Timestamp, Timestamp)`, `Confidence::new(f32)`, `TranscribedSegment`, `TimedToken`, `SpeakerTurn`, `Utterance`, `TranscriptDocument`。

- [ ] **Step 1: workspaceと失敗する型テストを作る**

```toml
# Cargo.toml
[workspace]
members = [
  "crates/speech2md-core",
  "crates/speech2md-formats",
  "crates/speech2md-runtime",
  "crates/speech2md-cli",
]
resolver = "3"

[workspace.package]
edition = "2024"
rust-version = "1.85"
license = "MIT"

[workspace.dependencies]
thiserror = "2"
serde = { version = "1", features = ["derive"] }
```

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.85.0"
components = ["clippy", "rustfmt", "llvm-tools-preview"]
profile = "minimal"
```

開発環境とCIで`cargo-llvm-cov 0.9`を使う。

三つの後続クレートには最小の`Cargo.toml`と空の`lib.rs`または`fn main() {}`を置き、最初の`cargo test --workspace`からworkspace全体を解決可能にする。`.gitignore`には`/target/`、`/test-data/srv-db/`、`*.part`を記載する。

```rust
#[test]
fn rejects_reversed_span() {
    assert!(TimeSpan::new(Timestamp::from_millis(20), Timestamp::from_millis(10)).is_err());
}

#[test]
fn confidence_is_a_probability() {
    assert!(Confidence::new(-0.1).is_err());
    assert!(Confidence::new(1.1).is_err());
}
```

- [ ] **Step 2: テストが未定義型で失敗することを確認する**

Run: `cargo test -p speech2md-core`
Expected: FAIL with unresolved `TimeSpan` and `Confidence`.

- [ ] **Step 3: 値型を最小実装する**

```rust
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Timestamp(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimeSpan { pub start: Timestamp, pub end: Timestamp }

impl TimeSpan {
    pub fn new(start: Timestamp, end: Timestamp) -> Result<Self, InvalidTimeSpan> {
        (start <= end).then_some(Self { start, end }).ok_or(InvalidTimeSpan)
    }
    pub fn duration_ms(self) -> u64 { self.end.as_millis() - self.start.as_millis() }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Confidence(f32);

impl Confidence {
    pub fn new(value: f32) -> Result<Self, InvalidConfidence> {
        (value.is_finite() && (0.0..=1.0).contains(&value))
            .then_some(Self(value)).ok_or(InvalidConfidence)
    }
}
```

`TranscribedSegment`へ`tokens: Vec<TimedToken>`を含め、単語またはトークン時刻が利用できない場合は空配列にする。sherpa-onnxのネイティブスコアは確率ではないため、根拠のある正規化を定義するまで`SpeakerTurn.confidence`を`None`にする。

- [ ] **Step 4: 型の単体テストと全workspaceテストを通す**

Run: `cargo test --workspace`
Expected: PASS.

Run: `cargo llvm-cov -p speech2md-core --fail-under-lines 100 --fail-under-functions 100 --show-missing-lines`
Expected: PASS with 100% line and function coverage.

- [ ] **Step 5: フォーマットとlintを確認する**

Run: `cargo fmt --all --check`
Expected: PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 6: コミットする**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml .gitignore crates
git commit -m "feat: define speech transcript domain types"
```

### Task 2: 話者割り当てと本文正規化

**Files:**
- Create: `crates/speech2md-core/src/assign.rs`
- Create: `crates/speech2md-core/src/normalize.rs`
- Modify: `crates/speech2md-core/src/lib.rs`
- Test: `crates/speech2md-core/tests/assign.rs`
- Test: `crates/speech2md-core/tests/normalize.rs`
- Test: `crates/speech2md-core/tests/properties.rs`

**Interfaces:**
- Consumes: Task 1の`TranscribedSegment`, `TimedToken`, `SpeakerTurn`, `Utterance`, `TimeSpan`。
- Produces: `assign_speakers(transcript, turns, &AssignmentConfig) -> Vec<Utterance>`、`normalize_utterances(Vec<Utterance>, &NormalizationConfig) -> Vec<Utterance>`。

- [ ] **Step 1: 最大重複、同率、欠落、重複話者区間の失敗テストを書く**

```rust
#[test]
fn assigns_the_speaker_with_the_largest_overlap() {
    let transcript = vec![segment(0, 1_000, "確認します")];
    let turns = vec![turn(0, 400, 0), turn(400, 1_000, 1)];
    let result = assign_speakers(&transcript, &turns, &AssignmentConfig::default());
    assert_eq!(result[0].speaker, Some(SpeakerId::new(1)));
}
```

同じfixture helperを使い、完全な同率では小さい`SpeakerId`を選ぶこと、1000ms中100msしか重ならず既定比率未満なら`None`になること、時刻付きtokenの前後で話者が変わる場合は二発話へ分割することも個別テストにする。

- [ ] **Step 2: 割り当てテストが関数未定義で失敗することを確認する**

Run: `cargo test -p speech2md-core --test assign`
Expected: FAIL with unresolved `assign_speakers`.

- [ ] **Step 3: 半開区間の重複と決定的な割り当てを実装する**

```rust
pub struct AssignmentConfig { pub min_overlap_ratio: f32 }

pub fn overlap_ms(left: TimeSpan, right: TimeSpan) -> u64 {
    left.end.as_millis().min(right.end.as_millis())
        .saturating_sub(left.start.as_millis().max(right.start.as_millis()))
}

pub fn assign_speakers(
    transcript: &[TranscribedSegment],
    turns: &[SpeakerTurn],
    config: &AssignmentConfig,
) -> Vec<Utterance> {
    transcript.iter().flat_map(|segment| assign_one(segment, turns, config)).collect()
}
```

`assign_one`はtoken時刻がある場合に話者境界で分割し、各区間について`overlap_ms`降順、`SpeakerId`昇順で候補を選ぶ。`overlap_ms / segment.duration_ms`が`min_overlap_ratio`未満なら`speaker=None`を返す。durationが0なら比率を0として扱う。

- [ ] **Step 4: 正規化の失敗テストを書く**

```rust
#[test]
fn merges_nearby_utterances_from_the_same_speaker() {
    let input = vec![utterance(0, 500, 0, "確認します。"), utterance(600, 900, 0, "次です。")];
    let result = normalize_utterances(input, &NormalizationConfig { max_gap_ms: 200, max_chars: 100 });
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].text, "確認します。次です。");
}
```

別テストで、推論窓境界の隣接完全一致だけを一回へ縮めること、`はい、はい`のような一発話内の反復を保持すること、話者が異なれば100msのgapでも統合しないことを固定する。

- [ ] **Step 5: 正規化テストの失敗を確認し、最小実装する**

Run: `cargo test -p speech2md-core --test normalize`
Expected: FAIL with unresolved `normalize_utterances`.

```rust
pub struct NormalizationConfig {
    pub max_gap_ms: u64,
    pub max_chars: usize,
}

pub fn normalize_utterances(
    utterances: Vec<Utterance>,
    config: &NormalizationConfig,
) -> Vec<Utterance> {
    merge_adjacent(remove_window_duplicates(normalize_spacing(utterances)), config)
}
```

- [ ] **Step 6: プロパティテストを追加する**

`proptest`で任意の妥当な区間列を生成し、結果が時刻順、全区間が妥当、非空入力の文字列が正規化規則以外で消失しないことを固定する。

Run: `cargo test -p speech2md-core`
Expected: PASS.

Run: `cargo llvm-cov -p speech2md-core --fail-under-lines 100 --fail-under-functions 100 --show-missing-lines`
Expected: PASS with no uncovered production lines or functions.

- [ ] **Step 7: コミットする**

```bash
git add crates/speech2md-core
git commit -m "feat: assign speakers and normalize utterances"
```

### Task 3: CommonMarkレンダラー

**Files:**
- Create: `crates/speech2md-formats/Cargo.toml`
- Create: `crates/speech2md-formats/src/lib.rs`
- Create: `crates/speech2md-formats/src/commonmark.rs`
- Test: `crates/speech2md-formats/tests/commonmark.rs`

**Interfaces:**
- Consumes: Task 1の`TranscriptDocument`, `Utterance`, `SpeakerId`, `Timestamp`。
- Produces: `render_commonmark(&TranscriptDocument) -> String`。

- [ ] **Step 1: 正確な期待文字列を持つ失敗テストを書く**

```rust
#[test]
fn renders_speaker_and_timestamp() {
    let document = document("meeting", utterance(1, 12_000, "API側の変更は完了しています。"));
    assert_eq!(render_commonmark(&document),
        "# meeting\n\n**Speaker 2**（00:00:12）\n\nAPI側の変更は完了しています。\n");
}

#[test]
fn renders_unknown_and_hours() {
    let output = render_commonmark(&document("long", unknown_utterance(10_862_000, "# status")));
    assert!(output.contains("**Unknown**（03:01:02）"));
    assert!(output.contains("\\# status"));
}
```

- [ ] **Step 2: テストの失敗を確認する**

Run: `cargo test -p speech2md-formats`
Expected: FAIL with unresolved `render_commonmark`.

- [ ] **Step 3: レンダラーと時刻表示を実装する**

```rust
pub fn render_commonmark(document: &TranscriptDocument) -> String {
    let mut out = format!("# {}\n", escape_inline(document.title()));
    for utterance in document.utterances() {
        let speaker = utterance.speaker
            .map(|id| format!("Speaker {}", id.as_u32() + 1))
            .unwrap_or_else(|| "Unknown".into());
        out.push_str(&format!("\n**{}**（{}）\n\n{}\n",
            speaker, format_timestamp(utterance.span.start), escape_blocks(&utterance.text)));
    }
    out
}
```

- [ ] **Step 4: CommonMarkパーサーで構造を検証する**

`pulldown-cmark`で生成文字列を解析し、見出しが一つ、各ラベルがstrong、発話内の`#`や`-`が新しいブロックにならないことをテストする。

Run: `cargo test -p speech2md-formats`
Expected: PASS.

Run: `cargo llvm-cov -p speech2md-formats --fail-under-lines 100 --fail-under-functions 100 --show-missing-lines`
Expected: PASS with 100% line and function coverage.

- [ ] **Step 5: コミットする**

```bash
git add crates/speech2md-formats
git commit -m "feat: render transcripts as CommonMark"
```

### Task 4: 音声プローブ、デコード、リサンプリング

**Files:**
- Create: `crates/speech2md-runtime/Cargo.toml`
- Create: `crates/speech2md-runtime/src/lib.rs`
- Create: `crates/speech2md-runtime/src/error.rs`
- Create: `crates/speech2md-runtime/src/audio/mod.rs`
- Create: `crates/speech2md-runtime/src/audio/decode.rs`
- Create: `crates/speech2md-runtime/src/audio/resample.rs`
- Create: `crates/speech2md-runtime/src/audio/pcm.rs`
- Test: `crates/speech2md-runtime/tests/audio.rs`
- Test data: `crates/speech2md-runtime/tests/fixtures/tone.{wav,mp3,m4a}`
- Test data: `crates/speech2md-runtime/tests/fixtures/silent.wav`
- Test data: `crates/speech2md-runtime/tests/fixtures/no-audio.mp4`

**Interfaces:**
- Consumes: `&Path` supplied by the runtime caller。
- Produces: `decode_to_pcm(path, temp_root) -> Result<DecodedPcm, RuntimeError>`、`DecodedPcm::samples() -> &[f32]`、`sample_rate() == 16_000`、`channels() == 1`。

- [ ] **Step 1: 自作fixture生成手順と契約テストを書く**

1kHz、2秒、48kHz stereoの自作波形を元にWAV、MP3、AAC-LC/M4Aを作り、生成コマンドとライセンスを`tests/fixtures/README.md`へ記録する。FFmpegはfixture作成時だけ使用し、製品実行時依存にはしない。

```rust
#[test]
fn decodes_wav_mp3_and_m4a_to_equivalent_mono_16khz() {
    let decoded: Vec<_> = ["tone.wav", "tone.mp3", "tone.m4a"]
        .map(|name| decode_fixture(name).unwrap());
    assert!(decoded.iter().all(|pcm| pcm.sample_rate() == 16_000 && pcm.channels() == 1));
    let lengths: Vec<_> = decoded.iter().map(|pcm| pcm.samples().len()).collect();
    assert!(lengths.iter().max().unwrap() - lengths.iter().min().unwrap() <= 320);
}
```

別テストで、MP3を`.wav`へ改名しても内容から処理できること、`no-audio.mp4`が`RuntimeError::NoAudioStream`になること、`silent.wav`が空音声ではなく有効な無音PCMとして成功することを検証する。

- [ ] **Step 2: 未実装による失敗を確認する**

Run: `cargo test -p speech2md-runtime --test audio`
Expected: FAIL with unresolved `decode_to_pcm`.

- [ ] **Step 3: Symphoniaによるプローブとデコードを実装する**

`symphonia = { version = "0.6", default-features = false, features = ["aac", "isomp4", "mp3", "pcm", "wav", "opt-simd"] }`を使う。既定トラックのデコード済みフレームを`f32`へ変換し、全チャンネルの算術平均でモノラル化する。音声トラックがない場合は`RuntimeError::NoAudioStream`を返す。

- [ ] **Step 4: rubatoによる16kHz変換を実装する**

`rubato::Fft`と`process_all_into_buffer`を使い、アンチエイリアスを伴う固定比率変換を行う。16kHz入力は再サンプルせず、そのまま書き出す。

- [ ] **Step 5: 単一バッキングストアのPCMを実装する**

一時ファイルへlittle-endian `f32`をヘッダーなしで書き、`memmap2::Mmap`を`DecodedPcm`が所有する。ファイル長が4の倍数であることとマップのアラインメントを検査してから`bytemuck::try_cast_slice`で借用する。`Drop`順序はマップ、ファイル、一時ディレクトリとする。

- [ ] **Step 6: 音声契約テストを通す**

Run: `cargo test -p speech2md-runtime --test audio`
Expected: PASS for all formats, wrong extension, silence, and no-audio input.

- [ ] **Step 7: コミットする**

```bash
git add crates/speech2md-runtime
git commit -m "feat: decode and normalize supported audio formats"
```

### Task 5: モデルマニフェスト、保存、取得

**Files:**
- Create: `models/manifest.json`
- Create: `crates/speech2md-runtime/src/model/mod.rs`
- Create: `crates/speech2md-runtime/src/model/manifest.rs`
- Create: `crates/speech2md-runtime/src/model/store.rs`
- Create: `crates/speech2md-runtime/src/model/download.rs`
- Modify: `crates/speech2md-runtime/src/lib.rs`
- Test: `crates/speech2md-runtime/tests/model_store.rs`

**Interfaces:**
- Consumes: 埋め込み`ModelManifest`、`ModelStore`のルート、明示的な`install(ids)`呼び出し。
- Produces: `ModelId::{WhisperBase, WhisperSmall, SpeakerSegmentation, SpeakerEmbedding}`、`ModelStore::require(ModelId) -> Result<PathBuf, ModelError>`、`ModelInstaller::install(&[ModelId])`。

- [ ] **Step 1: ローカルHTTPサーバーを使う失敗テストを書く**

```rust
#[test]
fn installs_only_after_sha256_matches() {
    let bytes = b"model bytes";
    let server = model_server(bytes);
    let (installer, store) = installer_for(&server, sha256(bytes));
    installer.install(&[ModelId::WhisperBase]).unwrap();
    assert_eq!(std::fs::read(store.require(ModelId::WhisperBase).unwrap()).unwrap(), bytes);
}
```

別テストで、誤ったhashなら確定名と`.part`がともに残らないこと、`require`が通信せず`MissingModel`と正確な導入コマンドを返すこと、中断応答が既存モデルを置換しないことを検証する。

- [ ] **Step 2: テストの失敗を確認する**

Run: `cargo test -p speech2md-runtime --test model_store`
Expected: FAIL with unresolved model APIs.

- [ ] **Step 3: マニフェストと保存先を実装する**

```rust
#[derive(Clone, Debug, Deserialize)]
pub struct ModelSpec {
    pub id: ModelId,
    pub engine_version: String,
    pub url: Url,
    pub size: u64,
    pub sha256: String,
    pub license: String,
    pub file_name: String,
}
```

Windowsでは`%LOCALAPPDATA%\speech2md\models`を既定とし、テストでは明示した一時ルートを使う。マニフェストは`include_str!`でバイナリへ埋め込む。
`SPEECH2MD_MODEL_DIR`が設定されている場合はその絶対パスを優先し、CIと隔離テストでユーザー領域を変更せずに済むようにする。

- [ ] **Step 4: ストリーミング取得、検証、原子的確定を実装する**

`reqwest::blocking::Client`で`.part`へストリーム保存し、同時にSHA-256を計算する。サイズとハッシュの一致後、同一ディレクトリ内で確定名へrenameする。`transcribe`側は`ModelStore::require`だけを使用し、`ModelInstaller`を参照しない。

- [ ] **Step 5: 上流モデルを取得してマニフェストを固定する**

Whisperは`ggml-base.bin`と`ggml-small.bin`、話者分離はsherpa-onnx 1.13.8が例示する`segmentation-3-0`と`3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx`を公式配布元から取得する。PowerShellの`Get-FileHash -Algorithm SHA256`と`(Get-Item).Length`で値を計測し、取得時のリリースURL、サイズ、ハッシュ、ライセンスを`models/manifest.json`へ記録する。マニフェスト内の全URLがHTTPSで、SHA-256が64桁小文字hexであることをテストする。

- [ ] **Step 6: モデルテストを通す**

Run: `cargo test -p speech2md-runtime --test model_store`
Expected: PASS without public network access.

- [ ] **Step 7: コミットする**

```bash
git add models crates/speech2md-runtime
git commit -m "feat: install and verify inference models"
```

### Task 6: Whisper文字起こしアダプター

**Files:**
- Create: `crates/speech2md-runtime/src/engine/mod.rs`
- Create: `crates/speech2md-runtime/src/engine/whisper.rs`
- Modify: `crates/speech2md-runtime/Cargo.toml`
- Test: `crates/speech2md-runtime/src/engine/whisper.rs`
- Test: `crates/speech2md-runtime/tests/whisper_model.rs`

**Interfaces:**
- Consumes: `&[f32]`の16kHz mono PCM、Whisperモデルパス、`TranscriptionRequest { prompt, threads, cancelled }`。
- Produces: `trait Transcriber { fn transcribe(&self, samples: &[f32], request: &TranscriptionRequest) -> Result<Vec<TranscribedSegment>, EngineError>; }`。

- [ ] **Step 1: ネイティブ型を使わない変換テストを書く**

```rust
#[test]
fn converts_whisper_centiseconds_to_milliseconds() {
    let span = native_span_to_domain(123, 456).unwrap();
    assert_eq!(span.start.as_millis(), 1_230);
    assert_eq!(span.end.as_millis(), 4_560);
}
```

別テストで、開始が終了より後のnative segmentを`EngineError::InvalidSegment`にすることと、`AtomicBool=true`ならabort callbackがfalseを返すことを検証する。文字列変換はwhisper-rsが返すUTF-8結果を使い、変換失敗を`EngineError::InvalidText`へ写す。

- [ ] **Step 2: 変換テストの失敗を確認する**

Run: `cargo test -p speech2md-runtime engine::whisper`
Expected: FAIL with unresolved adapter functions.

- [ ] **Step 3: whisper-rsアダプターを実装する**

`WhisperContextParameters`でGPUを無効にする。`FullParams`は日本語`ja`、翻訳なし、timestampsあり、`Greedy { best_of: 1 }`、論理CPU数に基づくスレッド数、任意のinitial promptを設定する。進捗とabort callbackは本文をログへ出さず、`AtomicBool`だけを読む。

- [ ] **Step 4: 小さなローカルモデル統合テストをignoredで追加する**

`SPEECH2MD_MODEL_DIR`に導入済みモデルがある場合だけ実行し、リポジトリの短い自作fixtureを使って、区間が時刻順で、日本語設定が適用され、テキストが空でないことを検証する。

Run: `cargo test -p speech2md-runtime --test whisper_model -- --ignored`
Expected: PASS when `SPEECH2MD_MODEL_DIR` points to installed local assets; otherwise the test prints the required setup and returns without downloading.

- [ ] **Step 5: 通常テストを通す**

Run: `cargo test -p speech2md-runtime`
Expected: PASS without model or network.

- [ ] **Step 6: コミットする**

```bash
git add crates/speech2md-runtime
git commit -m "feat: transcribe Japanese audio with whisper.cpp"
```

### Task 7: sherpa-onnx話者分離アダプター

**Files:**
- Create: `crates/speech2md-runtime/src/engine/sherpa.rs`
- Modify: `crates/speech2md-runtime/src/engine/mod.rs`
- Modify: `crates/speech2md-runtime/Cargo.toml`
- Test: `crates/speech2md-runtime/src/engine/sherpa.rs`
- Test: `crates/speech2md-runtime/tests/diarization_model.rs`

**Interfaces:**
- Consumes: `&[f32]`の16kHz mono PCM、segmentationとembeddingのモデルパス、`DiarizationRequest { num_speakers }`。
- Produces: `trait Diarizer { fn diarize(&self, samples: &[f32], request: &DiarizationRequest) -> Result<Vec<SpeakerTurn>, EngineError>; }`。

- [ ] **Step 1: 設定排他と変換の失敗テストを書く**

```rust
#[test]
fn rejects_zero_as_an_exact_speaker_count() {
    let request = DiarizationRequest { num_speakers: Some(0) };
    assert!(matches!(request.validate(), Err(EngineError::InvalidConfig(_))));
}
```

別テストで、秒を最も近いミリ秒へ丸めて開始順に並べること（1.2346秒は1235ms）と、負数または逆転したnative区間を`EngineError::InvalidSegment`にすることを検証する。

- [ ] **Step 2: テストの失敗を確認する**

Run: `cargo test -p speech2md-runtime engine::sherpa`
Expected: FAIL with unresolved diarization APIs.

- [ ] **Step 3: sherpa-onnx 1.13.8のstatic featureで実装する**

`OfflineSpeakerSegmentationPyannoteModelConfig`、`SpeakerEmbeddingExtractorConfig`、`FastClusteringConfig`から`OfflineSpeakerDiarizationConfig`を組み立てる。`num_clusters=0`を自動推定とし、正確な話者数がある場合だけ正数を設定する。結果を開始時刻順に変換し、話者番号を`SpeakerId`へ写す。ネイティブconfidenceは確率ではないため初期版では`None`にする。

- [ ] **Step 4: モデル必須のignored統合テストを書く**

`SPEECH2MD_MODEL_DIR`に導入済み話者モデルがある場合だけ実行し、リポジトリの短い自作複数話者fixtureを使って、2話者指定で2種類のIDと妥当な時刻区間が返ることを検証する。

- [ ] **Step 5: Windows static buildと通常テストを確認する**

Run: `cargo test -p speech2md-runtime`
Expected: PASS.

Run: `cargo check -p speech2md-runtime --target x86_64-pc-windows-msvc`
Expected: PASS without undeclared runtime DLL requirements.

- [ ] **Step 6: コミットする**

```bash
git add crates/speech2md-runtime
git commit -m "feat: diarize speakers with sherpa-onnx"
```

### Task 8: オフライン処理パイプラインと原子的出力

**Files:**
- Create: `crates/speech2md-runtime/src/pipeline.rs`
- Create: `crates/speech2md-runtime/src/output.rs`
- Modify: `crates/speech2md-runtime/src/lib.rs`
- Modify: `crates/speech2md-runtime/src/error.rs`
- Test: `crates/speech2md-runtime/tests/pipeline.rs`

**Interfaces:**
- Consumes: `Transcriber`, `Diarizer`, `ModelStore`, `TranscribeOptions`, 入力と出力の`Path`。
- Produces: `run_transcription(&RuntimeServices, &TranscribeOptions) -> Result<RunReport, RuntimeError>`、分類済み`RuntimeError`、完全な出力または無出力。

- [ ] **Step 1: fake engineを使う縦方向の失敗テストを書く**

```rust
#[test]
fn diarizer_failure_leaves_no_output_or_pcm_temp() {
    let fixture = PipelineFixture::with_diarizer_error();
    let result = run_transcription(&fixture.services(), &fixture.options());
    assert!(matches!(result, Err(RuntimeError::Diarization(_))));
    assert!(!fixture.output_path().exists());
    assert_eq!(std::fs::read_dir(fixture.temp_root()).unwrap().count(), 0);
}
```

成功経路ではfake transcriberとfake diarizerの呼出順を共有イベント列で確認し、生成CommonMarkを完全一致で検証する。別テストで空の文字起こしを`EmptyTranscript`にすること、atomic replace失敗時に既存bytesが不変であること、`RuntimeServices`の構築APIがdownloaderまたはHTTP clientを受け取らないことを固定する。

- [ ] **Step 2: テストの失敗を確認する**

Run: `cargo test -p speech2md-runtime --test pipeline`
Expected: FAIL with unresolved `run_transcription`.

- [ ] **Step 3: パイプラインを実装する**

処理順序を`decode_to_pcm`、`ModelStore::require`、`Transcriber::transcribe`、`Diarizer::diarize`、`assign_speakers`、`normalize_utterances`、`render_commonmark`、`atomic_write`に固定する。`RuntimeServices`にはデコーダー、transcriber、diarizer、model storeだけを含め、HTTP clientやinstallerを含めない。

- [ ] **Step 4: 原子的出力と上書き規則を実装する**

同じ出力ディレクトリにランダム名の一時ファイルを作り、`write_all`、`flush`、`sync_all`後にrenameする。既存出力は`force=false`なら`OutputExists`を返す。Windowsで`force=true`の場合は既存ファイルを退避名へrenameし、新ファイル確定後に退避を削除する。新ファイルのrenameに失敗した場合は退避を元へ戻す。

- [ ] **Step 5: キャンセルと古い一時領域の掃除を実装する**

`AtomicBool`をWhisperへ渡し、各処理段階の前後で確認する。sherpa呼び出し中は戻るまで待つ。起動時掃除は専用prefixと所有者markerを持ち、24時間より古いディレクトリだけを対象にする。計算したパスがOS一時ディレクトリ配下であることを確認してから削除する。

- [ ] **Step 6: パイプラインテストを通す**

Run: `cargo test -p speech2md-runtime --test pipeline`
Expected: PASS.

- [ ] **Step 7: コミットする**

```bash
git add crates/speech2md-runtime
git commit -m "feat: orchestrate offline transcription pipeline"
```

### Task 9: CLI、終了コード、診断表示

**Files:**
- Create: `crates/speech2md-cli/Cargo.toml`
- Create: `crates/speech2md-cli/src/main.rs`
- Create: `crates/speech2md-cli/src/args.rs`
- Create: `crates/speech2md-cli/src/commands.rs`
- Create: `crates/speech2md-cli/src/diagnostic.rs`
- Test: `crates/speech2md-cli/tests/cli.rs`

**Interfaces:**
- Consumes: Task 5のinstaller/store、Task 8の`run_transcription`と`RuntimeError`。
- Produces: `speech2md model install|list`、`speech2md transcribe`、`speech2md doctor`、安定した終了コード。

- [ ] **Step 1: assert_cmdによるCLI失敗テストを書く**

```rust
#[test]
fn transcribe_requires_an_input() {
    Command::cargo_bin("speech2md").unwrap()
        .args(["transcribe"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Usage:"));
}
```

別テストで、`--speakers 0`がexit 2になること、モデル不足がexit 4と正確な`model install`を表示すること、既存出力を`--force`なしで保持すること、command dispatchが`model install`分岐だけで`ModelInstaller`を生成することを検証する。

- [ ] **Step 2: CLIテストの失敗を確認する**

Run: `cargo test -p speech2md-cli --test cli`
Expected: FAIL because binary and arguments are absent.

- [ ] **Step 3: clapの引数型を実装する**

```rust
#[derive(Parser)]
struct Cli { #[command(subcommand)] command: Command }

#[derive(Subcommand)]
enum Command {
    Transcribe(TranscribeArgs),
    Model { #[command(subcommand)] command: ModelCommand },
    Doctor,
}
```

`TranscribeArgs`は`input`, `output`, `whisper`, `speakers`, `prompt`, `force`を持つ。`speakers`は1以上の整数だけを受け付け、省略時は自動推定する。

- [ ] **Step 4: command dispatchと終了コードを実装する**

成功0、使用法2、入力または設定3、モデル4、推論5、出力6、キャンセル130を割り当てる。診断には`error:`と、操作可能な場合だけ`help:`を表示する。音声本文と文字起こし本文を表示しない。

- [ ] **Step 5: `model list`と`doctor`を実装する**

`model list`はID、導入状態、サイズ、検証結果を表示する。`doctor`はWindows x86-64、CPUスレッド数、Whisper CPU情報、モデル存在、ハッシュ、書き込み可能な一時領域を検査し、音声やネットワークへアクセスしない。

- [ ] **Step 6: CLIテストとhelp snapshotを通す**

Run: `cargo test -p speech2md-cli`
Expected: PASS.

- [ ] **Step 7: コミットする**

```bash
git add crates/speech2md-cli Cargo.lock
git commit -m "feat: expose speech2md command line interface"
```

### Task 10: Linux CIと任意実行の重い検証

**Files:**
- Modify: `crates/speech2md-cli/tests/cli.rs`
- Create: `.github/workflows/ci.yml`
- Create: `.github/workflows/heavy.yml`
- Modify: `.gitignore`
- Test: `crates/speech2md-runtime/tests/fixtures/README.md`

**Interfaces:**
- Consumes: Task 1から9の全公開境界。
- Produces: UbuntuのモデルなしCI、手動起動するUbuntuの実モデルE2E、ローカルWindows release検証手順。

- [ ] **Step 1: fake enginesを注入したE2Eテストを追加する**

WAV入力から実バイナリ相当のcommand handlerを通し、話者と時刻を含むMarkdownが生成されること、pulldown-cmarkで解析できること、一時ファイルが残らないことを検証する。

- [ ] **Step 2: 実モデルE2Eをignoredで追加する**

四つのモデル環境変数と複数話者音声がある場合に、実CLIで`transcribe --speakers 2`を実行する。出力本文の完全一致は求めず、2種類以下の話者ラベル、時刻順、非空本文、CommonMark解析成功を検証する。

- [ ] **Step 3: テストの状態分離を監査する**

全テストについて、固定パス、process-globalなcurrent directory変更、共有可能な固定port、前のテストが作るファイル、実行順を前提にしていないことを確認する。環境変数を変更する必要があるテストは子プロセスへ閉じ込める。`serial_test`による順序固定で問題を隠さず、共有資源を依存注入または`TempDir`へ置き換える。

- [ ] **Step 4: Ubuntuの通常CIを作る**

```yaml
name: CI
on:
  pull_request:
  push:
    branches: [main]
permissions:
  contents: read
concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

jobs:
  test:
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy, llvm-tools-preview
      - run: rustup toolchain install nightly --profile minimal
      - uses: taiki-e/install-action@cargo-llvm-cov
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
      - name: Test three random sequential orders
        shell: pwsh
        run: |
          1..3 | ForEach-Object {
            cargo +nightly test --workspace -- -Z unstable-options --shuffle --test-threads=1
            if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
          }
      - run: cargo llvm-cov -p speech2md-core --fail-under-lines 100 --fail-under-functions 100
      - run: cargo llvm-cov -p speech2md-formats --fail-under-lines 100 --fail-under-functions 100
      - run: cargo llvm-cov --workspace --html --output-dir target/coverage
      - run: cargo build --release -p speech2md-cli
```

workflow triggerは`pull_request`と既定ブランチへの`push`に限定する。通常CIにはモデルのダウンロード、ignored test、SRV-DB、長時間性能テストを含めない。jobへ`timeout-minutes: 30`を設定し、ハングによる課金を制限する。

- [ ] **Step 5: Ubuntuの手動実モデルworkflowを作る**

```yaml
name: Heavy model tests
on:
  workflow_dispatch:
    inputs:
      run_native_model_tests:
        description: Download models and run ignored native inference tests
        required: true
        default: true
        type: boolean
permissions:
  contents: read

jobs:
  native-model-tests:
    if: ${{ inputs.run_native_model_tests }}
    runs-on: ubuntu-latest
    timeout-minutes: 120
    env:
      SPEECH2MD_MODEL_DIR: ${{ runner.temp }}/speech2md-models
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release -p speech2md-cli
      - run: cargo run --release -p speech2md-cli -- model install
      - run: cargo test -p speech2md-runtime --test whisper_model -- --ignored
      - run: cargo test -p speech2md-runtime --test diarization_model -- --ignored
```

workflow内では`SPEECH2MD_MODEL_DIR=${{ runner.temp }}/speech2md-models`を指定し、モデルをActions cacheやartifactへ保存しない。ignored test用の短い自作音声fixtureだけを使う。SRV-DBと長時間音声は取得しない。

- [ ] **Step 6: Windows releaseバイナリをローカルで検査する**

Windows上で`dumpbin /dependents target\release\speech2md.exe`を実行し、Visual C++ランタイム以外の未同梱DLLがないことを記録する。`speech2md --help`と`speech2md doctor`をクリーンな一時ディレクトリで実行する。

- [ ] **Step 7: 通常CI相当の検証をLinuxで通す**

Run: `cargo fmt --all --check`
Expected: PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

Run: `cargo test --workspace`
Expected: PASS with model tests ignored.

Run three times: `cargo +nightly test --workspace -- -Z unstable-options --shuffle --test-threads=1`
Expected: PASS three times; each invocation prints its shuffle seed. On failure, rerun with `--shuffle-seed SEED --test-threads=1` using the printed seed.

Run: `cargo llvm-cov -p speech2md-core --fail-under-lines 100 --fail-under-functions 100 --show-missing-lines`
Expected: PASS at 100%.

Run: `cargo llvm-cov -p speech2md-formats --fail-under-lines 100 --fail-under-functions 100 --show-missing-lines`
Expected: PASS at 100%.

Run: `cargo llvm-cov --workspace --html --output-dir target/coverage --show-missing-lines`
Expected: generate a workspace report; every uncovered production line has an external-native or OS-specific reason recorded in `docs/testing.md`.

Run: `cargo build --release -p speech2md-cli`
Expected: PASS and create the Linux `target/release/speech2md` binary.

- [ ] **Step 8: 手動CIとローカルWindows検証を通す**

Run the `Heavy model tests` workflow manually with `run_native_model_tests=true`.
Expected: PASS on Ubuntu without saving downloaded models as cache or artifact.

Run locally on Windows: `cargo build --release -p speech2md-cli`.
Expected: PASS and create `target/release/speech2md.exe`; then complete the `dumpbin`, `--help`, and `doctor` checks from Step 6.

- [ ] **Step 9: コミットする**

```bash
git add .github .gitignore crates
git commit -m "ci: test speech2md on Linux"
```

### Task 11: README、SRV-DB評価、性能記録

**Files:**
- Modify: `README.md`
- Create: `docs/testing.md`
- Create: `crates/speech2md-runtime/tests/srv_db.rs`
- Modify: `.gitignore`
- Test: `crates/speech2md-cli/tests/cli.rs`

**Interfaces:**
- Consumes: 完成したCLI、ローカルに配置したSRV-DB、Task 9のhelp。
- Produces: 利用者向け手順、再現可能な外部評価、READMEコマンド例の回帰テスト。

- [ ] **Step 1: READMEの使用例をCLIテストへ先に追加する**

次のコマンドをREADME契約として引数解析テストへ追加する。

```console
speech2md model install
speech2md transcribe meeting.m4a
speech2md transcribe meeting.wav --speakers 3 --prompt "Rust, Kubernetes, PostgreSQL"
speech2md transcribe meeting.mp3 --whisper small --output minutes.md
speech2md model list
speech2md doctor
```

Run: `cargo test -p speech2md-cli --test cli readme_`
Expected: PASS only after all documented arguments exist.

- [ ] **Step 2: READMEを設計仕様の項目順で記述する**

対象環境、対応形式、インストール、初回モデル導入、基本操作、話者数、モデルとprompt、出力例、オフライン保証、モデル保存場所、速度と精度、制約、トラブルシューティング、SRV-DB謝辞を記載する。モデルのライセンスと保存容量を表にし、`transcribe`が暗黙に取得しないことを明記する。

- [ ] **Step 3: SRV-DB外部評価テストを書く**

`SRV_DB_DIR`がない場合は明示的にskipするignored testを作る。公開原稿と対応するローカル音声についてCER、話速別CER、検出話者数、ラベル一貫性、実時間係数、ピークメモリをJSONで標準出力へ出す。音声、原稿、推論本文をリポジトリまたはログファイルへコピーしない。DERは算出しない。

- [ ] **Step 4: 外部評価手順とデータ境界を記述する**

`docs/testing.md`に公式URL、利用条件、対象データセット4と5、配置例、`SRV_DB_DIR`、実行コマンド、指標定義、結果の読み方を記載する。`.gitignore`へ`test-data/srv-db/`と評価生成物を追加する。

- [ ] **Step 5: 1時間と3時間の性能測定を実施する**

代表的なWindows CPUでWhisper baseと話者分離を実行し、CPU名、論理コア数、音声時間、処理時間、実時間係数、ピークメモリ、一時ディスク量をREADMEへ記録する。実時間係数1.0を超えた場合も実測値をそのまま記載し、保証値へ書き換えない。

- [ ] **Step 6: 文書と全workspaceを検証する**

Run: `cargo test --workspace`
Expected: PASS.

Run: `cargo test -p speech2md-runtime --test srv_db -- --ignored` with `SRV_DB_DIR` and model variables set locally.
Expected: PASS and print a metrics JSON object without copying dataset files.

Run: `rg -n "SRV-DB|model install|transcribe|オフライン|CommonMark" README.md docs/testing.md`
Expected: each required topic appears in the appropriate document.

- [ ] **Step 7: コミットする**

```bash
git add README.md docs/testing.md crates/speech2md-runtime/tests/srv_db.rs .gitignore crates/speech2md-cli/tests/cli.rs
git commit -m "docs: add setup and evaluation guide"
```

## 最終検証

- [ ] `cargo fmt --all --check`が成功する。
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`が成功する。
- [ ] `cargo test --workspace`がモデルとネットワークなしで成功する。
- [ ] ランダム順かつ単一スレッドのworkspaceテストが異なるseedで3回成功する。
- [ ] テストが固定パス、固定port、current directory変更、別テストの生成物へ依存しない。
- [ ] PRとpushの通常GitHub Actionsが`ubuntu-latest`だけでモデル不要テストを実行する。
- [ ] 実モデルテストは`workflow_dispatch`でだけ起動し、モデルをcacheまたはartifactへ保存しない。
- [ ] GitHub ActionsのworkflowにWindows runner、SRV-DB、1時間または3時間の入力を含めない。
- [ ] `speech2md-core`と`speech2md-formats`のline coverageとfunction coverageが100%になる。
- [ ] workspace全体のカバレッジレポートを生成し、100%未達のproduction lineごとに理由と代替検証を記録する。
- [ ] 外部依存で妥当な理由がない限り、`speech2md-runtime`と`speech2md-cli`もline coverage 100%になる。
- [ ] 実モデルを指定したignoredテストがWindows x86-64で成功する。
- [ ] `cargo build --release -p speech2md-cli`が成功する。
- [ ] `speech2md model install`以外のコマンドがHTTP clientを構築しない。
- [ ] WAV、MP3、AAC-LC/M4Aのfixtureが同じ16kHz mono契約を満たす。
- [ ] 既存出力、推論失敗、キャンセルの各経路で不完全な出力を残さない。
- [ ] READMEの全コマンド例が現在のCLIで解析できる。
- [ ] SRV-DBの音声、原稿、派生物がGitの追跡対象にない。
