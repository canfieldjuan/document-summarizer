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
