use persist_core::{PersistError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Help,
    Version,
    Doctor,
    Config,
    Daemon {
        action: Option<String>,
    },
    Attach {
        session_id: Option<u32>,
        readonly: bool,
    },
    New,
    List {
        session_id: Option<u32>,
        tag_filter: Option<String>,
        plain: bool,
    },
    HistoryAppend {
        session_id: u32,
        shell: String,
    },
    ProcessTree {
        session_id: u32,
    },
    ProcessStats {
        session_id: u32,
    },
    Snapshot {
        session_id: u32,
    },
    Metrics,
    Close {
        session_id: u32,
    },
    Kill {
        session_id: u32,
    },
    Log {
        session_id: u32,
    },
    LogSearch {
        keyword: String,
        session_id: Option<u32>,
        case_insensitive: bool,
    },
    LogExport {
        session_id: u32,
        output_path: Option<String>,
    },
    Rename {
        session_id: u32,
        name: String,
    },
    Note {
        session_id: u32,
        text: Option<String>,
    },
    Tag {
        session_id: u32,
        action: String,
        tag: Option<String>,
    },
    Detach {
        session_id: u32,
    },
    Install,
    Uninstall {
        purge: bool,
    },
    Pin {
        session_id: u32,
        pinned: bool,
    },
    Lock {
        session_id: u32,
        locked: bool,
    },
    Replay {
        session_id: u32,
        tail: Option<usize>,
        head: Option<usize>,
        speed: Option<f64>,
        follow: bool,
    },
}

