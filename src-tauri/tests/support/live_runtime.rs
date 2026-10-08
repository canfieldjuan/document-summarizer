use crate::pipeline::contracts::ModelRuntime;
use crate::pipeline::model_settings::{
    register_gguf, runtime_from_settings, settings_path, QwenProfileRuntime,
};
use std::{env, path::Path};

pub fn configured_live_runtime(
    db_path: &Path,
) -> (Box<dyn ModelRuntime>, Option<tempfile::TempDir>) {
    if let Some(settings_path) = env::var_os("DOC_SUM_MODEL_SETTINGS_PATH") {
        return (
            runtime_from_settings(Path::new(&settings_path), db_path)
                .expect("selected product model settings should configure"),
            None,
        );
    }
    if let Ok(analysis_model) = env::var("DOC_SUM_QUALIFICATION_ANALYSIS_MODEL") {
        let verification_model = env::var("DOC_SUM_QUALIFICATION_VERIFICATION_MODEL")
            .unwrap_or_else(|_| analysis_model.clone());
        let context_tokens = env::var("DOC_SUM_QUALIFICATION_CONTEXT_TOKENS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(8_192);
        return (
            Box::new(
                QwenProfileRuntime::qualification_candidate(
                    &analysis_model,
                    &verification_model,
                    context_tokens,
                )
                .expect("qualification stage models should configure"),
            ),
            None,
        );
    }
    let model = env::var_os("DOC_SUM_QUALIFICATION_GGUF").expect(
        "default live acceptance requires DOC_SUM_QUALIFICATION_GGUF or explicit model settings",
    );
    let directory = isolated_runtime_directory();
    let path = settings_path(directory.path());
    register_gguf(&path, Path::new(&model)).expect("default GGUF should register");
    let runtime =
        runtime_from_settings(&path, db_path).expect("default model profile should configure");
    // Keep the private socket directory alive until the acceptance run ends.
    (runtime, Some(directory))
}

fn isolated_runtime_directory() -> tempfile::TempDir {
    let mut builder = tempfile::Builder::new();
    builder.prefix("docsum-accept-");
    #[cfg(unix)]
    {
        use std::{fs, os::unix::fs::PermissionsExt};
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    builder
        .tempdir()
        .expect("isolated model settings directory")
}

#[test]
#[cfg(unix)]
fn isolated_runtime_directory_is_owner_only() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let directory = isolated_runtime_directory();
    assert_eq!(
        fs::metadata(directory.path()).unwrap().permissions().mode() & 0o777,
        0o700
    );
}
