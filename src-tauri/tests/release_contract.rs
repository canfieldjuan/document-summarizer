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
    assert!(readme.contains("Keep the source file readable until parsing completes."));
    assert!(!readme.contains("stores imported native-text PDFs"));
}

#[test]
fn windows_connect_release_boundary_is_documented_and_exercised_natively() {
    let contracts = include_str!("../../docs/CONTRACTS.md")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let workflow = include_str!("../../.github/workflows/rust.yml");

    assert!(contracts.contains(r"%LOCALAPPDATA%\LocalConnect\runtime\v1\providers"));
    assert!(contracts.contains("protected DACL"));
    assert!(contracts.contains("fixed same-directory temporary file"));
    assert!(contracts.contains("installed Windows cited-summary demonstration is complete"));
    assert!(workflow.contains("runs-on: windows-2022"));
    assert!(workflow.contains("cargo test --locked --lib connect::"));
    assert!(workflow.contains("cargo clippy --locked --lib --tests -- -D warnings"));
}

#[test]
fn installed_linux_cited_summary_release_proof_is_recorded() {
    let contracts = include_str!("../../docs/CONTRACTS.md")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let ledger = include_str!("../../docs/BUILD_LEDGER.md")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    assert!(ledger.contains("Slice 38 — Installed Linux Cited-Summary Demonstration"));
    assert!(ledger.contains("39274d7e70788489d014c3c48437a65a88a4168d8169600ad5fce4c605f7b8c8"));
    assert!(ledger.contains("Package-manager-installed Linux cited-summary"));
    assert!(contracts.contains("installed Linux cited-summary demonstration is complete"));
}

#[test]
fn installed_windows_cited_summary_release_proof_is_recorded() {
    let contracts = include_str!("../../docs/CONTRACTS.md")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let readme = include_str!("../../README.md")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let ledger = include_str!("../../docs/BUILD_LEDGER.md")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    assert!(ledger.contains("Slice 39 — Installed Windows Cited-Summary Demonstration"));
    assert!(ledger.contains("3d2ed4ffd3676328fc3e50bd30ac2afdca45564d5b2a6aa86c03d82dac1c0d6a"));
    assert!(ledger.contains("87fbd8e8b86640ba620b37839e16c98ea828e94d1dd0e906a7ef81ee980db8f7"));
    assert!(ledger.contains("7317f6ba-85cb-4a1f-ba63-f1b853cbaebb"));
    assert!(ledger.contains("VM-only loopback bridge"));
    assert!(contracts.contains("installed Windows cited-summary demonstration is complete"));
    assert!(!contracts.contains("installed Windows cited-summary demonstration remains pending"));
    assert!(contracts.contains("Native Windows CI produces MSI and NSIS installers"));
    assert!(!contracts.contains(
        "AppImage, RPM, macOS, and Windows packaging remain separate target-platform work"
    ));
    assert!(readme.contains("On Windows, native CI produces MSI and NSIS installers"));
    assert!(readme.contains("installed-app lifecycle is proven for the MSI"));
    assert!(!readme.contains(
        "Other operating-system bundle formats are deferred until they can be built and exercised on their target platforms"
    ));
}
