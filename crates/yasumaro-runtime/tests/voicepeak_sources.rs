use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const NARRATORS: [&str; 4] = ["男性1", "男性2", "男性3", "女性1"];

fn source_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test-data/voicepeak")
}

fn read_rows(name: &str) -> Vec<(String, String)> {
    let path = source_dir().join(name);
    let contents = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));

    contents
        .lines()
        .enumerate()
        .map(|(index, line)| {
            assert!(
                !line.is_empty(),
                "{}:{} is blank",
                path.display(),
                index + 1
            );
            assert!(
                !line.contains('\t'),
                "{}:{} contains a tab",
                path.display(),
                index + 1
            );
            let fields = line.split(',').collect::<Vec<_>>();
            assert_eq!(
                fields.len(),
                2,
                "{}:{} must contain exactly one ASCII comma",
                path.display(),
                index + 1
            );
            assert!(
                NARRATORS.contains(&fields[0]),
                "{}:{} has an unknown narrator",
                path.display(),
                index + 1
            );
            assert!(
                !fields[1].trim().is_empty(),
                "{}:{} has empty dialogue",
                path.display(),
                index + 1
            );
            (fields[0].to_owned(), fields[1].to_owned())
        })
        .collect()
}

fn narrator_counts(rows: &[(String, String)]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for (narrator, _) in rows {
        *counts.entry(narrator.clone()).or_default() += 1;
    }
    counts
}

#[test]
fn all_voicepeak_sources_are_importable_two_column_csv() {
    let expected = [
        "balanced-4speakers.csv",
        "imbalanced-4speakers.csv",
        "male-3speakers.csv",
        "pair-male1-male3.csv",
        "pair-male1-male2.csv",
        "pair-male2-male3.csv",
        "single-female1.csv",
        "single-male1.csv",
        "single-male3.csv",
        "single-male2.csv",
    ];

    for name in expected {
        assert!(!read_rows(name).is_empty(), "{name} must contain dialogue");
    }
}

#[test]
fn balanced_sources_give_each_narrator_equal_turns() {
    let cases = [
        (
            "balanced-4speakers.csv",
            vec!["女性1", "男性1", "男性2", "男性3"],
        ),
        ("male-3speakers.csv", vec!["男性1", "男性2", "男性3"]),
    ];

    for (name, expected_narrators) in cases {
        let counts = narrator_counts(&read_rows(name));
        assert_eq!(
            counts.keys().map(String::as_str).collect::<Vec<_>>(),
            expected_narrators,
            "unexpected narrators in {name}"
        );
        let distinct_counts = counts.values().copied().collect::<BTreeSet<_>>();
        assert_eq!(distinct_counts.len(), 1, "{name} is not turn-balanced");
        assert!(counts.values().all(|count| *count >= 4));
    }
}

#[test]
fn pair_and_single_sources_contain_the_expected_narrators() {
    let cases = [
        ("pair-male1-male3.csv", vec!["男性1", "男性3"]),
        ("pair-male1-male2.csv", vec!["男性1", "男性2"]),
        ("pair-male2-male3.csv", vec!["男性2", "男性3"]),
        ("single-female1.csv", vec!["女性1"]),
        ("single-male1.csv", vec!["男性1"]),
        ("single-male3.csv", vec!["男性3"]),
        ("single-male2.csv", vec!["男性2"]),
    ];

    for (name, expected) in cases {
        let actual = narrator_counts(&read_rows(name))
            .into_keys()
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "unexpected narrators in {name}");
    }
}

#[test]
fn imbalanced_source_has_one_deliberately_short_narrator() {
    let counts = narrator_counts(&read_rows("imbalanced-4speakers.csv"));
    assert_eq!(counts.len(), 4);
    assert_eq!(counts.get("男性3"), Some(&1));
    assert!(
        counts
            .iter()
            .filter(|(narrator, _)| **narrator != "男性3")
            .all(|(_, count)| *count == 6)
    );
}
