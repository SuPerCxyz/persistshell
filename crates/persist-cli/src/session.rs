use std::io::{BufRead, Write};

use persist_core::{Config, PersistError, Result};
use persist_ipc::{
    decode_list_sessions_resp, decode_new_session_resp, decode_note_get_resp, decode_op_resp,
    decode_process_stats_resp, decode_process_tree_resp, decode_tag_list_resp, encode_detach,
    encode_lock, encode_note, encode_pin, encode_rename, encode_tag, read_frame, write_frame,
    DetachPayload, Frame, ListSessionsRespPayload, LockPayload, MessageType, NotePayload,
    OpRespPayload, PinPayload, RenamePayload, TagPayload,
};

pub fn new_session(config: &Config) -> Result<()> {
    let mut socket = connect_and_hello(config)?;

    write_frame(
        socket.stream(),
        &Frame {
            msg_type: MessageType::NewSession,
            flags: 0,
            request_id: 0,
            payload: vec![],
        },
    )?;

    let resp = read_frame(socket.stream())?;
    if resp.msg_type != MessageType::NewSessionResp {
        return Err(PersistError::invalid_argument("expected NEW_SESSION_RESP"));
    }
    let session = decode_new_session_resp(&resp.payload)
        .ok_or_else(|| PersistError::invalid_argument("invalid NEW_SESSION_RESP payload"))?;

    drop(socket);
    println!("{}", session.session_id);
    Ok(())
}

pub fn process_tree(config: &Config, session_id: u32) -> Result<()> {
    let mut socket = connect_and_hello(config)?;
    write_frame(
        socket.stream(),
        &Frame {
            msg_type: MessageType::ProcessTree,
            flags: 0,
            request_id: 0,
            payload: encode_detach(&DetachPayload { session_id }),
        },
    )?;
    let response = read_frame(socket.stream())?;
    if response.msg_type != MessageType::ProcessTreeResp {
        return Err(PersistError::invalid_argument("expected PROCESS_TREE_RESP"));
    }
    let tree = decode_process_tree_resp(&response.payload)
        .ok_or_else(|| PersistError::invalid_argument("invalid PROCESS_TREE_RESP payload"))?;
    if tree.nodes.is_empty() {
        println!("(no foreground process)");
        return Ok(());
    }
    for node in tree.nodes {
        let indent = "  ".repeat(node.depth as usize);
        let command = if node.command.is_empty() {
            &node.name
        } else {
            &node.command
        };
        println!("{indent}{} {}", node.pid, command);
    }
    Ok(())
}

pub fn process_stats(config: &Config, session_id: u32) -> Result<()> {
    let mut socket = connect_and_hello(config)?;
    write_frame(
        socket.stream(),
        &Frame {
            msg_type: MessageType::ProcessStats,
            flags: 0,
            request_id: 0,
            payload: encode_detach(&DetachPayload { session_id }),
        },
    )?;
    let response = read_frame(socket.stream())?;
    if response.msg_type != MessageType::ProcessStatsResp {
        return Err(PersistError::invalid_argument(
            "expected PROCESS_STATS_RESP",
        ));
    }
    let stats = decode_process_stats_resp(&response.payload)
        .ok_or_else(|| PersistError::invalid_argument("invalid PROCESS_STATS_RESP payload"))?;
    let Some(pid) = stats.pid else {
        println!("(no foreground process)");
        return Ok(());
    };
    println!("pid: {pid}");
    println!(
        "cpu_ticks: user={} system={}",
        stats.user_ticks, stats.system_ticks
    );
    println!("rss_kib: {}", stats.rss_kib);
    println!(
        "io_bytes: read={} write={}",
        stats.read_bytes, stats.write_bytes
    );
    Ok(())
}

