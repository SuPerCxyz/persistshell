use std::process::Command;

#[test]
fn persistd_help_prints_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_persistd"))
        .arg("help")
        .output()
        .expect("run persistd help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    assert!(stdout.contains("Usage"));
}
