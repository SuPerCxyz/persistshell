use std::process::Command;

#[test]
fn version_command_prints_binary_name() {
    let output = Command::new(env!("CARGO_BIN_EXE_persist"))
        .arg("--version")
        .output()
        .expect("run persist --version");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    assert!(stdout.starts_with("persist "));
}

#[test]
fn help_command_prints_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_persist"))
        .arg("help")
        .output()
        .expect("run persist help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    assert!(stdout.contains("Usage"));
}