pub fn snapshot(config: &Config, session_id: u32) -> Result<()> {
    let mut socket = connect_and_hello(config)?;
    write_frame(
        socket.stream(),
        &Frame {
            msg_type: MessageType::SessionSnapshot,
            flags: 0,
            request_id: 0,
            payload: encode_detach(&DetachPayload { session_id }),
        },
    )?;
    let response = read_frame(socket.stream())?;
    if response.msg_type != MessageType::SessionSnapshotResp {
        return Err(PersistError::invalid_argument(
            "expected SESSION_SNAPSHOT_RESP",
        ));
    }
    let json = decode_note_get_resp(&response.payload)
        .ok_or_else(|| PersistError::invalid_argument("invalid SESSION_SNAPSHOT_RESP payload"))?;
    let value: serde_json::Value = serde_json::from_str(&json)
        .map_err(|_| PersistError::invalid_argument("invalid SESSION_SNAPSHOT_RESP JSON"))?;
    if let Some(error) = value.get("error").and_then(serde_json::Value::as_str) {
        return Err(PersistError::invalid_argument(error));
    }
    println!("{json}");
    Ok(())
}

pub fn metrics(config: &Config) -> Result<()> {
    let mut socket = connect_and_hello(config)?;
    write_frame(
        socket.stream(),
        &Frame {
            msg_type: MessageType::Metrics,
            flags: 0,
            request_id: 0,
            payload: vec![],
        },
    )?;
    let response = read_frame(socket.stream())?;
    if response.msg_type != MessageType::MetricsResp {
        return Err(PersistError::invalid_argument("expected METRICS_RESP"));
    }
    let json = decode_note_get_resp(&response.payload)
        .ok_or_else(|| PersistError::invalid_argument("invalid METRICS_RESP payload"))?;
    let value: serde_json::Value = serde_json::from_str(&json)
        .map_err(|_| PersistError::invalid_argument("invalid METRICS_RESP JSON"))?;
    if let Some(error) = value.get("error").and_then(serde_json::Value::as_str) {
        return Err(PersistError::invalid_argument(error));
    }
    println!("{json}");
    Ok(())
}

pub fn list_sessions<W: Write>(
    config: &Config,
    tag_filter: Option<&str>,
    output: &mut W,
) -> Result<()> {
    let list = fetch_sessions(config, tag_filter)?;
    write_session_list(output, &list)
}

pub fn list_session<W: Write>(config: &Config, session_id: u32, output: &mut W) -> Result<()> {
    let mut list = fetch_sessions(config, None)?;
    list.sessions.retain(|entry| entry.session_id == session_id);
    if list.sessions.is_empty() {
        return Err(PersistError::invalid_argument(format!(
            "session {session_id} was not found"
        )));
    }
    write_session_list(output, &list)
}

pub fn fetch_sessions(
    config: &Config,
    tag_filter: Option<&str>,
) -> Result<ListSessionsRespPayload> {
    let mut socket = connect_and_hello(config)?;

    match tag_filter {
        Some(tag) => {
            let payload = persist_ipc::encode_note_get_resp(tag);
            write_frame(
                socket.stream(),
                &Frame {
                    msg_type: MessageType::ListSessionsByTag,
                    flags: 0,
                    request_id: 0,
                    payload,
                },
            )?;
        }
        None => {
            write_frame(
                socket.stream(),
                &Frame {
                    msg_type: MessageType::ListSessions,
                    flags: 0,
                    request_id: 0,
                    payload: vec![],
                },
            )?;
        }
    }

    let resp = read_frame(socket.stream())?;
    if resp.msg_type != MessageType::ListSessionsResp {
        return Err(PersistError::invalid_argument(
            "expected LIST_SESSIONS_RESP",
        ));
    }
    decode_list_sessions_resp(&resp.payload)
        .ok_or_else(|| PersistError::invalid_argument("invalid LIST_SESSIONS_RESP payload"))
}

