use serde_json::Value;
use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::Command;

#[cfg(target_os = "linux")]
enum ExecutableFixture<'a> {
    Copy(&'a Path),
    Bytes(&'a [u8], u32),
}

#[cfg(target_os = "linux")]
fn create_executable_fixture(destination: &Path, fixture: ExecutableFixture<'_>) {
    match fixture {
        ExecutableFixture::Copy(source) => {
            assert!(Command::new("cp")
                .arg("--")
                .arg(source)
                .arg(destination)
                .status()
                .unwrap()
                .success());
        }
        ExecutableFixture::Bytes(bytes, initial_mode) => {
            use std::io::Write;
            use std::os::unix::fs::PermissionsExt;
            // Like fs::write, rewriting a fixture preserves its existing mode.
            let mode = match std::fs::metadata(destination) {
                Ok(metadata) => metadata.permissions().mode() & 0o7777,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => initial_mode,
                Err(error) => panic!("fixture metadata: {error}"),
            };
            let mut child = Command::new("sh")
                .arg("-c")
                .arg("set -eu\ncat > \"$1\"\nchmod \"$2\" \"$1\"")
                .arg("executable-fixture")
                .arg(destination)
                .arg(format!("{mode:o}"))
                .stdin(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(bytes).unwrap();
            assert!(child.wait().unwrap().success());
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn executable_fixture_copy_preserves_bytes_and_has_independent_ownership() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("fixture with spaces");
    let source = Path::new("/bin/true");
    create_executable_fixture(&destination, ExecutableFixture::Copy(source));
    assert_eq!(
        std::fs::read(source).unwrap(),
        std::fs::read(&destination).unwrap()
    );
    let original = std::fs::metadata(source).unwrap();
    let copied = std::fs::metadata(&destination).unwrap();
    assert_ne!(
        (original.dev(), original.ino()),
        (copied.dev(), copied.ino())
    );
    assert_ne!(copied.mode() & 0o111, 0);
    std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(std::fs::metadata(source).unwrap().mode(), original.mode());
}

#[cfg(target_os = "linux")]
#[test]
fn executable_fixture_bytes_preserve_exact_content_and_existing_mode() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("script with spaces and 'quotes'");
    let bytes = b"#!/bin/sh\n# literal $HOME and `id`\nexit 0\n";
    create_executable_fixture(&destination, ExecutableFixture::Bytes(bytes, 0o700));
    assert_eq!(std::fs::read(&destination).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(&destination)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert!(Command::new(&destination).status().unwrap().success());
    create_executable_fixture(&destination, ExecutableFixture::Bytes(b"", 0o644));
    assert!(std::fs::read(&destination).unwrap().is_empty());
    assert_eq!(
        std::fs::metadata(&destination)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[cfg(target_os = "linux")]
#[test]
fn executable_fixtures_run_during_parallel_forks() {
    use std::os::unix::process::CommandExt;
    use std::sync::{Arc, Barrier};

    let root = tempfile::tempdir().unwrap();
    let barrier = Arc::new(Barrier::new(12));
    std::thread::scope(|scope| {
        for worker in 0..12 {
            let barrier = Arc::clone(&barrier);
            let root = root.path();
            scope.spawn(move || {
                barrier.wait();
                for iteration in 0..32 {
                    if worker < 6 {
                        let destination = root.join(format!("fixture {worker} {iteration}"));
                        if worker < 3 {
                            create_executable_fixture(
                                &destination,
                                ExecutableFixture::Copy(Path::new("/bin/true")),
                            );
                        } else {
                            create_executable_fixture(
                                &destination,
                                ExecutableFixture::Bytes(b"#!/bin/sh\nexit 0\n", 0o700),
                            );
                        }
                        let status = Command::new(&destination).status().unwrap();
                        assert!(status.success());
                    } else {
                        let mut command = Command::new("/bin/true");
                        unsafe {
                            command.pre_exec(|| {
                                let delay = libc::timespec {
                                    tv_sec: 0,
                                    tv_nsec: 5_000_000,
                                };
                                // A parked fork retains any inherited writable descriptors.
                                libc::syscall(
                                    libc::SYS_nanosleep,
                                    &delay,
                                    std::ptr::null_mut::<libc::timespec>(),
                                );
                                Ok(())
                            });
                        }
                        assert!(command.status().unwrap().success());
                    }
                }
            });
        }
    });
}

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
    assert!(postrm.contains("action=finish-remove"));
    assert!(postrm.contains("action=finish-purge"));
    assert!(postrm.contains("--connect-package \"$action\""));
    for script in [preinst, postinst, prerm, postrm] {
        assert!(script.contains(env!("CARGO_PKG_VERSION")));
        assert!(!script.contains("systemctl"));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn debian_purge_dispatches_after_completed_removal() {
    use std::os::unix::fs::PermissionsExt;

    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("package-root");
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(root.join("package-removal-receipt-v1.json"), b"{}").unwrap();
    let invoked = temporary.path().join("invoked");
    let controller = root.join("package-controller-test");
    create_executable_fixture(
        &controller,
        ExecutableFixture::Bytes(
            (format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
                invoked.display()
            ))
            .as_ref(),
            0o700,
        ),
    );
    std::fs::set_permissions(&controller, std::fs::Permissions::from_mode(0o700)).unwrap();
    let hook = include_str!("../linux/debian/postrm")
        .replace("/var/lib/document-summarizer", root.to_str().unwrap())
        .replace(
            "-user root",
            &format!("-user {}", unsafe { libc::geteuid() }),
        );
    let script = temporary.path().join("postrm");
    create_executable_fixture(&script, ExecutableFixture::Bytes((hook).as_ref(), 0o644));
    assert!(Command::new("sh")
        .arg(&script)
        .arg("remove")
        .status()
        .unwrap()
        .success());
    assert!(
        !invoked.exists(),
        "completed ordinary removal keeps its reinstall receipt"
    );
    assert!(Command::new("sh")
        .arg(&script)
        .arg("purge")
        .status()
        .unwrap()
        .success());
    assert_eq!(
        std::fs::read_to_string(&invoked).unwrap_or_default(),
        "--connect-package\nfinish-purge\n0.1.0\n",
        "purge must reach the controller after the removal operation record is gone"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn debian_purge_retry_ignores_untrusted_inbox_but_validates_root_state() {
    use std::os::unix::fs::PermissionsExt;
    for case in [
        "empty",
        "inbox-file",
        "inbox-directory",
        "inbox-symlink",
        "receipt",
        "unknown",
        "participant-link",
        "root-link",
        "unsafe-lock",
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("package-root");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        let participants = root.join("participants-v1");
        std::fs::create_dir(&participants).unwrap();
        std::fs::set_permissions(&participants, std::fs::Permissions::from_mode(0o1777)).unwrap();
        let sentinel = temporary.path().join("sentinel");
        std::fs::write(&sentinel, b"preserve").unwrap();
        match case {
            "empty" => {}
            "inbox-file" => std::fs::write(participants.join("untrusted"), b"junk").unwrap(),
            "inbox-directory" => {
                let nested = participants.join("untrusted");
                std::fs::create_dir(&nested).unwrap();
                std::fs::write(nested.join("junk"), b"junk").unwrap();
            }
            "inbox-symlink" => {
                std::os::unix::fs::symlink(&sentinel, participants.join("untrusted")).unwrap();
            }
            "receipt" => {
                std::fs::write(root.join("package-removal-receipt-v1.json"), b"{}").unwrap()
            }
            "unknown" => std::fs::write(root.join("unrelated"), b"preserve").unwrap(),
            "participant-link" => {
                std::fs::remove_dir(&participants).unwrap();
                std::os::unix::fs::symlink(temporary.path(), &participants).unwrap();
            }
            "root-link" => {
                std::fs::rename(&root, temporary.path().join("saved")).unwrap();
                std::os::unix::fs::symlink(temporary.path().join("saved"), &root).unwrap();
            }
            "unsafe-lock" => std::fs::create_dir(root.join(".package-operation-v1.lock")).unwrap(),
            _ => unreachable!(),
        }
        let hook = include_str!("../linux/debian/postrm")
            .replace("/var/lib/document-summarizer", root.to_str().unwrap())
            .replace(
                "-user root",
                &format!("-user {}", unsafe { libc::geteuid() }),
            );
        let script = temporary.path().join("postrm");
        create_executable_fixture(&script, ExecutableFixture::Bytes((hook).as_ref(), 0o644));
        for _ in 0..2 {
            assert_eq!(
                Command::new("sh")
                    .arg(&script)
                    .arg("purge")
                    .status()
                    .unwrap()
                    .success(),
                case == "empty" || case.starts_with("inbox-"),
                "{case}"
            );
            assert_eq!(std::fs::read(&sentinel).unwrap(), b"preserve");
            if case.starts_with("inbox-") {
                assert!(std::fs::symlink_metadata(participants.join("untrusted")).is_ok());
            }
        }
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
    create_executable_fixture(
        &legacy,
        ExecutableFixture::Bytes(
            (format!("#!/bin/sh\ntouch '{}'\nexit 99\n", legacy_marker.display())).as_ref(),
            0o755,
        ),
    );
    std::fs::set_permissions(&legacy, std::fs::Permissions::from_mode(0o755)).unwrap();
    let runtime = temporary.path().join("run-user");
    let script = preinst_for_test(&package_root, &legacy, &runtime);
    let script_path = temporary.path().join("preinst");
    create_executable_fixture(
        &script_path,
        ExecutableFixture::Bytes((script).as_ref(), 0o644),
    );
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
    create_executable_fixture(&installed, ExecutableFixture::Copy(Path::new("/bin/true")));
    std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755)).unwrap();
    let runtime = temporary.path().join("run-user");
    std::fs::create_dir(&runtime).unwrap();
    let script = temporary.path().join("preinst");
    create_executable_fixture(
        &script,
        ExecutableFixture::Bytes(
            (preinst_for_test(&package_root, &installed, &runtime)).as_ref(),
            0o644,
        ),
    );
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
        create_executable_fixture(&installed, ExecutableFixture::Copy(Path::new("/bin/true")));
        std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755)).unwrap();
        let runtime = temporary.path().join("run-user");
        let normal = preinst_for_test(&package_root, &installed, &runtime);
        assert_eq!(normal.matches(boundary).count(), 1, "boundary {index}");
        let crashing = normal.replacen(boundary, &format!("{boundary}      exit 86\n"), 1);
        let script = temporary.path().join("preinst");
        create_executable_fixture(
            &script,
            ExecutableFixture::Bytes((crashing).as_ref(), 0o644),
        );
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
        create_executable_fixture(&script, ExecutableFixture::Bytes((normal).as_ref(), 0o644));
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
    create_executable_fixture(&installed, ExecutableFixture::Copy(Path::new("/bin/sleep")));
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
    create_executable_fixture(
        &script,
        ExecutableFixture::Bytes((crashing).as_ref(), 0o644),
    );
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let crashed = Command::new("sh")
        .arg(&script)
        .args(["upgrade", "0.0.9"])
        .status()
        .unwrap();
    assert_eq!(crashed.code(), Some(86));
    create_executable_fixture(&script, ExecutableFixture::Bytes((normal).as_ref(), 0o644));
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

#[cfg(target_os = "linux")]
#[test]
fn preinst_signals_pinned_process_instead_of_recycled_numeric_identity() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let installed = root.path().join("document-summarizer");
    create_executable_fixture(&installed, ExecutableFixture::Copy(Path::new("/bin/sleep")));
    std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut legacy = Command::new(&installed).arg("30").spawn().unwrap();
    let mut unrelated = Command::new("/bin/sleep").arg("30").spawn().unwrap();
    let normal = preinst_for_test(
        &root.path().join("package"),
        &installed,
        &root.path().join("runtime"),
    );
    // Model the kernel's numeric PID lookup after the checked process exits.
    // The numeric-signal double maps its recycled slot to a real unrelated child.
    // Descriptor signaling never consults this recycled numeric slot.
    let numeric_signal_lookup = format!(
        r#"
  kill() {{
    if [ "$2" = "{}" ]; then
      /bin/kill -KILL "$2" 2>/dev/null || true
      /bin/kill "$1" "{}"
    else
      /bin/kill "$@"
    fi
  }}
"#,
        legacy.id(),
        unrelated.id()
    );
    let script = root.path().join("preinst");
    create_executable_fixture(
        &script,
        ExecutableFixture::Bytes(
            (normal.replace(
                "  terminate_legacy_processes() {",
                &format!("{numeric_signal_lookup}  terminate_legacy_processes() {{"),
            ))
            .as_ref(),
            0o644,
        ),
    );
    let status = Command::new("sh")
        .arg(&script)
        .args(["upgrade", "0.0.9"])
        .status()
        .unwrap();
    let foreign_status = unrelated.try_wait().unwrap();
    let _ = legacy.kill();
    legacy.wait().unwrap();
    let _ = unrelated.kill();
    unrelated.wait().unwrap();
    eprintln!("upgrade={status} unrelated_exit={foreign_status:?}");
    assert!(status.success());
    assert!(
        foreign_status.is_none(),
        "numeric PID reuse signaled an unrelated process"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn preinst_handles_exit_after_capture_and_escalates_on_the_same_process_handle() {
    use std::os::unix::{fs::PermissionsExt, process::CommandExt};
    for exited_after_capture in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let installed = root.path().join("document-summarizer");
        create_executable_fixture(&installed, ExecutableFixture::Copy(Path::new("/bin/sleep")));
        std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut command = Command::new(&installed);
        command.arg("30");
        unsafe {
            command.pre_exec(|| {
                libc::signal(libc::SIGTERM, libc::SIG_IGN);
                Ok(())
            });
        }
        let mut child = command.spawn().unwrap();
        let mut normal = preinst_for_test(
            &root.path().join("package"),
            &installed,
            &root.path().join("runtime"),
        );
        if exited_after_capture {
            let marker = "# bootstrap-process-handles-captured";
            assert_eq!(normal.matches(marker).count(), 1);
            normal = normal.replace(
                marker,
                "signal_process($_, SIGKILL) for @handles;\nselect(undef, undef, undef, 0.1);",
            );
        }
        let script = root.path().join("preinst");
        create_executable_fixture(&script, ExecutableFixture::Bytes((normal).as_ref(), 0o644));
        let status = Command::new("sh")
            .arg(&script)
            .args(["upgrade", "0.0.9"])
            .status()
            .unwrap();
        let stopped = child.try_wait().unwrap().is_some();
        let _ = child.kill();
        child.wait().unwrap();
        assert!(
            status.success(),
            "exit after capture: {exited_after_capture}"
        );
        assert!(stopped);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn preinst_rejects_unavailable_process_handles_before_package_mutation() {
    let root = tempfile::tempdir().unwrap();
    let package = root.path().join("package");
    let script = root.path().join("preinst");
    let normal = preinst_for_test(
        &package,
        &root.path().join("binary"),
        &root.path().join("runtime"),
    );
    let probe = "my $machine = (uname())[4];";
    assert_eq!(normal.matches(probe).count(), 1);
    create_executable_fixture(
        &script,
        ExecutableFixture::Bytes(
            (normal.replace(probe, "my $machine = 'unsupported';")).as_ref(),
            0o644,
        ),
    );
    let output = Command::new("sh")
        .arg(script)
        .args(["upgrade", "0.0.9"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported pidfd ABI"));
    assert!(!package.exists());
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
        .find("state.package_installation.enter_job()")
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

#[cfg(target_os = "linux")]
#[test]
fn abort_upgrade_restores_bootstrap_at_every_preinstall_boundary() {
    use std::os::unix::fs::PermissionsExt;
    let boundaries = [
        "      install_wrapper\n",
        "    write_bootstrap quarantining\n",
        "        chmod 0600 \"$installed_binary\"\n",
        "          sync -f \"$quarantine_temporary\"\n",
        "        mv -T \"$quarantine_temporary\" \"$quarantine\"\n",
        "        rm -f \"$installed_binary\"\n        sync -f \"$(dirname \"$installed_binary\")\"\n",
        "      # bootstrap-crash-boundary-after-terminate\n",
        "      # bootstrap-crash-boundary-after-registration-cleanup\n",
        "      write_bootstrap quiesced\n",
    ];
    for (index, boundary) in boundaries.into_iter().enumerate() {
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("package");
        let installed = root.path().join("document-summarizer");
        let runtime = root.path().join("run-user");
        create_executable_fixture(&installed, ExecutableFixture::Copy(Path::new("/bin/true")));
        std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755)).unwrap();
        let original = std::fs::read(&installed).unwrap();
        let normal = preinst_for_test(&package, &installed, &runtime);
        assert_eq!(normal.matches(boundary).count(), 1);
        let preinst = root.path().join("preinst");
        create_executable_fixture(
            &preinst,
            ExecutableFixture::Bytes(
                (normal.replacen(boundary, &format!("{boundary}      exit 86\n"), 1)).as_ref(),
                0o644,
            ),
        );
        assert_eq!(
            std::process::Command::new("sh")
                .arg(&preinst)
                .args(["upgrade", "0.0.9"])
                .status()
                .unwrap()
                .code(),
            Some(86)
        );
        let postrm = root.path().join("postrm");
        let script = include_str!("../linux/debian/postrm")
            .replace("/var/lib/document-summarizer", package.to_str().unwrap())
            .replace("/usr/bin/document-summarizer", installed.to_str().unwrap())
            .replace(
                "expected_root_uid=0",
                &format!("expected_root_uid={}", unsafe { libc::geteuid() }),
            );
        create_executable_fixture(&postrm, ExecutableFixture::Bytes((script).as_ref(), 0o644));
        for _ in 0..2 {
            assert!(std::process::Command::new("sh")
                .arg(&postrm)
                .args(["abort-upgrade", "0.0.9", "0.1.0"])
                .status()
                .unwrap()
                .success());
            assert!(
                std::fs::read(&installed).unwrap() == original,
                "abort-upgrade left the exit-75 wrapper at boundary {index}"
            );
            assert_eq!(
                std::fs::metadata(&installed).unwrap().permissions().mode() & 0o777,
                0o755
            );
            assert!(
                !package.join("package-quiesce-v1.record").exists(),
                "rollback must clear only its settled barrier"
            );
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn abort_remove_dispatches_to_the_installed_transaction_controller() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let calls = root.path().join("calls");
    let installed = root.path().join("document-summarizer");
    create_executable_fixture(
        &installed,
        ExecutableFixture::Bytes(
            (format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n",
                calls.display()
            ))
            .as_ref(),
            0o755,
        ),
    );
    std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755)).unwrap();
    let postinst = root.path().join("postinst");
    create_executable_fixture(
        &postinst,
        ExecutableFixture::Bytes(
            (include_str!("../linux/debian/postinst")
                .replace("/usr/bin/document-summarizer", installed.to_str().unwrap()))
            .as_ref(),
            0o644,
        ),
    );
    assert!(std::process::Command::new("sh")
        .arg(&postinst)
        .arg("abort-remove")
        .status()
        .unwrap()
        .success());
    assert_eq!(
        std::fs::read_to_string(calls).unwrap_or_default(),
        "--connect-package abort-remove 0.1.0\n",
        "abort-remove returned success without invoking rollback"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn abort_upgrade_replays_rollback_crashes_and_rejects_tampered_state() {
    use std::os::unix::fs::PermissionsExt;
    for case in [
        "after-restore",
        "before-clear",
        "receipt",
        "source-version",
        "foreign-launcher",
        "symlink",
        "hardlink",
        "mode",
        "operation",
    ] {
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("package");
        let installed = root.path().join("document-summarizer");
        let runtime = root.path().join("run-user");
        create_executable_fixture(&installed, ExecutableFixture::Copy(Path::new("/bin/true")));
        std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755)).unwrap();
        let original = std::fs::read(&installed).unwrap();
        let preinst = root.path().join("preinst");
        create_executable_fixture(
            &preinst,
            ExecutableFixture::Bytes(
                (preinst_for_test(&package, &installed, &runtime)).as_ref(),
                0o644,
            ),
        );
        assert!(std::process::Command::new("sh")
            .arg(&preinst)
            .args(["upgrade", "0.0.9"])
            .status()
            .unwrap()
            .success());
        let receipt = package.join("package-quiesce-v1.record");
        let text = std::fs::read_to_string(&receipt).unwrap();
        let generation = text
            .lines()
            .find_map(|line| line.strip_prefix("generation="))
            .unwrap();
        let quarantine = package.join(format!("legacy-executable-{generation}"));
        match case {
            "receipt" => {
                std::fs::write(&receipt, text.replace("legacy_sha256=", "invalid_sha256=")).unwrap()
            }
            "foreign-launcher" => std::fs::write(&installed, b"foreign executable").unwrap(),
            "symlink" => {
                let target = root.path().join("foreign");
                std::fs::rename(&quarantine, &target).unwrap();
                std::os::unix::fs::symlink(target, &quarantine).unwrap();
            }
            "hardlink" => std::fs::hard_link(&quarantine, root.path().join("linked")).unwrap(),
            "mode" => std::fs::set_permissions(&quarantine, std::fs::Permissions::from_mode(0o644))
                .unwrap(),
            "operation" => {
                std::fs::write(package.join("package-operation-v1.json"), b"unsettled").unwrap()
            }
            _ => {}
        }
        let postrm = root.path().join("postrm");
        let normal = include_str!("../linux/debian/postrm")
            .replace("/var/lib/document-summarizer", package.to_str().unwrap())
            .replace("/usr/bin/document-summarizer", installed.to_str().unwrap())
            .replace(
                "expected_root_uid=0",
                &format!("expected_root_uid={}", unsafe { libc::geteuid() }),
            );
        let script = match case {
            "after-restore" => normal.replace(
                "  # bootstrap-rollback-crash-boundary-after-restore",
                "  exit 87",
            ),
            "before-clear" => normal.replace(
                "  # bootstrap-rollback-crash-boundary-before-clear",
                "  exit 87",
            ),
            _ => normal.clone(),
        };
        create_executable_fixture(&postrm, ExecutableFixture::Bytes((script).as_ref(), 0o644));
        let before = std::fs::read(&receipt).unwrap();
        let launcher_before = std::fs::read(&installed).unwrap();
        let source = if case == "source-version" {
            "9.9.9"
        } else {
            "0.0.9"
        };
        let status = std::process::Command::new("sh")
            .arg(&postrm)
            .args(["abort-upgrade", source, "0.1.0"])
            .status()
            .unwrap();
        assert!(!status.success(), "{case}");
        assert_eq!(std::fs::read(&receipt).unwrap(), before, "{case}");
        if matches!(case, "after-restore" | "before-clear") {
            assert_eq!(status.code(), Some(87));
            assert!(std::fs::read(&installed).unwrap() == original);
            create_executable_fixture(&postrm, ExecutableFixture::Bytes((normal).as_ref(), 0o644));
            assert!(std::process::Command::new("sh")
                .arg(&postrm)
                .args(["abort-upgrade", "0.0.9", "0.1.0"])
                .status()
                .unwrap()
                .success());
            assert!(!receipt.exists());
        } else {
            assert_eq!(
                std::fs::read(&installed).unwrap(),
                launcher_before,
                "{case}"
            );
        }
    }
}
