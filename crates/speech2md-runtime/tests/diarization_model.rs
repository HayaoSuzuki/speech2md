use std::collections::BTreeSet;
use std::path::PathBuf;

use speech2md_runtime::decode_to_pcm;
use speech2md_runtime::engine::{DiarizationRequest, Diarizer, SherpaDiarizer};

#[test]
#[ignore = "requires locally installed speaker models and a local four-speaker fixture"]
fn diarizes_a_local_four_speaker_fixture_without_downloading() {
    let Some(model_root) = std::env::var_os("SPEECH2MD_MODEL_DIR").map(PathBuf::from) else {
        return;
    };
    let Some(fixture) = std::env::var_os("SPEECH2MD_DIARIZATION_FIXTURE").map(PathBuf::from) else {
        return;
    };
    assert!(
        fixture.is_file(),
        "SPEECH2MD_DIARIZATION_FIXTURE must name a readable file"
    );
    let temporary = tempfile::tempdir().expect("temporary processing root");
    let pcm = decode_to_pcm(&fixture, temporary.path()).expect("decode fixture");
    let diarizer = SherpaDiarizer::new(
        &model_root.join("segmentation-3-0.onnx"),
        &model_root.join("3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx"),
        1,
    )
    .expect("initialize sherpa-onnx");
    let turns = diarizer
        .diarize(
            pcm.samples(),
            &DiarizationRequest {
                num_speakers: Some(4),
            },
        )
        .expect("diarize fixture");

    assert!(!turns.is_empty());
    assert_eq!(
        turns
            .iter()
            .map(|turn| turn.speaker.as_u32())
            .collect::<BTreeSet<_>>()
            .len(),
        4
    );
    assert!(turns.iter().all(|turn| turn.span.duration_ms() > 0));
    assert!(
        turns
            .windows(2)
            .all(|pair| pair[0].span.start() <= pair[1].span.start())
    );
}