pub fn write_session_list<W: Write>(output: &mut W, list: &ListSessionsRespPayload) -> Result<()> {
    if list.sessions.is_empty() {
        writeln!(output, "(no sessions)").map_err(write_list_error)?;
        return Ok(());
    }

    writeln!(
        output,
        "{:<5}  {:<7}  {:<20}  {:<20}  {:<8}  {:<10}  {:<4}  {:<4}  {:<4}  {:<4}  CLOSED",
        "ID", "STATUS", "NAME", "FOREGROUND", "IDLE", "EXIT", "NOTE", "TAGS", "PIN", "LOCK"
    )
    .map_err(write_list_error)?;
    writeln!(output, "{}", "-".repeat(118)).map_err(write_list_error)?;

    for entry in &list.sessions {
        let code = entry
            .exit_code
            .map(|c| format!("exit={c}"))
            .unwrap_or_else(|| "-".to_string());
        let closed = entry.closed_at.as_deref().unwrap_or("-");
        let idle = if entry.idle.is_empty() {
            "-"
        } else {
            &entry.idle
        };
        let foreground = foreground_display(&entry.foreground_cmd, &entry.foreground_name);
        let note_indicator = if entry.has_note { "📝" } else { "" };
        let tags_indicator = if entry.has_tags { "🏷" } else { "" };
        let pin_indicator = if entry.is_pinned { "📌" } else { "" };
        let lock_indicator = if entry.is_locked { "🔒" } else { "" };
        writeln!(
            output,
            "{:<5}  {:<7}  {:<20}  {:<20}  {:<8}  {:<10}  {:<4}  {:<4}  {:<4}  {:<4}  {}",
            entry.session_id,
            entry.status,
            entry.name,
            foreground,
            idle,
            code,
            note_indicator,
            tags_indicator,
            pin_indicator,
            lock_indicator,
            closed
        )
        .map_err(write_list_error)?;
    }
    Ok(())
}

fn write_list_error(source: std::io::Error) -> PersistError {
    PersistError::Io {
        operation: "write session list",
        source,
    }
}

fn foreground_display<'a>(cmd: &'a str, name: &'a str) -> &'a str {
    if !cmd.is_empty() {
        cmd
    } else if !name.is_empty() {
        name
    } else {
        "-"
    }
}

pub fn close_session(config: &Config, session_id: u32) -> Result<()> {
    let mut socket = connect_and_hello(config)?;

    let payload = encode_detach(&DetachPayload { session_id });
    write_frame(
        socket.stream(),
        &Frame {
            msg_type: MessageType::Close,
            flags: 0,
            request_id: 0,
            payload,
        },
    )?;

    let resp = read_frame(socket.stream())?;
    if resp.msg_type != MessageType::CloseResp {
        return Err(PersistError::invalid_argument("expected CLOSE_RESP"));
    }
    let op = decode_op_resp(&resp.payload)
        .ok_or_else(|| PersistError::invalid_argument("invalid CLOSE_RESP payload"))?;

    finish_operation(op)
}

pub fn kill_session(config: &Config, session_id: u32) -> Result<()> {
    let mut socket = connect_and_hello(config)?;

    let payload = encode_detach(&DetachPayload { session_id });
    write_frame(
        socket.stream(),
        &Frame {
            msg_type: MessageType::Kill,
            flags: 0,
            request_id: 0,
            payload,
        },
    )?;

    let resp = read_frame(socket.stream())?;
    if resp.msg_type != MessageType::KillResp {
        return Err(PersistError::invalid_argument("expected KILL_RESP"));
    }
    let op = decode_op_resp(&resp.payload)
        .ok_or_else(|| PersistError::invalid_argument("invalid KILL_RESP payload"))?;

    finish_operation(op)
}

pub fn rename_session(config: &Config, session_id: u32, name: &str) -> Result<()> {
    let mut socket = connect_and_hello(config)?;

    let payload = encode_rename(&RenamePayload {
        session_id,
        name: name.to_string(),
    });
    write_frame(
        socket.stream(),
        &Frame {
            msg_type: MessageType::Rename,
            flags: 0,
            request_id: 0,
            payload,
        },
    )?;

    let resp = read_frame(socket.stream())?;
    if resp.msg_type != MessageType::RenameResp {
        return Err(PersistError::invalid_argument("expected RENAME_RESP"));
    }
    let op = decode_op_resp(&resp.payload)
        .ok_or_else(|| PersistError::invalid_argument("invalid RENAME_RESP payload"))?;

    finish_operation(op)
}

