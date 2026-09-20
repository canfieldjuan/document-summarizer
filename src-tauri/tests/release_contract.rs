use serde_json::Value;
use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::Command;

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
fn linux_bundle_installs_disabled_background_provider_unit_with_bounded_supervision() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config: Value = serde_json::from_str(include_str!("../tauri.linux.conf.json"))
        .expect("Linux Tauri config must be valid JSON");
    assert_eq!(
        config["bundle"]["linux"]["deb"]["files"]
            ["/usr/lib/systemd/user/document-summarizer-connect.service"],
        "linux/document-summarizer-connect.service"
    );

    let unit_path = manifest_dir.join("linux/document-summarizer-connect.service");
    let unit = std::fs::read_to_string(unit_path)
        .expect("the Linux bundle must carry its systemd user unit");
    assert!(unit.contains("ExecStart=/usr/bin/document-summarizer --connect-provider"));
    assert!(unit.contains("Restart=on-failure"));
    assert!(unit.contains("RestartSec=5s"));
    assert!(unit.contains("StartLimitIntervalSec=300"));
    assert!(unit.contains("StartLimitBurst=5"));
    assert!(unit.contains("TimeoutStopSec=40s"));
    assert!(unit.contains("KillMode=control-group"));
    assert!(
        unit.contains("EnvironmentFile=%h/.local/state/document-summarizer/connect-provider.env")
    );
    assert!(unit.contains("WantedBy=default.target"));
    assert!(!unit.contains("Alias="));
}

