// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(not(debug_assertions), not(feature = "custom-protocol")))]
compile_error!(
    "release builds must use the Tauri build command so the frontend is embedded; run `npm run desktop:build:no-bundle` or `npm run desktop:build`"
);

fn main() {
    if let Err(error) = tauri_appdoc_sum_lib::run() {
        eprintln!("Document Summarizer failed to start: {error}");
        std::process::exit(1);
    }
}
