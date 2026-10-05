// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(not(debug_assertions), not(feature = "custom-protocol")))]
compile_error!(
    "release builds must use the Tauri build command so the frontend is embedded; run `npm run desktop:build:no-bundle` or `npm run desktop:build`"
);

fn main() {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let background_provider = arguments
        .first()
        .is_some_and(|argument| argument == "--connect-provider");
    #[cfg(target_os = "linux")]
    if arguments
        .first()
        .is_some_and(|argument| argument == "--connect-package-user-stop")
    {
        let result = arguments
            .get(1)
            .and_then(|value| value.to_str())
            .zip(arguments.get(2).and_then(|value| value.to_str()))
            .ok_or_else(|| "missing package participant paths".into())
            .and_then(|(app_data, runtime_root)| {
                document_summarizer_lib::run_package_user_stop(app_data, runtime_root)
            });
        match result {
            Ok(()) => return,
            Err(error) => {
                eprintln!("Document Summarizer package participant stop failed: {error}");
                std::process::exit(1);
            }
        }
    }
    #[cfg(target_os = "linux")]
    if arguments
        .first()
        .is_some_and(|argument| argument == "--connect-package")
    {
        let package_arguments = arguments
            .iter()
            .skip(1)
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        match document_summarizer_lib::run_package_control(&package_arguments) {
            Ok(()) => return,
            Err(error) => {
                eprintln!("Document Summarizer package control failed: {error}");
                std::process::exit(1);
            }
        }
    }
    #[cfg(target_os = "linux")]
    if arguments
        .first()
        .is_some_and(|argument| argument == "--connect-background")
    {
        let result = arguments
            .get(1)
            .and_then(|value| value.to_str())
            .ok_or_else(|| "missing Connect background control command".into())
            .and_then(document_summarizer_lib::run_background_control);
        match result {
            Ok(status) => {
                println!("{status}");
                return;
            }
            Err(error) => {
                eprintln!("Document Summarizer background control failed: {error}");
                std::process::exit(1);
            }
        }
    }
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