pub fn signal_detach(config: &Config, session_id: u32) -> Result<()> {
    let mut socket = connect_and_hello(config)?;

    let payload = encode_detach(&DetachPayload { session_id });
    write_frame(
        socket.stream(),
        &Frame {
            msg_type: MessageType::DetachSignal,
            flags: 0,
            request_id: 0,
            payload,
        },
    )?;

    let resp = read_frame(socket.stream())?;
    if resp.msg_type != MessageType::DetachSignalResp {
        return Err(PersistError::invalid_argument(
            "expected DETACH_SIGNAL_RESP",
        ));
    }
    let op = decode_op_resp(&resp.payload)
        .ok_or_else(|| PersistError::invalid_argument("invalid DETACH_SIGNAL_RESP payload"))?;

    if op.ok {
        println!("ok");
    } else {
        println!("session is not currently attached");
    }
    Ok(())
}

pub fn read_session_log<W: Write>(config: &Config, session_id: u32, stdout: &mut W) -> Result<()> {
    if !config.logging.session_log {
        writeln!(stdout, "session logging is disabled").map_err(|source| PersistError::Io {
            operation: "write session log output",
            source,
        })?;
        return Ok(());
    }

    let log_path = config
        .paths
        .data_dir
        .join("sessions")
        .join(format!("{session_id}.log"));
    let content = std::fs::read_to_string(&log_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            PersistError::invalid_argument(format!(
                "no log file for session {session_id}: not found at {}",
                log_path.display()
            ))
        } else {
            PersistError::Io {
                operation: "read session log",
                source: e,
            }
        }
    })?;

    writeln!(stdout, "{content}").map_err(|source| PersistError::Io {
        operation: "write session log output",
        source,
    })
}

pub fn log_search<W: Write>(
    config: &Config,
    keyword: &str,
    session_id: Option<u32>,
    case_insensitive: bool,
    stdout: &mut W,
) -> Result<()> {
    let log_dir = config.paths.data_dir.join("sessions");

    if !log_dir.exists() {
        writeln!(stdout, "(no session logs found)").map_err(|source| PersistError::Io {
            operation: "write log search output",
            source,
        })?;
        return Ok(());
    }

    let keyword_lower = keyword.to_lowercase();
    let mut total_matches = 0;

    let file_names: Vec<_> = if let Some(sid) = session_id {
        // Collect current + rotated files for a specific session
        let mut names = vec![format!("{sid}.log")];
        for i in 1..=config.logging.max_files {
            names.push(format!("{sid}.log.{i}"));
        }
        names
    } else {
        // Collect all .log files (and .log.N rotated) from the directory
        let mut names = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&log_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy().to_string();
                // Match *.log or *.log.N
                if name_str.ends_with(".log") || name_str.contains(".log.") {
                    names.push(name_str);
                }
            }
            names.sort();
        }
        names
    };

    for file_name in &file_names {
        let path = log_dir.join(file_name);
        if !path.exists() {
            continue;
        }
        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let reader = std::io::BufReader::new(file);

        // Extract session_id from filename for display
        let sid = file_name
            .split('.')
            .next()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);

        for (line_num, line) in reader.lines().enumerate() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            let matched = if case_insensitive {
                line.to_lowercase().contains(&keyword_lower)
            } else {
                line.contains(keyword)
            };
            if matched {
                writeln!(stdout, "[{sid}] {:<6} {}", line_num + 1, line).map_err(|source| {
                    PersistError::Io {
                        operation: "write log search output",
                        source,
                    }
                })?;
                total_matches += 1;
            }
        }
    }

    if total_matches == 0 {
        writeln!(stdout, "(no matches)").map_err(|source| PersistError::Io {
            operation: "write log search output",
            source,
        })?;
    }

    Ok(())
}

pub(crate) fn connect_and_hello(config: &Config) -> Result<persist_ipc::ClientSocket> {
    let mut socket = persist_ipc::ClientSocket::connect(&config.paths.socket_path)?;

    let uid = unsafe { libc::getuid() };
    let hello_payload = persist_ipc::encode_hello(&persist_ipc::HelloPayload {
        protocol_major: 0,
        protocol_minor: 1,
        uid,
        pid: std::process::id(),
    });
    write_frame(
        socket.stream(),
        &Frame {
            msg_type: MessageType::Hello,
            flags: 0,
            request_id: 0,
            payload: hello_payload,
        },
    )?;

    let ack = read_frame(socket.stream())?;
    if ack.msg_type != MessageType::HelloAck {
        return Err(PersistError::invalid_argument("expected HELLO_ACK"));
    }
    socket.clear_timeouts()?;
    Ok(socket)
}

