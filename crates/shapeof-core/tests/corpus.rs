use std::path::PathBuf;

use serde_json::Value;
use shapeof_core::{Target, report};

fn corpus() -> Value {
    let path = std::env::var_os("SHAPEOF_CORPUS")
        .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/cases.json"), PathBuf::from);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let corpus: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

    assert_eq!(corpus["format"], 1, "{}: corpus format", path.display());
    corpus
}

fn target(name: &str) -> Target {
    match name {
        "openai" => Target::OpenAi,
        "anthropic" => Target::Anthropic,
        "plain" => Target::Plain,
        other => panic!("unknown target {other}"),
    }
}

#[test]
fn corpus_cases() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");

    assert!(!cases.is_empty(), "no cases");

    for case in cases {
        let name = case["name"].as_str().expect("name");
        let input = case["input"].as_str().expect("input");
        let got: Value =
            serde_json::from_str(&report(input, target(case["target"].as_str().expect("target")))).expect("JSON");

        assert_eq!(got, case["want"], "{name}");
    }
}