#[test]
fn debian_package_hooks_coordinate_provider_ownership() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config: Value = serde_json::from_str(include_str!("../tauri.linux.conf.json"))
        .expect("Linux Tauri config must be valid JSON");
    let deb = &config["bundle"]["linux"]["deb"];
    let scripts = [
        ("preInstallScript", "linux/debian/preinst"),
        ("postInstallScript", "linux/debian/postinst"),
        ("preRemoveScript", "linux/debian/prerm"),
        ("postRemoveScript", "linux/debian/postrm"),
    ];
    for (field, relative) in scripts {
        assert_eq!(deb[field], relative);
        let body = std::fs::read_to_string(manifest_dir.join(relative))
            .expect("Debian lifecycle script must be packaged");
        assert!(body.starts_with("#!/bin/sh\nset -eu\n"));
    }

    let preinst = include_str!("../linux/debian/preinst");
    let postinst = include_str!("../linux/debian/postinst");
    let prerm = include_str!("../linux/debian/prerm");
    let postrm = include_str!("../linux/debian/postrm");
    assert!(preinst.contains("package-quiesce-v1.record"));
    assert!(preinst.contains("sha256sum"));
    assert!(!preinst.contains("/usr/bin/document-summarizer --connect-package prepare-upgrade"));
    assert!(preinst.contains("--connect-package prepare-reinstall"));
    assert!(postinst.contains("--connect-package adopt-bootstrap"));
    assert!(postinst.contains("--connect-package initialize"));
    assert!(postinst.contains("--connect-package recover-install"));
    assert!(prerm.contains("--connect-package prepare-remove"));
    assert!(postrm.contains("--connect-package finish-remove"));
    for script in [preinst, postinst, prerm, postrm] {
        assert!(script.contains(env!("CARGO_PKG_VERSION")));
        assert!(!script.contains("systemctl"));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn first_upgrade_bootstrap_never_executes_the_legacy_binary() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let temporary = tempfile::tempdir().unwrap();
    let package_root = temporary.path().join("package-root");
    let legacy_marker = temporary.path().join("legacy-invoked");
    let legacy = temporary.path().join("legacy-document-summarizer");
    std::fs::write(
        &legacy,
        format!("#!/bin/sh\ntouch '{}'\nexit 99\n", legacy_marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&legacy, std::fs::Permissions::from_mode(0o755)).unwrap();
    let runtime = temporary.path().join("run-user");
    let script = preinst_for_test(&package_root, &legacy, &runtime);
    let script_path = temporary.path().join("preinst");
    std::fs::write(&script_path, script).unwrap();
    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();

    let status = Command::new("sh")
        .arg(&script_path)
        .args(["upgrade", "0.0.9"])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(!legacy_marker.exists());
    let bootstrap = package_root.join("package-quiesce-v1.record");
    let first_bootstrap = std::fs::read(&bootstrap).unwrap();
    let bootstrap_text = std::str::from_utf8(&first_bootstrap).unwrap();
    let generation = bootstrap_text
        .lines()
        .find_map(|line| line.strip_prefix("generation="))
        .unwrap();
    let quarantine = package_root.join(format!("legacy-executable-{generation}"));
    assert!(quarantine.is_file());
    assert_eq!(
        std::fs::symlink_metadata(&quarantine).unwrap().mode() & 0o777,
        0o600
    );
    assert_eq!(Command::new(&legacy).status().unwrap().code(), Some(75));
    let replay = Command::new("sh")
        .arg(&script_path)
        .args(["upgrade", "0.0.9"])
        .status()
        .unwrap();
    assert!(replay.success());
    assert!(!legacy_marker.exists());
    assert_eq!(std::fs::read(&bootstrap).unwrap(), first_bootstrap);
    let metadata = std::fs::symlink_metadata(&bootstrap).unwrap();
    assert_eq!(metadata.mode() & 0o777, 0o600);
    assert!(std::fs::read_to_string(bootstrap)
        .unwrap()
        .contains("kind=upgrade\n"));
}

#[cfg(target_os = "linux")]
fn preinst_for_test(package_root: &Path, installed_binary: &Path, runtime_root: &Path) -> String {
    include_str!("../linux/debian/preinst")
        .replace(
            "/var/lib/document-summarizer",
            package_root.to_str().unwrap(),
        )
        .replace(
            "/usr/bin/document-summarizer",
            installed_binary.to_str().unwrap(),
        )
        .replace("/run/user", runtime_root.to_str().unwrap())
        .replace(
            "expected_root_uid=0",
            &format!("expected_root_uid={}", unsafe { libc::geteuid() }),
        )
        .replace(" -o root -g root", "")
}

#[cfg(target_os = "linux")]
#[test]
fn preinst_rejects_unsafe_existing_bootstrap_before_success() {
    use std::os::unix::fs::PermissionsExt;

    let temporary = tempfile::tempdir().unwrap();
    let package_root = temporary.path().join("package-root");
    std::fs::create_dir(&package_root).unwrap();
    std::fs::set_permissions(&package_root, std::fs::Permissions::from_mode(0o755)).unwrap();
    let installed = temporary.path().join("document-summarizer");
    std::fs::copy("/bin/true", &installed).unwrap();
    std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755)).unwrap();
    let runtime = temporary.path().join("run-user");
    std::fs::create_dir(&runtime).unwrap();
    let script = temporary.path().join("preinst");
    std::fs::write(
        &script,
        preinst_for_test(&package_root, &installed, &runtime),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        package_root.join("package-quiesce-v1.record"),
        b"tampered\n",
    )
    .unwrap();
    std::fs::set_permissions(
        package_root.join("package-quiesce-v1.record"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();

    let status = Command::new("sh")
        .arg(&script)
        .args(["upgrade", "0.0.9"])
        .status()
        .unwrap();
    assert!(!status.success());

    let bootstrap = package_root.join("package-quiesce-v1.record");
    std::fs::remove_file(&bootstrap).unwrap();
    let valid = Command::new("sh")
        .arg(&script)
        .args(["upgrade", "0.0.9"])
        .status()
        .unwrap();
    assert!(valid.success());
    let valid_bytes = std::fs::read(&bootstrap).unwrap();

    std::fs::set_permissions(&bootstrap, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!Command::new("sh")
        .arg(&script)
        .args(["upgrade", "0.0.9"])
        .status()
        .unwrap()
        .success());
    std::fs::set_permissions(&bootstrap, std::fs::Permissions::from_mode(0o600)).unwrap();

    assert!(!Command::new("sh")
        .arg(&script)
        .args(["upgrade", "0.0.8"])
        .status()
        .unwrap()
        .success());

    let mut tampered = valid_bytes.clone();
    let phase = tampered
        .windows(b"phase=quiesced".len())
        .position(|window| window == b"phase=quiesced")
        .unwrap();
    tampered[phase + "phase=".len()] = b'x';
    std::fs::write(&bootstrap, tampered).unwrap();
    assert!(!Command::new("sh")
        .arg(&script)
        .args(["upgrade", "0.0.9"])
        .status()
        .unwrap()
        .success());

    std::fs::remove_file(&bootstrap).unwrap();
    let linked = package_root.join("bootstrap-hardlink-fixture");
    std::fs::write(&linked, &valid_bytes).unwrap();
    std::fs::set_permissions(&linked, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::hard_link(&linked, &bootstrap).unwrap();
    assert!(!Command::new("sh")
        .arg(&script)
        .args(["upgrade", "0.0.9"])
        .status()
        .unwrap()
        .success());

    std::fs::remove_file(&bootstrap).unwrap();
    let symlink_target = package_root.join("bootstrap-symlink-fixture");
    std::fs::write(&symlink_target, valid_bytes).unwrap();
    std::fs::set_permissions(&symlink_target, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::os::unix::fs::symlink(&symlink_target, &bootstrap).unwrap();
    assert!(!Command::new("sh")
        .arg(script)
        .args(["upgrade", "0.0.9"])
        .status()
        .unwrap()
        .success());
}

#[cfg(target_os = "linux")]
#[test]
fn preinst_crash_boundaries_resume_the_exact_generation() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let boundaries = [
        "    write_bootstrap quarantining\n",
        "        chmod 0600 \"$installed_binary\"\n",
        "          sync -f \"$quarantine_temporary\"\n",
        "        mv -T \"$quarantine_temporary\" \"$quarantine\"\n",
        "        rm -f \"$installed_binary\"\n        sync -f \"$(dirname \"$installed_binary\")\"\n",
        "      install_wrapper\n",
        "      # bootstrap-crash-boundary-after-terminate\n",
        "      # bootstrap-crash-boundary-after-registration-cleanup\n",
        "      write_bootstrap quiesced\n",
    ];
    for (index, boundary) in boundaries.into_iter().enumerate() {
        let temporary = tempfile::tempdir().unwrap();
        let package_root = temporary.path().join("package-root");
        let installed = temporary.path().join("document-summarizer");
        std::fs::copy("/bin/true", &installed).unwrap();
        std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755)).unwrap();
        let runtime = temporary.path().join("run-user");
        let normal = preinst_for_test(&package_root, &installed, &runtime);
        assert_eq!(normal.matches(boundary).count(), 1, "boundary {index}");
        let crashing = normal.replacen(boundary, &format!("{boundary}      exit 86\n"), 1);
        let script = temporary.path().join("preinst");
        std::fs::write(&script, crashing).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            Command::new("sh")
                .arg(&script)
                .args(["upgrade", "0.0.9"])
                .status()
                .unwrap()
                .code(),
            Some(86),
            "boundary {index}"
        );
        std::fs::write(&script, normal).unwrap();
        assert!(Command::new("sh")
            .arg(&script)
            .args(["upgrade", "0.0.9"])
            .status()
            .unwrap()
            .success());
        let bootstrap =
            std::fs::read_to_string(package_root.join("package-quiesce-v1.record")).unwrap();
        assert!(bootstrap.contains("phase=quiesced\n"), "boundary {index}");
        let generation = bootstrap
            .lines()
            .find_map(|line| line.strip_prefix("generation="))
            .unwrap();
        let quarantine = package_root.join(format!("legacy-executable-{generation}"));
        assert_eq!(
            std::fs::symlink_metadata(quarantine).unwrap().mode() & 0o777,
            0o600,
            "boundary {index}"
        );
        assert_eq!(
            Command::new(&installed).status().unwrap().code(),
            Some(75),
            "boundary {index}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn preinst_quiesces_live_legacy_owner_without_executing_it() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    let temporary = tempfile::tempdir().unwrap();
    let package_root = temporary.path().join("package-root");
    let installed = temporary.path().join("document-summarizer");
    std::fs::copy("/bin/sleep", &installed).unwrap();
    std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut legacy = Command::new(&installed).arg("30").spawn().unwrap();
    let runtime_base = temporary.path().join("run-user");
    let runtime = runtime_base.join(unsafe { libc::geteuid() }.to_string());
    let providers = runtime.join("local-connect/v1/providers");
    std::fs::create_dir_all(&providers).unwrap();
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(&providers, std::fs::Permissions::from_mode(0o700)).unwrap();
    let registration = providers.join("document-summarizer-fixture.json");
    std::fs::write(
        &registration,
        format!(
            "{{\n  \"protocol_version\": 1,\n  \"app_id\": \"document-summarizer\",\n  \"pid\": {},\n  \"transport\": {{}},\n  \"auth\": {{}}\n}}\n",
            legacy.id()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&registration, std::fs::Permissions::from_mode(0o600)).unwrap();
    let script = temporary.path().join("preinst");
    let normal = preinst_for_test(&package_root, &installed, &runtime_base);
    let boundary = "      # bootstrap-crash-boundary-after-terminate\n";
    assert_eq!(normal.matches(boundary).count(), 1);
    let crashing = normal.replacen(boundary, &format!("{boundary}      exit 86\n"), 1);
    std::fs::write(&script, crashing).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let crashed = Command::new("sh")
        .arg(&script)
        .args(["upgrade", "0.0.9"])
        .status()
        .unwrap();
    assert_eq!(crashed.code(), Some(86));
    std::fs::write(&script, normal).unwrap();
    let status = Command::new("sh")
        .arg(&script)
        .args(["upgrade", "0.0.9"])
        .status()
        .unwrap();
    let stopped = (0..20).any(|_| {
        if legacy.try_wait().unwrap().is_some() {
            true
        } else {
            std::thread::sleep(Duration::from_millis(10));
            false
        }
    });
    if !stopped {
        legacy.kill().unwrap();
        legacy.wait().unwrap();
    }
    assert!(status.success());
    assert!(stopped);
    assert!(!registration.exists());
    assert_eq!(Command::new(&installed).status().unwrap().code(), Some(75));
    let bootstrap = package_root.join("package-quiesce-v1.record");
    let first_bootstrap = std::fs::read(&bootstrap).unwrap();
    let replay = Command::new("sh")
        .arg(script)
        .args(["upgrade", "0.0.9"])
        .status()
        .unwrap();
    assert!(replay.success());
    assert_eq!(std::fs::read(bootstrap).unwrap(), first_bootstrap);
}

#[test]
fn authenticated_request_body_is_consumed_before_lifecycle_admission() {
    let provider = include_str!("../src/connect/provider.rs");
    let request_start = provider
        .find("async fn create_job_for_request")
        .expect("request handler must exist");
    let request_path = &provider[request_start..];
    let stream = request_path
        .find("Multipart::from_request")
        .expect("request body must be authenticated before streaming");
    let package = request_path
        .find("package_admission_for_job")
        .expect("job commit must acquire package admission");
    assert!(
        stream < package,
        "package admission must not cover body streaming"
    );
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
    assert!(workflow.contains("repository: canfieldjuan/connect-contracts"));
    assert!(workflow.contains("ref: 3005d82a7be885fba36f8688b5967a5b56a0abea"));
    assert!(workflow.contains("LOCAL_CONNECT_ENTITLEMENT_KEYRING_FILE:"));
    assert!(workflow.contains("connect-contracts\\entitlements\\v1\\release\\keyring.json"));
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