fn finish_operation(op: OpRespPayload) -> Result<()> {
    if op.ok {
        println!("ok");
        Ok(())
    } else {
        Err(PersistError::invalid_argument(op.error_msg))
    }
}

pub fn note_session(config: &Config, session_id: u32, text: Option<&str>) -> Result<()> {
    let mut socket = connect_and_hello(config)?;

    match text {
        Some(note_text) => {
            // Set note
            let payload = encode_note(&NotePayload {
                session_id,
                note: note_text.to_string(),
            });
            write_frame(
                socket.stream(),
                &Frame {
                    msg_type: MessageType::NoteSet,
                    flags: 0,
                    request_id: 0,
                    payload,
                },
            )?;

            let resp = read_frame(socket.stream())?;
            if resp.msg_type != MessageType::NoteSetResp {
                return Err(PersistError::invalid_argument("expected NOTE_SET_RESP"));
            }
            let op = decode_op_resp(&resp.payload)
                .ok_or_else(|| PersistError::invalid_argument("invalid NOTE_SET_RESP payload"))?;
            finish_operation(op)?;
        }
        None => {
            // Get note
            let payload = encode_detach(&DetachPayload { session_id });
            write_frame(
                socket.stream(),
                &Frame {
                    msg_type: MessageType::NoteGet,
                    flags: 0,
                    request_id: 0,
                    payload,
                },
            )?;

            let resp = read_frame(socket.stream())?;
            if resp.msg_type != MessageType::NoteGetResp {
                return Err(PersistError::invalid_argument("expected NOTE_GET_RESP"));
            }
            let note = decode_note_get_resp(&resp.payload)
                .ok_or_else(|| PersistError::invalid_argument("invalid NOTE_GET_RESP payload"))?;
            if note.is_empty() {
                println!("(no note)");
            } else {
                println!("{}", note);
            }
        }
    }

    Ok(())
}

pub fn tag_session(
    config: &Config,
    session_id: u32,
    action: &str,
    tag: Option<&str>,
) -> Result<()> {
    let mut socket = connect_and_hello(config)?;

    match action {
        "add" => {
            let payload = encode_tag(&TagPayload {
                session_id,
                tag: tag.unwrap().to_string(),
            });
            write_frame(
                socket.stream(),
                &Frame {
                    msg_type: MessageType::TagAdd,
                    flags: 0,
                    request_id: 0,
                    payload,
                },
            )?;
            let resp = read_frame(socket.stream())?;
            if resp.msg_type != MessageType::TagAddResp {
                return Err(PersistError::invalid_argument("expected TAG_ADD_RESP"));
            }
            let op = decode_op_resp(&resp.payload)
                .ok_or_else(|| PersistError::invalid_argument("invalid TAG_ADD_RESP payload"))?;
            finish_operation(op)?;
        }
        "remove" => {
            let payload = encode_tag(&TagPayload {
                session_id,
                tag: tag.unwrap().to_string(),
            });
            write_frame(
                socket.stream(),
                &Frame {
                    msg_type: MessageType::TagRemove,
                    flags: 0,
                    request_id: 0,
                    payload,
                },
            )?;
            let resp = read_frame(socket.stream())?;
            if resp.msg_type != MessageType::TagRemoveResp {
                return Err(PersistError::invalid_argument("expected TAG_REMOVE_RESP"));
            }
            let op = decode_op_resp(&resp.payload)
                .ok_or_else(|| PersistError::invalid_argument("invalid TAG_REMOVE_RESP payload"))?;
            finish_operation(op)?;
        }
        "list" => {
            let payload = encode_detach(&DetachPayload { session_id });
            write_frame(
                socket.stream(),
                &Frame {
                    msg_type: MessageType::TagList,
                    flags: 0,
                    request_id: 0,
                    payload,
                },
            )?;
            let resp = read_frame(socket.stream())?;
            if resp.msg_type != MessageType::TagListResp {
                return Err(PersistError::invalid_argument("expected TAG_LIST_RESP"));
            }
            let list = decode_tag_list_resp(&resp.payload)
                .ok_or_else(|| PersistError::invalid_argument("invalid TAG_LIST_RESP payload"))?;
            if list.tags.is_empty() {
                println!("(no tags)");
            } else {
                for tag in &list.tags {
                    println!("{tag}");
                }
            }
        }
        _ => unreachable!(),
    }

    Ok(())
}

