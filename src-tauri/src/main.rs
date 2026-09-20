// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(not(debug_assertions), not(feature = "custom-protocol")))]
compile_error!(
    "release builds must use the Tauri build command so the frontend is embedded; run `npm run desktop:build:no-bundle` or `npm run desktop:build`"
);

fn main() {
    let background_provider = std::env::args_os()
        .nth(1)
        .is_some_and(|argument| argument == "--connect-provider");
    #[cfg(target_os = "linux")]
    let result = if background_provider {
        document_summarizer_lib::run_background_connect_provider()
    } else {
        document_summarizer_lib::run()
    };
    #[cfg(not(target_os = "linux"))]
    let result = if background_provider {
        Err("The Connect background provider is currently supported only on Linux".into())
    } else {
        document_summarizer_lib::run()
    };
    if let Err(error) = result {
        eprintln!("Document Summarizer failed to start: {error}");
        std::process::exit(1);
    }
}
