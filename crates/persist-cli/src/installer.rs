use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use persist_core::{Config, PersistError, Result};

const HOOK_MARKER_START: &str = "# === PERSISTSHELL AUTO-HOOK ===";
const HOOK_MARKER_END: &str = "# === /PERSISTSHELL AUTO-HOOK ===";

const HOOK_SCRIPT: &str = r#"# === PERSISTSHELL AUTO-HOOK ===
if [ -n "$SSH_TTY" ] && [ -z "${PERSIST_DISABLE:-}" ] && command -v persist >/dev/null 2>&1; then
    persist daemon start >/dev/null 2>&1 || true
    persist attach 2>/dev/null
fi
# === /PERSISTSHELL AUTO-HOOK ===
"#;

pub fn install(_config: &Config) -> Result<()> {
    let profile_path = detect_profile()?;
    ensure_no_hook(&profile_path)?;
    append_hook(&profile_path)?;
    println!("installed persist hook in {}", profile_path.display());
    println!("next SSH login will auto-attach to a new PersistShell session");
    if let Some(home) = home_dir() {
        let bashrc = home.join(".bashrc");
        if bashrc.exists() && profile_path != bashrc {
            println!("note: {} is your active profile", profile_path.display());
        }
    }
    Ok(())
}

pub fn uninstall(config: &Config, purge: bool) -> Result<()> {
    let profile_path = detect_profile()?;
    let removed = remove_hook(&profile_path)?;
    if removed {
        println!("removed persist hook from {}", profile_path.display());
    } else {
        println!("no persist hook found in {}", profile_path.display());
    }
    if purge {
        purge_all(config);
    }
    Ok(())
}

fn detect_profile() -> Result<PathBuf> {
    let shell = std::env::var("SHELL").ok();
    let home = home_dir();
    detect_profile_for(shell.as_deref(), home.as_deref())
}

fn detect_profile_for(shell: Option<&str>, home: Option<&Path>) -> Result<PathBuf> {
    if let Some(shell) = shell {
        let home =
            home.ok_or_else(|| PersistError::invalid_argument("cannot determine home directory"))?;

        if shell.ends_with("/zsh") {
            let zshrc = home.join(".zshrc");
            if zshrc.exists() {
                return Ok(zshrc);
            }
            // Create .zshrc if it doesn't exist
            if !zshrc.exists() {
                fs::write(&zshrc, "").map_err(|e| PersistError::Io {
                    operation: "create .zshrc",
                    source: e,
                })?;
            }
            return Ok(zshrc);
        }

        if shell.ends_with("/bash") {
            let bashrc = home.join(".bashrc");
            if !bashrc.exists() {
                let profile = home.join(".bash_profile");
                if profile.exists() {
                    return Ok(profile);
                }
            }
            if !bashrc.exists() {
                fs::write(&bashrc, "").map_err(|e| PersistError::Io {
                    operation: "create .bashrc",
                    source: e,
                })?;
            }
            return Ok(bashrc);
        }
    }

    Err(PersistError::invalid_argument(
        "unsupported shell: only bash and zsh are supported",
    ))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn ensure_no_hook(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(path).map_err(|e| PersistError::Io {
        operation: "read profile",
        source: e,
    })?;
    if content.contains(HOOK_MARKER_START) {
        return Err(PersistError::invalid_argument(
            "persist hook already installed",
        ));
    }
    Ok(())
}

fn append_hook(path: &Path) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| PersistError::Io {
            operation: "open profile for append",
            source: e,
        })?;

    writeln!(file).map_err(|e| PersistError::Io {
        operation: "write newline to profile",
        source: e,
    })?;
    file.write_all(HOOK_SCRIPT.as_bytes())
        .map_err(|e| PersistError::Io {
            operation: "write hook to profile",
            source: e,
        })?;
    Ok(())
}

fn remove_hook(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(path).map_err(|e| PersistError::Io {
        operation: "read profile",
        source: e,
    })?;

    let start = content.find(HOOK_MARKER_START);
    let end = content.find(HOOK_MARKER_END);

    match (start, end) {
        (Some(s), Some(e)) => {
            let e = e + HOOK_MARKER_END.len();
            // Remove the hook including surrounding whitespace
            let before = &content[..s].trim_end();
            let after = &content[e..].trim_start();
            let new_content = if before.is_empty() && after.is_empty() {
                String::new()
            } else if before.is_empty() {
                after.to_string()
            } else if after.is_empty() {
                before.to_string()
            } else {
                format!("{before}\n{after}")
            };
            fs::write(path, new_content).map_err(|e| PersistError::Io {
                operation: "write profile",
                source: e,
            })?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn purge_all(config: &Config) {
    let dirs = [
        config.paths.config_dir.as_path(),
        config.paths.data_dir.as_path(),
        config.paths.state_dir.as_path(),
    ];
    for dir in &dirs {
        if dir.exists() && fs::remove_dir_all(dir).is_ok() {
            println!("removed {}", dir.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detect_profile_returns_bashrc() {
        let home =
            std::env::temp_dir().join(format!("persist_test_profile_{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();
        let result = detect_profile_for(Some("/bin/bash"), Some(&home)).unwrap();
        assert_eq!(result, home.join(".bashrc"));
        assert!(result.exists());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn ensure_no_hook_rejects_existing() {
        let dir = std::env::temp_dir().join("persist_test_installer");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join(".bashrc");
        fs::write(&path, HOOK_SCRIPT).unwrap();
        let result = ensure_no_hook(&path);
        assert!(result.is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_and_remove_hook_roundtrip() {
        let dir = std::env::temp_dir().join("persist_test_roundtrip");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join(".bashrc");
        fs::write(&path, "# existing config\n").unwrap();

        append_hook(&path).unwrap();
        let after_append = fs::read_to_string(&path).unwrap();
        assert!(after_append.contains(HOOK_MARKER_START));

        let removed = remove_hook(&path).unwrap();
        assert!(removed);
        let after_remove = fs::read_to_string(&path).unwrap();
        assert!(!after_remove.contains(HOOK_MARKER_START));
        assert!(after_remove.contains("# existing config"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_hook_returns_false_when_missing() {
        let dir = std::env::temp_dir().join("persist_test_nohook");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join(".bashrc");
        fs::write(&path, "# just config\n").unwrap();
        assert!(!remove_hook(&path).unwrap());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hook_script_contains_ssh_tty_check() {
        assert!(HOOK_SCRIPT.contains("SSH_TTY"));
        assert!(HOOK_SCRIPT.contains("PERSIST_DISABLE"));
        assert!(HOOK_SCRIPT.contains("persist daemon start"));
        assert!(HOOK_SCRIPT.contains("persist attach"));
    }
}