pub fn pin_session(config: &Config, session_id: u32, pinned: bool) -> Result<()> {
    let mut socket = connect_and_hello(config)?;

    let payload = encode_pin(&PinPayload { session_id, pinned });
    write_frame(
        socket.stream(),
        &Frame {
            msg_type: MessageType::PinSet,
            flags: 0,
            request_id: 0,
            payload,
        },
    )?;

    let resp = read_frame(socket.stream())?;
    if resp.msg_type != MessageType::PinSetResp {
        return Err(PersistError::invalid_argument("expected PIN_SET_RESP"));
    }
    let op = decode_op_resp(&resp.payload)
        .ok_or_else(|| PersistError::invalid_argument("invalid PIN_SET_RESP payload"))?;
    finish_operation(op)
}

pub fn lock_session(config: &Config, session_id: u32, locked: bool) -> Result<()> {
    let mut socket = connect_and_hello(config)?;
    let payload = encode_lock(&LockPayload { session_id, locked });
    write_frame(
        socket.stream(),
        &Frame {
            msg_type: MessageType::LockSet,
            flags: 0,
            request_id: 0,
            payload,
        },
    )?;
    let resp = read_frame(socket.stream())?;
    if resp.msg_type != MessageType::LockSetResp {
        return Err(PersistError::invalid_argument("expected LOCK_SET_RESP"));
    }
    let op = decode_op_resp(&resp.payload)
        .ok_or_else(|| PersistError::invalid_argument("invalid LOCK_SET_RESP payload"))?;
    finish_operation(op)
}

pub fn export_session_log<W: Write>(
    config: &Config,
    session_id: u32,
    output_path: Option<&str>,
    stdout: &mut W,
) -> Result<()> {
    if !config.logging.session_log {
        writeln!(stdout, "session logging is disabled").map_err(|source| PersistError::Io {
            operation: "write session log output",
            source,
        })?;
        return Ok(());
    }

    let log_path = config
        .paths
        .data_dir
        .join("sessions")
        .join(format!("{session_id}.log"));

    let content = std::fs::read_to_string(&log_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            PersistError::invalid_argument(format!(
                "no log file for session {session_id}: not found at {}",
                log_path.display()
            ))
        } else {
            PersistError::Io {
                operation: "read session log",
                source: e,
            }
        }
    })?;

    match output_path {
        Some(path) => {
            let dest = std::path::Path::new(path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| PersistError::Io {
                    operation: "create export directory",
                    source: e,
                })?;
            }
            std::fs::write(dest, &content).map_err(|e| PersistError::Io {
                operation: "write exported log",
                source: e,
            })?;
            writeln!(
                stdout,
                "exported session {session_id} log to {}",
                dest.display()
            )
            .map_err(|source| PersistError::Io {
                operation: "write export confirmation",
                source,
            })
        }
        None => writeln!(stdout, "{content}").map_err(|source| PersistError::Io {
            operation: "write session log output",
            source,
        }),
    }
}