pub fn parse_command(args: &[String]) -> Result<Command> {
    match args.first().map(String::as_str) {
        None | Some("-h" | "--help" | "help") => Ok(Command::Help),
        Some("-V" | "--version" | "version") => Ok(Command::Version),
        Some("doctor") => Ok(Command::Doctor),
        Some("config") => parse_config_command(&args[1..]),
        Some("daemon") => Ok(Command::Daemon {
            action: args.get(1).cloned(),
        }),
        Some("new") => Ok(Command::New),
        Some("__history-append") => {
            let session_id = args.get(1).and_then(|id| id.parse().ok()).ok_or_else(|| {
                PersistError::invalid_argument(
                    "usage: persist __history-append <session_id> <shell>",
                )
            })?;
            let shell = args.get(2).cloned().ok_or_else(|| {
                PersistError::invalid_argument(
                    "usage: persist __history-append <session_id> <shell>",
                )
            })?;
            if args.len() != 3 || !matches!(shell.as_str(), "bash" | "zsh" | "fish") {
                return Err(PersistError::invalid_argument(
                    "usage: persist __history-append <session_id> <bash|zsh|fish>",
                ));
            }
            Ok(Command::HistoryAppend { session_id, shell })
        }
        Some("ls") => parse_list_command(&args[1..]),
        Some("ps") => {
            let session_id = args
                .get(1)
                .and_then(|id| id.parse().ok())
                .ok_or_else(|| PersistError::invalid_argument("usage: persist ps <session_id>"))?;
            Ok(Command::ProcessTree { session_id })
        }
        Some("stats") => {
            let session_id = args.get(1).and_then(|id| id.parse().ok()).ok_or_else(|| {
                PersistError::invalid_argument("usage: persist stats <session_id>")
            })?;
            Ok(Command::ProcessStats { session_id })
        }
        Some("snapshot") => {
            let session_id = args.get(1).and_then(|id| id.parse().ok()).ok_or_else(|| {
                PersistError::invalid_argument("usage: persist snapshot <session_id>")
            })?;
            Ok(Command::Snapshot { session_id })
        }
        Some("metrics") => Ok(Command::Metrics),
        Some("attach") => {
            let mut id = None;
            let mut readonly = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "-r" | "--readonly" => readonly = true,
                    other => {
                        if id.is_none() {
                            id = other.parse::<u32>().ok();
                        } else {
                            return Err(PersistError::invalid_argument(format!(
                                "unexpected argument for attach: {other}"
                            )));
                        }
                    }
                }
                i += 1;
            }
            Ok(Command::Attach {
                session_id: id,
                readonly,
            })
        }
        Some("close") => {
            let id = args
                .get(1)
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| {
                    PersistError::invalid_argument("usage: persist close <session_id>")
                })?;
            Ok(Command::Close { session_id: id })
        }
        Some("kill") => {
            let id = args
                .get(1)
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| {
                    PersistError::invalid_argument("usage: persist kill <session_id>")
                })?;
            Ok(Command::Kill { session_id: id })
        }
        Some("log") => match args.get(1).map(String::as_str) {
            Some("export") => {
                let id = args
                    .get(2)
                    .and_then(|s| s.parse::<u32>().ok())
                    .ok_or_else(|| {
                        PersistError::invalid_argument(
                            "usage: persist log export <session_id> [--output <path>]",
                        )
                    })?;
                let rest: Vec<&str> = args[3..].iter().map(String::as_str).collect();
                let output_path = match rest.as_slice() {
                    ["--output", path] | ["-o", path] => Some(path.to_string()),
                    [] => None,
                    [other, ..] => {
                        return Err(PersistError::invalid_argument(format!(
                            "unexpected argument for log export: {other}"
                        )));
                    }
                };
                Ok(Command::LogExport {
                    session_id: id,
                    output_path,
                })
            }
            Some("search") => {
                let keyword = args.get(2).cloned().ok_or_else(|| {
                    PersistError::invalid_argument(
                        "usage: persist log search <keyword> [--session <id>] [-i]",
                    )
                })?;
                let rest: Vec<&str> = args[3..].iter().map(String::as_str).collect();
                let mut session_id = None;
                let mut case_insensitive = false;
                let mut i = 0;
                while i < rest.len() {
                    match rest[i] {
                        "--session" | "-s" => {
                            let val = rest.get(i + 1).ok_or_else(|| {
                                PersistError::invalid_argument("--session requires a value")
                            })?;
                            session_id = Some(val.parse::<u32>().map_err(|_| {
                                PersistError::invalid_argument(format!("invalid session_id: {val}"))
                            })?);
                            i += 2;
                        }
                        "-i" | "--ignore-case" => {
                            case_insensitive = true;
                            i += 1;
                        }
                        other => {
                            return Err(PersistError::invalid_argument(format!(
                                "unexpected argument for log search: {other}"
                            )));
                        }
                    }
                }
                Ok(Command::LogSearch {
                    keyword,
                    session_id,
                    case_insensitive,
                })
            }
            _ => {
                let id = args
                    .get(1)
                    .and_then(|s| s.parse::<u32>().ok())
                    .ok_or_else(|| {
                        PersistError::invalid_argument("usage: persist log <session_id>")
                    })?;
                Ok(Command::Log { session_id: id })
            }
        },
        Some("replay") => {
            let id = args
                .get(1)
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| {
                    PersistError::invalid_argument("usage: persist replay <session_id> [--tail <n>] [--head <n>] [--speed <f>] [--follow]")
                })?;
            let rest: Vec<&str> = args[2..].iter().map(String::as_str).collect();
            let mut tail: Option<usize> = None;
            let mut head: Option<usize> = None;
            let mut speed: Option<f64> = None;
            let mut follow = false;
            let mut i = 0;
            while i < rest.len() {
                match rest[i] {
                    "--tail" | "-t" => {
                        let val = rest.get(i + 1).ok_or_else(|| {
                            PersistError::invalid_argument("--tail requires a number")
                        })?;
                        tail = Some(val.parse::<usize>().map_err(|_| {
                            PersistError::invalid_argument(format!("invalid number: {val}"))
                        })?);
                        i += 2;
                    }
                    "--head" | "-h" => {
                        let val = rest.get(i + 1).ok_or_else(|| {
                            PersistError::invalid_argument("--head requires a number")
                        })?;
                        head = Some(val.parse::<usize>().map_err(|_| {
                            PersistError::invalid_argument(format!("invalid number: {val}"))
                        })?);
                        i += 2;
                    }
                    "--speed" | "-s" => {
                        let val = rest.get(i + 1).ok_or_else(|| {
                            PersistError::invalid_argument("--speed requires a factor")
                        })?;
                        speed = Some(val.parse::<f64>().map_err(|_| {
                            PersistError::invalid_argument(format!("invalid speed factor: {val}"))
                        })?);
                        i += 2;
                    }
                    "--follow" | "-f" => {
                        follow = true;
                        i += 1;
                    }
                    other => {
                        return Err(PersistError::invalid_argument(format!(
                            "unexpected argument for replay: {other}"
                        )));
                    }
                }
            }
            Ok(Command::Replay {
                session_id: id,
                tail,
                head,
                speed,
                follow,
            })
        }
        Some("rename") => {
            let (id, name) = match (args.get(1), args.get(2)) {
                (Some(id_str), Some(name)) => {
                    let id = id_str.parse::<u32>().map_err(|_| {
                        PersistError::invalid_argument(format!("invalid session_id: {id_str}"))
                    })?;
                    (id, name.clone())
                }
                _ => {
                    return Err(PersistError::invalid_argument(
                        "usage: persist rename <session_id> <name>",
                    ));
                }
            };
            Ok(Command::Rename {
                session_id: id,
                name,
            })
        }
        Some("install") => Ok(Command::Install),
        Some("uninstall") => {
            let purge = args.get(1).map(String::as_str) == Some("--purge");
            Ok(Command::Uninstall { purge })
        }
        Some("detach") => {
            let id = args
                .get(1)
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| {
                    PersistError::invalid_argument("usage: persist detach <session_id>")
                })?;
            Ok(Command::Detach { session_id: id })
        }
        Some("pin") => {
            let id = args
                .get(1)
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| PersistError::invalid_argument("usage: persist pin <session_id>"))?;
            Ok(Command::Pin {
                session_id: id,
                pinned: true,
            })
        }
        Some("unpin") => {
            let id = args
                .get(1)
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| {
                    PersistError::invalid_argument("usage: persist unpin <session_id>")
                })?;
            Ok(Command::Pin {
                session_id: id,
                pinned: false,
            })
        }
        Some("lock") | Some("unlock") => {
            let id = args
                .get(1)
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| {
                    PersistError::invalid_argument("usage: persist lock|unlock <session_id>")
                })?;
            Ok(Command::Lock {
                session_id: id,
                locked: args[0] == "lock",
            })
        }
        Some("note") => {
            let id = args
                .get(1)
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| {
                    PersistError::invalid_argument("usage: persist note <session_id> [<text>]")
                })?;
            let text = args.get(2).cloned();
            Ok(Command::Note {
                session_id: id,
                text,
            })
        }
        Some("tag") => {
            let id = args
                .get(1)
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| {
                    PersistError::invalid_argument(
                        "usage: persist tag <session_id> <add|remove|list> [<tag>]",
                    )
                })?;
            let action = args.get(2).cloned().ok_or_else(|| {
                PersistError::invalid_argument(
                    "usage: persist tag <session_id> <add|remove|list> [<tag>]",
                )
            })?;
            let tag = args.get(3).cloned();
            if action == "add" || action == "remove" {
                if tag.is_none() {
                    return Err(PersistError::invalid_argument(format!(
                        "usage: persist tag <session_id> {action} <tag>"
                    )));
                }
            } else if action != "list" {
                return Err(PersistError::invalid_argument(format!(
                    "unknown tag action: {action}"
                )));
            }
            Ok(Command::Tag {
                session_id: id,
                action,
                tag,
            })
        }
        Some(other) => Err(PersistError::invalid_argument(format!(
            "unknown persist command: {other}"
        ))),
    }
}

