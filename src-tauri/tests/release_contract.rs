use serde_json::Value;
use std::path::Path;

fn contains_rust_source(path: &Path) -> bool {
    if path.is_file() {
        return path.extension().is_some_and(|extension| extension == "rs");
    }
    if !path.is_dir() {
        return false;
    }

    path.read_dir()
        .expect("release source directory must be readable")
        .filter_map(Result::ok)
        .any(|entry| contains_rust_source(&entry.path()))
}

#[test]
fn release_identity_is_not_scaffold_metadata() {
    assert_eq!(env!("CARGO_PKG_NAME"), "document-summarizer");
    assert_eq!(env!("CARGO_PKG_AUTHORS"), "Juan Canfield");
    assert_eq!(
        env!("CARGO_PKG_DESCRIPTION"),
        "Local, evidence-grounded summaries for native-text PDF documents"
    );

    let config: Value = serde_json::from_str(include_str!("../tauri.conf.json"))
        .expect("base Tauri config must be valid JSON");
    assert_eq!(config["productName"], "Document Summarizer");
    assert_eq!(config["bundle"]["targets"], "all");
    assert_eq!(config["bundle"]["publisher"], "Juan Canfield");
    assert_eq!(config["bundle"]["category"], "Productivity");
    assert!(config["bundle"]["shortDescription"]
        .as_str()
        .is_some_and(|description| !description.trim().is_empty()));
}

#[test]
fn linux_release_targets_only_the_supported_bundle() {
    let config: Value = serde_json::from_str(include_str!("../tauri.linux.conf.json"))
        .expect("Linux Tauri config must be valid JSON");
    assert_eq!(config["bundle"]["targets"], serde_json::json!(["deb"]));
}

#[test]
fn legacy_pdf_probes_cannot_be_discovered_as_release_binaries() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let conventional_bin_dir = manifest_dir.join("src/bin");

    assert!(contains_rust_source(&manifest_dir.join("src")));
    assert!(
        !contains_rust_source(&conventional_bin_dir),
        "src/bin must not contain release-discoverable Rust probes"
    );
    assert!(manifest_dir.join("tools/legacy/test_min_pdf.rs").is_file());
    assert!(manifest_dir.join("tools/legacy/test_pdf.rs").is_file());
}

#[test]
fn inference_locality_disclosure_matches_runtime_boundaries() {
    let readme = include_str!("../../README.md");
    let contracts = include_str!("../../docs/CONTRACTS.md");
    let initial_ui = include_str!("../../index.html");
    let refreshed_ui = include_str!("../../src/main.ts");
    let normalized = [readme, contracts, initial_ui, refreshed_ui]
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "));

    for disclosure in &normalized {
        assert!(disclosure.contains("complete document-derived model prompts"));
        assert!(disclosure.contains("selected source excerpts"));
    }
    assert!(normalized[0].contains("may be outside this workstation"));
    assert!(normalized[1].contains("administrator-configured HTTPS origin"));
    assert!(normalized[1].contains("native loopback Ollama"));
    assert!(normalized[1].contains("app-managed local llama.cpp"));
    for release_surface in [&normalized[0], &normalized[2], &normalized[3]] {
        assert!(!release_surface.contains("on-prem"));
    }
}

#[test]
fn source_retention_description_matches_path_only_ingestion() {
    let readme = include_str!("../../README.md")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    assert!(readme.contains("records the source file's path and identity without copying the PDF"));
    assert!(!readme.contains("stores imported native-text PDFs"));
}
