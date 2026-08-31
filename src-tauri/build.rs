use std::env;
use std::fs;
use std::path::PathBuf;

const KEYRING_ENV: &str = "LOCAL_CONNECT_ENTITLEMENT_KEYRING_FILE";
const MAX_KEYRING_BYTES: u64 = 64 * 1024;
const EMPTY_KEYRING: &str = r#"{"keys":[]}"#;

fn main() {
    println!("cargo:rerun-if-env-changed={KEYRING_ENV}");
    let keyring = match env::var_os(KEYRING_ENV) {
        Some(source) => {
            let source = PathBuf::from(source);
            println!("cargo:rerun-if-changed={}", source.display());
            let metadata = fs::metadata(&source).unwrap_or_else(|error| {
                panic!("Connect entitlement key ring is unavailable: {error}")
            });
            assert!(
                metadata.is_file() && metadata.len() <= MAX_KEYRING_BYTES,
                "Connect entitlement key ring must be a regular file no larger than {MAX_KEYRING_BYTES} bytes"
            );
            let value = fs::read_to_string(&source).unwrap_or_else(|error| {
                panic!("Connect entitlement key ring is unreadable: {error}")
            });
            let parsed: serde_json::Value = serde_json::from_str(&value).unwrap_or_else(|error| {
                panic!("Connect entitlement key ring is invalid JSON: {error}")
            });
            assert!(
                parsed.is_object(),
                "Connect entitlement key ring must be a JSON object"
            );
            value
        }
        None => EMPTY_KEYRING.to_string(),
    };
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must set OUT_DIR"))
        .join("connect-entitlement-keyring.json");
    fs::write(output, keyring).expect("Connect entitlement key ring could not be embedded");
    tauri_build::build()
}
