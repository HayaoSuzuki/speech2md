use std::collections::BTreeMap;
use std::path::PathBuf;

use yasumaro_runtime::engine::{DiarizationRequest, Diarizer, SherpaDiarizer};
use yasumaro_runtime::{ModelId, ModelStore, decode_to_pcm};

#[test]
#[ignore = "requires locally installed speaker models and a local four-speaker fixture"]
fn diarizes_a_local_four_speaker_fixture_without_downloading() {
    let Some(model_root) = std::env::var_os("YASUMARO_MODEL_DIR").map(PathBuf::from) else {
        return;
    };
    let Some(fixture) = std::env::var_os("YASUMARO_DIARIZATION_FIXTURE").map(PathBuf::from) else {
        return;
    };
    assert!(
        fixture.is_file(),
        "YASUMARO_DIARIZATION_FIXTURE must name a readable file"
    );
    let temporary = tempfile::tempdir().expect("temporary processing root");
    let pcm = decode_to_pcm(&fixture, temporary.path()).expect("decode fixture");
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let model_store = ModelStore::new(model_root);
    let segmentation = model_store
        .acquire(ModelId::SpeakerSegmentation)
        .expect("installed speaker segmentation model");
    let embedding = model_store
        .acquire(ModelId::SpeakerEmbedding)
        .expect("installed speaker embedding model");
    let diarizer =
        SherpaDiarizer::new(segmentation, embedding, threads).expect("initialize sherpa-onnx");
    let turns = diarizer
        .diarize(
            pcm.samples(),
            &DiarizationRequest {
                num_speakers: Some(4),
            },
        )
        .expect("diarize fixture");

    assert!(!turns.is_empty());
    let mut speaker_durations = BTreeMap::<u32, u64>::new();
    for turn in &turns {
        *speaker_durations.entry(turn.speaker.as_u32()).or_default() += turn.span.duration_ms();
    }
    assert_eq!(
        speaker_durations.len(),
        4,
        "speaker durations: {speaker_durations:?}"
    );
    assert!(turns.iter().all(|turn| turn.span.duration_ms() > 0));
    assert!(
        turns
            .windows(2)
            .all(|pair| pair[0].span.start() <= pair[1].span.start())
    );
}