fn parse_list_command(args: &[String]) -> Result<Command> {
    let mut session_id = None;
    let mut tag_filter = None;
    let mut plain = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--plain" => {
                plain = true;
                index += 1;
            }
            "--tag" | "-t" => {
                let tag = args
                    .get(index + 1)
                    .ok_or_else(|| PersistError::invalid_argument("--tag requires a value"))?;
                tag_filter = Some(tag.clone());
                index += 2;
            }
            value if session_id.is_none() => {
                session_id = Some(value.parse::<u32>().map_err(|_| {
                    PersistError::invalid_argument(format!("invalid session_id: {value}"))
                })?);
                index += 1;
            }
            other => {
                return Err(PersistError::invalid_argument(format!(
                    "unexpected argument for ls: {other}"
                )));
            }
        }
    }
    if session_id.is_some() && (tag_filter.is_some() || plain) {
        return Err(PersistError::invalid_argument(
            "persist ls <id> cannot be combined with --tag or --plain",
        ));
    }
    Ok(Command::List {
        session_id,
        tag_filter,
        plain,
    })
}

fn parse_config_command(args: &[String]) -> Result<Command> {
    match args {
        [] => Ok(Command::Config),
        [action] if action == "show" => Ok(Command::Config),
        [action, extra, ..] if action == "show" => Err(PersistError::invalid_argument(format!(
            "unexpected argument for persist config show: {extra}"
        ))),
        [action, ..] => Err(PersistError::invalid_argument(format!(
            "unknown persist config command: {action}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_as_help() {
        assert_eq!(parse_command(&[]).expect("parse"), Command::Help);
    }

    #[test]
    fn parses_version_alias() {
        let args = vec!["--version".to_string()];
        assert_eq!(parse_command(&args).expect("parse"), Command::Version);
    }

    #[test]
    fn parses_config_show() {
        let args = vec!["config".to_string(), "show".to_string()];
        assert_eq!(parse_command(&args).expect("parse"), Command::Config);
    }

    #[test]
    fn rejects_unknown_config_action() {
        let args = vec!["config".to_string(), "set".to_string()];
        assert!(parse_command(&args).is_err());
    }

    #[test]
    fn parses_new() {
        assert_eq!(
            parse_command(&["new".to_string()]).expect("parse"),
            Command::New
        );
    }

    #[test]
    fn parses_ls() {
        assert_eq!(
            parse_command(&["ls".to_string()]).expect("parse"),
            Command::List {
                session_id: None,
                tag_filter: None,
                plain: false,
            }
        );
    }

    #[test]
    fn parses_hidden_history_append() {
        assert_eq!(
            parse_command(&[
                "__history-append".to_string(),
                "42".to_string(),
                "bash".to_string(),
            ])
            .expect("parse"),
            Command::HistoryAppend {
                session_id: 42,
                shell: "bash".to_string(),
            }
        );
    }

    #[test]
    fn rejects_unknown_history_shell() {
        assert!(parse_command(&[
            "__history-append".to_string(),
            "42".to_string(),
            "ksh".to_string(),
        ])
        .is_err());
    }

    #[test]
    fn parses_ls_with_tag() {
        assert_eq!(
            parse_command(&["ls".to_string(), "--tag".to_string(), "work".to_string()])
                .expect("parse"),
            Command::List {
                session_id: None,
                tag_filter: Some("work".to_string()),
                plain: false,
            }
        );
    }

    #[test]
    fn parses_ls_with_t_short() {
        assert_eq!(
            parse_command(&["ls".to_string(), "-t".to_string(), "work".to_string()])
                .expect("parse"),
            Command::List {
                session_id: None,
                tag_filter: Some("work".to_string()),
                plain: false,
            }
        );
    }

    #[test]
    fn parses_ls_with_session_id() {
        assert_eq!(
            parse_command(&["ls".to_string(), "12".to_string()]).expect("parse"),
            Command::List {
                session_id: Some(12),
                tag_filter: None,
                plain: false,
            }
        );
    }

    #[test]
    fn parses_plain_tagged_ls() {
        assert_eq!(
            parse_command(&[
                "ls".to_string(),
                "--plain".to_string(),
                "--tag".to_string(),
                "work".to_string(),
            ])
            .expect("parse"),
            Command::List {
                session_id: None,
                tag_filter: Some("work".to_string()),
                plain: true,
            }
        );
    }

    #[test]
    fn rejects_ls_id_with_plain() {
        assert!(
            parse_command(&["ls".to_string(), "12".to_string(), "--plain".to_string(),]).is_err()
        );
    }

    #[test]
    fn parses_snapshot_with_id() {
        assert_eq!(
            parse_command(&["snapshot".to_string(), "42".to_string()]).expect("parse"),
            Command::Snapshot { session_id: 42 }
        );
    }

    #[test]
    fn rejects_snapshot_without_id() {
        assert!(parse_command(&["snapshot".to_string()]).is_err());
    }

    #[test]
    fn parses_metrics() {
        assert_eq!(
            parse_command(&["metrics".to_string()]).expect("parse"),
            Command::Metrics
        );
    }

    #[test]
    fn parses_close_with_id() {
        assert_eq!(
            parse_command(&["close".to_string(), "42".to_string()]).expect("parse"),
            Command::Close { session_id: 42 }
        );
    }

    #[test]
    fn rejects_close_without_id() {
        assert!(parse_command(&["close".to_string()]).is_err());
    }

    #[test]
    fn parses_kill_with_id() {
        assert_eq!(
            parse_command(&["kill".to_string(), "7".to_string()]).expect("parse"),
            Command::Kill { session_id: 7 }
        );
    }

    #[test]
    fn rejects_kill_without_id() {
        assert!(parse_command(&["kill".to_string()]).is_err());
    }

    #[test]
    fn parses_attach() {
        assert_eq!(
            parse_command(&["attach".to_string()]).expect("parse"),
            Command::Attach {
                session_id: None,
                readonly: false,
            }
        );
    }

    #[test]
    fn parses_attach_with_id() {
        assert_eq!(
            parse_command(&["attach".to_string(), "7".to_string()]).expect("parse"),
            Command::Attach {
                session_id: Some(7),
                readonly: false,
            }
        );
    }

    #[test]
    fn parses_attach_readonly() {
        assert_eq!(
            parse_command(&[
                "attach".to_string(),
                "--readonly".to_string(),
                "7".to_string()
            ])
            .expect("parse"),
            Command::Attach {
                session_id: Some(7),
                readonly: true,
            }
        );
    }

    #[test]
    fn parses_attach_readonly_short() {
        assert_eq!(
            parse_command(&["attach".to_string(), "-r".to_string(), "7".to_string()])
                .expect("parse"),
            Command::Attach {
                session_id: Some(7),
                readonly: true,
            }
        );
        assert_eq!(
            parse_command(&["attach".to_string(), "7".to_string(), "-r".to_string()])
                .expect("parse"),
            Command::Attach {
                session_id: Some(7),
                readonly: true,
            }
        );
    }

    #[test]
    fn parses_rename() {
        assert_eq!(
            parse_command(&[
                "rename".to_string(),
                "1".to_string(),
                "my-shell".to_string()
            ])
            .expect("parse"),
            Command::Rename {
                session_id: 1,
                name: "my-shell".to_string()
            }
        );
    }

    #[test]
    fn parses_detach_with_id() {
        assert_eq!(
            parse_command(&["detach".to_string(), "42".to_string()]).expect("parse"),
            Command::Detach { session_id: 42 }
        );
    }

    #[test]
    fn rejects_detach_without_id() {
        assert!(parse_command(&["detach".to_string()]).is_err());
    }

    #[test]
    fn parses_note_get() {
        assert_eq!(
            parse_command(&["note".to_string(), "1".to_string()]).expect("parse"),
            Command::Note {
                session_id: 1,
                text: None
            }
        );
    }

    #[test]
    fn parses_note_set() {
        assert_eq!(
            parse_command(&[
                "note".to_string(),
                "1".to_string(),
                "hello world".to_string()
            ])
            .expect("parse"),
            Command::Note {
                session_id: 1,
                text: Some("hello world".to_string())
            }
        );
    }

    #[test]
    fn rejects_note_without_id() {
        assert!(parse_command(&["note".to_string()]).is_err());
    }

    #[test]
    fn parses_tag_add() {
        assert_eq!(
            parse_command(&[
                "tag".to_string(),
                "1".to_string(),
                "add".to_string(),
                "work".to_string()
            ])
            .expect("parse"),
            Command::Tag {
                session_id: 1,
                action: "add".to_string(),
                tag: Some("work".to_string())
            }
        );
    }

    #[test]
    fn parses_tag_remove() {
        assert_eq!(
            parse_command(&[
                "tag".to_string(),
                "1".to_string(),
                "remove".to_string(),
                "work".to_string()
            ])
            .expect("parse"),
            Command::Tag {
                session_id: 1,
                action: "remove".to_string(),
                tag: Some("work".to_string())
            }
        );
    }

    #[test]
    fn parses_tag_list() {
        assert_eq!(
            parse_command(&["tag".to_string(), "1".to_string(), "list".to_string()])
                .expect("parse"),
            Command::Tag {
                session_id: 1,
                action: "list".to_string(),
                tag: None
            }
        );
    }

    #[test]
    fn rejects_tag_without_id() {
        assert!(parse_command(&["tag".to_string()]).is_err());
    }

    #[test]
    fn rejects_tag_add_without_tag() {
        assert!(parse_command(&["tag".to_string(), "1".to_string(), "add".to_string()]).is_err());
    }

    #[test]
    fn rejects_tag_unknown_action() {
        assert!(parse_command(&["tag".to_string(), "1".to_string(), "wat".to_string()]).is_err());
    }

    #[test]
    fn parses_pin_with_id() {
        assert_eq!(
            parse_command(&["pin".to_string(), "42".to_string()]).expect("parse"),
            Command::Pin {
                session_id: 42,
                pinned: true,
            }
        );
    }

    #[test]
    fn parses_unpin_with_id() {
        assert_eq!(
            parse_command(&["unpin".to_string(), "7".to_string()]).expect("parse"),
            Command::Pin {
                session_id: 7,
                pinned: false,
            }
        );
    }

    #[test]
    fn rejects_pin_without_id() {
        assert!(parse_command(&["pin".to_string()]).is_err());
    }

    #[test]
    fn rejects_unpin_without_id() {
        assert!(parse_command(&["unpin".to_string()]).is_err());
    }

    #[test]
    fn parses_lock_with_id() {
        assert_eq!(
            parse_command(&["lock".to_string(), "42".to_string()]).expect("parse"),
            Command::Lock {
                session_id: 42,
                locked: true,
            }
        );
    }

    #[test]
    fn parses_process_tree_with_id() {
        assert_eq!(
            parse_command(&["ps".to_string(), "42".to_string()]).expect("parse"),
            Command::ProcessTree { session_id: 42 }
        );
    }

    #[test]
    fn rejects_process_tree_without_id() {
        assert!(parse_command(&["ps".to_string()]).is_err());
    }

    #[test]
    fn parses_unlock_with_id() {
        assert_eq!(
            parse_command(&["unlock".to_string(), "7".to_string()]).expect("parse"),
            Command::Lock {
                session_id: 7,
                locked: false,
            }
        );
    }

    #[test]
    fn rejects_lock_without_id() {
        assert!(parse_command(&["lock".to_string()]).is_err());
    }

    #[test]
    fn rejects_unlock_without_id() {
        assert!(parse_command(&["unlock".to_string()]).is_err());
    }

    #[test]
    fn parses_log_search() {
        assert_eq!(
            parse_command(&["log".to_string(), "search".to_string(), "foo".to_string()])
                .expect("parse"),
            Command::LogSearch {
                keyword: "foo".to_string(),
                session_id: None,
                case_insensitive: false,
            }
        );
    }

    #[test]
    fn parses_log_search_with_session() {
        assert_eq!(
            parse_command(&[
                "log".to_string(),
                "search".to_string(),
                "bar".to_string(),
                "--session".to_string(),
                "3".to_string(),
            ])
            .expect("parse"),
            Command::LogSearch {
                keyword: "bar".to_string(),
                session_id: Some(3),
                case_insensitive: false,
            }
        );
    }

    #[test]
    fn parses_log_search_with_ignore_case() {
        assert_eq!(
            parse_command(&[
                "log".to_string(),
                "search".to_string(),
                "baz".to_string(),
                "-i".to_string(),
            ])
            .expect("parse"),
            Command::LogSearch {
                keyword: "baz".to_string(),
                session_id: None,
                case_insensitive: true,
            }
        );
    }

    #[test]
    fn parses_log_search_with_short_session() {
        assert_eq!(
            parse_command(&[
                "log".to_string(),
                "search".to_string(),
                "qux".to_string(),
                "-s".to_string(),
                "5".to_string(),
            ])
            .expect("parse"),
            Command::LogSearch {
                keyword: "qux".to_string(),
                session_id: Some(5),
                case_insensitive: false,
            }
        );
    }

    #[test]
    fn rejects_log_search_without_keyword() {
        assert!(parse_command(&["log".to_string(), "search".to_string()]).is_err());
    }

    #[test]
    fn parses_replay_with_id() {
        let cmd = parse_command(&["replay".to_string(), "5".to_string()]).expect("parse");
        assert_eq!(
            cmd,
            Command::Replay {
                session_id: 5,
                tail: None,
                head: None,
                speed: None,
                follow: false,
            }
        );
    }

    #[test]
    fn parses_replay_with_tail() {
        let cmd = parse_command(&[
            "replay".to_string(),
            "5".to_string(),
            "--tail".to_string(),
            "10".to_string(),
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::Replay {
                session_id: 5,
                tail: Some(10),
                head: None,
                speed: None,
                follow: false,
            }
        );
    }

    #[test]
    fn parses_replay_with_head() {
        let cmd = parse_command(&[
            "replay".to_string(),
            "5".to_string(),
            "--head".to_string(),
            "3".to_string(),
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::Replay {
                session_id: 5,
                tail: None,
                head: Some(3),
                speed: None,
                follow: false,
            }
        );
    }

    #[test]
    fn parses_replay_with_follow() {
        let cmd = parse_command(&[
            "replay".to_string(),
            "7".to_string(),
            "--follow".to_string(),
        ])
        .expect("parse");
        assert_eq!(
            cmd,
            Command::Replay {
                session_id: 7,
                tail: None,
                head: None,
                speed: None,
                follow: true,
            }
        );
    }

    #[test]
    fn rejects_replay_without_id() {
        assert!(parse_command(&["replay".to_string()]).is_err());
    }
}