pub fn replay_session<W: Write>(
    config: &Config,
    session_id: u32,
    tail: Option<usize>,
    head: Option<usize>,
    _speed: Option<f64>,
    _follow: bool,
    stdout: &mut W,
) -> Result<()> {
    if !config.logging.session_log {
        writeln!(stdout, "session logging is disabled").map_err(|source| PersistError::Io {
            operation: "write replay output",
            source,
        })?;
        return Ok(());
    }

    let log_path = config
        .paths
        .data_dir
        .join("sessions")
        .join(format!("{session_id}.log"));

    let content = match std::fs::read_to_string(&log_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(PersistError::invalid_argument(format!(
                "no log file for session {session_id}: not found at {}",
                log_path.display()
            )));
        }
        Err(e) => {
            return Err(PersistError::Io {
                operation: "read session log",
                source: e,
            });
        }
    };

    let lines: Vec<&str> = content.lines().collect();
    let selected: &[&str] = match (head, tail) {
        (Some(h), _) => &lines[..std::cmp::min(h, lines.len())],
        (_, Some(t)) => {
            let start = lines.len().saturating_sub(t);
            &lines[start..]
        }
        _ => &lines[..],
    };

    for line in selected {
        writeln!(stdout, "{line}").map_err(|source| PersistError::Io {
            operation: "write replay output",
            source,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use persist_core::ConfigPaths;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

    fn test_setup() -> (Config, std::path::PathBuf) {
        let id = std::process::id().wrapping_add(TEST_COUNTER.fetch_add(1, Ordering::SeqCst));
        let base = std::env::temp_dir().join(format!("persist-log-search-{id}"));
        let home = base.join("home");
        let run = base.join("run");
        let _ = std::fs::create_dir_all(&home);
        let _ = std::fs::create_dir_all(&run);
        let paths = ConfigPaths::from_base_dirs(home, None, None, None, run);
        (Config::default_with_paths(paths), base)
    }

    fn write_log(base: &std::path::Path, session_id: u32, lines: &[&str]) {
        let dir = base.join("home/.local/share/persistshell/sessions");
        std::fs::create_dir_all(&dir).expect("create log dir");
        let path = dir.join(format!("{session_id}.log"));
        let content = lines.join("\n");
        std::fs::write(&path, content).expect("write log");
    }

    #[test]
    fn log_export_to_stdout() {
        let (config, base) = test_setup();
        write_log(&base, 1, &["hello world", "goodbye world"]);
        let mut out = Vec::new();
        export_session_log(&config, 1, None, &mut out).expect("export");
        let output = String::from_utf8(out).expect("utf8");
        assert!(output.contains("hello world"), "output: {output}");
        assert!(output.contains("goodbye world"), "output: {output}");
    }

    #[test]
    fn log_export_to_file() {
        let (config, base) = test_setup();
        write_log(&base, 1, &["line one", "line two"]);
        let export_path = base.join("exported.log");
        let export_str = export_path.to_string_lossy().to_string();
        let mut out = Vec::new();
        export_session_log(&config, 1, Some(&export_str), &mut out).expect("export");
        let output = String::from_utf8(out).expect("utf8");
        assert!(
            output.contains("exported session 1 log to"),
            "output: {output}"
        );
        let exported = std::fs::read_to_string(&export_path).expect("read exported file");
        assert!(exported.contains("line one"), "exported: {exported}");
        assert!(exported.contains("line two"), "exported: {exported}");
    }

    #[test]
    fn log_export_session_not_found() {
        let (config, _base) = test_setup();
        let mut out = Vec::new();
        let err = export_session_log(&config, 99, None, &mut out).unwrap_err();
        assert!(err.to_string().contains("no log file for session 99"));
    }

    #[test]
    fn log_export_disabled_logging() {
        let (config, base) = test_setup();
        write_log(&base, 1, &["hello"]);
        let mut disabled_config = config.clone();
        disabled_config.logging.session_log = false;
        let mut out = Vec::new();
        export_session_log(&disabled_config, 1, None, &mut out).expect("export");
        let output = String::from_utf8(out).expect("utf8");
        assert!(
            output.contains("session logging is disabled"),
            "output: {output}"
        );
    }

    #[test]
    fn log_search_finds_matching_lines() {
        let (config, base) = test_setup();
        write_log(&base, 1, &["hello world", "goodbye world", "foo bar"]);
        let mut out = Vec::new();
        log_search(&config, "world", None, false, &mut out).expect("search");
        let output = String::from_utf8(out).expect("utf8");
        assert!(output.contains("[1]"), "output: {output}");
        assert!(output.contains("hello world"), "output: {output}");
        assert!(output.contains("goodbye world"), "output: {output}");
        assert!(!output.contains("foo bar"), "output: {output}");
    }

    #[test]
    fn log_search_case_insensitive() {
        let (config, base) = test_setup();
        write_log(&base, 2, &["Hello World", "goodbye"]);
        let mut out = Vec::new();
        log_search(&config, "hello", None, true, &mut out).expect("search");
        let output = String::from_utf8(out).expect("utf8");
        assert!(output.contains("Hello World"), "output: {output}");
    }

    #[test]
    fn log_search_specific_session() {
        let (config, base) = test_setup();
        write_log(&base, 1, &["common text"]);
        write_log(&base, 2, &["common text"]);
        let mut out = Vec::new();
        log_search(&config, "common", Some(1), false, &mut out).expect("search");
        let output = String::from_utf8(out).expect("utf8");
        assert!(output.contains("[1]"), "output: {output}");
        assert!(!output.contains("[2]"), "output: {output}");
    }

    #[test]
    fn log_search_no_match_shows_message() {
        let (config, base) = test_setup();
        write_log(&base, 1, &["foo bar"]);
        let mut out = Vec::new();
        log_search(&config, "nonexistent", None, false, &mut out).expect("search");
        let output = String::from_utf8(out).expect("utf8");
        assert!(output.contains("(no matches)"), "output: {output}");
    }

    #[test]
    fn log_search_empty_log_dir() {
        let id = std::process::id().wrapping_add(99999);
        let base = std::env::temp_dir().join(format!("persist-log-search-empty-{id}"));
        let home = base.join("home");
        let run = base.join("run");
        let _ = std::fs::create_dir_all(&home);
        let _ = std::fs::create_dir_all(&run);
        let paths = ConfigPaths::from_base_dirs(home, None, None, None, run);
        let config = Config::default_with_paths(paths);
        let mut out = Vec::new();
        log_search(&config, "foo", None, false, &mut out).expect("search");
        let output = String::from_utf8(out).expect("utf8");
        assert!(
            output.contains("(no session logs found)"),
            "output: {output}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn replay_full_log() {
        let (config, base) = test_setup();
        let lines = &["line one", "line two", "line three"];
        write_log(&base, 3, lines);
        let mut out = Vec::new();
        replay_session(&config, 3, None, None, None, false, &mut out).expect("replay");
        let output = String::from_utf8(out).expect("utf8");
        assert!(output.contains("line one"), "output: {output}");
        assert!(output.contains("line two"), "output: {output}");
        assert!(output.contains("line three"), "output: {output}");
    }

    #[test]
    fn replay_tail() {
        let (config, base) = test_setup();
        write_log(&base, 4, &["a", "b", "c", "d", "e"]);
        let mut out = Vec::new();
        replay_session(&config, 4, Some(2), None, None, false, &mut out).expect("replay");
        let output = String::from_utf8(out).expect("utf8");
        assert!(!output.contains("a"), "output: {output}");
        assert!(!output.contains("b"), "output: {output}");
        assert!(!output.contains("c"), "output: {output}");
        assert!(output.contains("d\n"), "output: {output}");
        assert!(output.contains("e\n"), "output: {output}");
    }

    #[test]
    fn replay_head() {
        let (config, base) = test_setup();
        write_log(&base, 5, &["first", "second", "third", "fourth", "fifth"]);
        let mut out = Vec::new();
        replay_session(&config, 5, None, Some(2), None, false, &mut out).expect("replay");
        let output = String::from_utf8(out).expect("utf8");
        assert!(output.contains("first\n"), "output: {output}");
        assert!(output.contains("second\n"), "output: {output}");
        assert!(!output.contains("third"), "output: {output}");
    }

    #[test]
    fn replay_session_not_found() {
        let (config, _base) = test_setup();
        let mut out = Vec::new();
        let err = replay_session(&config, 42, None, None, None, false, &mut out).unwrap_err();
        assert!(
            err.to_string().contains("no log file for session 42"),
            "err: {err}"
        );
    }
}
#[test]
fn foreground_display_prefers_command_then_name() {
    assert_eq!(foreground_display("make -j8", "make"), "make -j8");
    assert_eq!(foreground_display("", "make"), "make");
    assert_eq!(foreground_display("", ""), "-");
}
