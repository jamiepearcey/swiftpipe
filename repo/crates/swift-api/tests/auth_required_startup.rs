use std::process::Command;

#[test]
fn auth_required_exits_config_error_without_token() {
    let output = Command::new(env!("CARGO_BIN_EXE_swiftpipe-api"))
        .arg("--auth-required")
        .env_remove("SWIFTPIPE_AUTH_REQUIRED")
        .env_remove("SWIFTPIPE_AUTH_TOKEN")
        .output()
        .expect("run swiftpipe-api");

    assert_eq!(output.status.code(), Some(78));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("SWIFTPIPE_AUTH_TOKEN"),
        "stderr should name SWIFTPIPE_AUTH_TOKEN, got: {stderr}"
    );
}
