use std::io::{self, BufRead, Write};
use std::path::Path;

use persist_core::{command_history_path, read_commands_desc, Config, PersistError, Result};
use persist_ipc::ListSessionsRespPayload;

const PAGE_SIZE: usize = 50;

pub fn browse(
    config: &Config,
    initial_session_id: Option<u32>,
    tag_filter: Option<&str>,
) -> Result<()> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    browse_with(
        &config.paths.data_dir,
        initial_session_id,
        tag_filter,
        &mut input,
        &mut output,
        |tag| crate::session::fetch_sessions(config, tag),
        |session_id| crate::attach::attach(config, Some(session_id), false),
    )
}

pub(crate) fn browse_with<R, W, F, A>(
    data_dir: &Path,
    initial_session_id: Option<u32>,
    tag_filter: Option<&str>,
    input: &mut R,
    output: &mut W,
    mut fetch: F,
    mut attach: A,
) -> Result<()>
where
    R: BufRead,
    W: Write,
    F: FnMut(Option<&str>) -> Result<ListSessionsRespPayload>,
    A: FnMut(u32) -> Result<()>,
{
    let mut selected = initial_session_id;
    loop {
        let list = fetch(tag_filter)?;
        if let Some(session_id) = selected.take() {
            if !list
                .sessions
                .iter()
                .any(|entry| entry.session_id == session_id)
            {
                writeln!(output, "Session {session_id} 不存在。").browser_io()?;
                continue;
            }
            match session_menu(data_dir, session_id, input, output, &mut attach)? {
                MenuResult::Back => continue,
                MenuResult::Quit => return Ok(()),
            }
        }

        crate::session::write_session_list(output, &list)?;
        if list.sessions.is_empty() {
            return Ok(());
        }
        write!(output, "Session ID（q 退出）: ").browser_io()?;
        output.flush().browser_io()?;
        let Some(choice) = read_choice(input)? else {
            return Ok(());
        };
        if choice.eq_ignore_ascii_case("q") {
            return Ok(());
        }
        match choice.parse::<u32>() {
            Ok(session_id) => selected = Some(session_id),
            Err(_) => writeln!(output, "请输入有效的 Session ID，或输入 q 退出。").browser_io()?,
        }
    }
}

fn session_menu<R, W, A>(
    data_dir: &Path,
    session_id: u32,
    input: &mut R,
    output: &mut W,
    attach: &mut A,
) -> Result<MenuResult>
where
    R: BufRead,
    W: Write,
    A: FnMut(u32) -> Result<()>,
{
    loop {
        writeln!(output, "\nSession {session_id}").browser_io()?;
        writeln!(output, "[h] 查看命令历史").browser_io()?;
        writeln!(output, "[a] attach 进入会话").browser_io()?;
        writeln!(output, "[b] 返回 Session 列表").browser_io()?;
        writeln!(output, "[q] 退出").browser_io()?;
        write!(output, "> ").browser_io()?;
        output.flush().browser_io()?;
        let Some(choice) = read_choice(input)? else {
            return Ok(MenuResult::Quit);
        };
        match choice.to_ascii_lowercase().as_str() {
            "h" => {
                if history_pages(data_dir, session_id, input, output)? == MenuResult::Quit {
                    return Ok(MenuResult::Quit);
                }
            }
            "a" => {
                output.flush().browser_io()?;
                if let Err(error) = attach(session_id) {
                    writeln!(output, "attach 失败: {error}").browser_io()?;
                }
            }
            "b" => return Ok(MenuResult::Back),
            "q" => return Ok(MenuResult::Quit),
            _ => writeln!(output, "请输入 h、a、b 或 q。").browser_io()?,
        }
    }
}

fn history_pages<R: BufRead, W: Write>(
    data_dir: &Path,
    session_id: u32,
    input: &mut R,
    output: &mut W,
) -> Result<MenuResult> {
    let path = command_history_path(data_dir, session_id);
    let mut offset = 0;
    loop {
        let records = match read_commands_desc(&path, offset, PAGE_SIZE) {
            Ok(records) => records,
            Err(error) => {
                writeln!(output, "命令历史不可用: {error}").browser_io()?;
                return Ok(MenuResult::Back);
            }
        };
        writeln!(output, "\nSession {session_id} 命令历史（最新优先）").browser_io()?;
        write_history_status(data_dir, session_id, output)?;
        if records.is_empty() {
            writeln!(output, "(没有命令历史)").browser_io()?;
        } else {
            for record in &records {
                write_record(output, record)?;
            }
        }
        writeln!(output, "[n] 更早  [p] 更新  [b] 返回  [q] 退出").browser_io()?;
        write!(output, "> ").browser_io()?;
        output.flush().browser_io()?;
        let Some(choice) = read_choice(input)? else {
            return Ok(MenuResult::Quit);
        };
        match choice.to_ascii_lowercase().as_str() {
            "n" if records.len() == PAGE_SIZE => offset += PAGE_SIZE,
            "n" => writeln!(output, "已经是最早一页。").browser_io()?,
            "p" if offset >= PAGE_SIZE => offset -= PAGE_SIZE,
            "p" => writeln!(output, "已经是最新一页。").browser_io()?,
            "b" => return Ok(MenuResult::Back),
            "q" => return Ok(MenuResult::Quit),
            _ => writeln!(output, "请输入 n、p、b 或 q。").browser_io()?,
        }
    }
}

fn write_history_status<W: Write>(data_dir: &Path, session_id: u32, output: &mut W) -> Result<()> {
    let status_path = data_dir
        .join("history/.hooks")
        .join(session_id.to_string())
        .join("status");
    match std::fs::read_to_string(status_path)
        .as_deref()
        .map(str::trim)
    {
        Ok("filtered") => writeln!(
            output,
            "实时命令历史不可用：检测到自定义 Shell history 过滤器，已优先保留用户配置。"
        )
        .browser_io()?,
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            writeln!(output, "实时命令历史状态不可读: {error}").browser_io()?
        }
        _ => {}
    }
    Ok(())
}

fn write_record<W: Write>(output: &mut W, record: &persist_core::CommandRecord) -> Result<()> {
    let command = sanitize_command(&String::from_utf8_lossy(&record.command));
    let mut lines = command.lines();
    let first = lines.next().unwrap_or_default();
    writeln!(
        output,
        "[{}] {}  {}",
        record.sequence,
        format_timestamp(record.completed_at_ms),
        first
    )
    .browser_io()?;
    for line in lines {
        writeln!(output, "    {line}").browser_io()?;
    }
    Ok(())
}

fn sanitize_command(command: &str) -> String {
    command
        .chars()
        .map(|character| {
            if character == '\n' || character == '\t' || !character.is_control() {
                character
            } else {
                '?'
            }
        })
        .collect()
}

fn format_timestamp(milliseconds: u64) -> String {
    let seconds = (milliseconds / 1000).min(i64::MAX as u64) as libc::time_t;
    let mut broken_down = unsafe { std::mem::zeroed::<libc::tm>() };
    if unsafe { libc::localtime_r(&seconds, &mut broken_down) }.is_null() {
        return seconds.to_string();
    }
    let mut buffer = [0u8; 32];
    let format = b"%Y-%m-%d %H:%M:%S\0";
    let length = unsafe {
        libc::strftime(
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            format.as_ptr().cast(),
            &broken_down,
        )
    };
    String::from_utf8_lossy(&buffer[..length]).into_owned()
}

fn read_choice<R: BufRead>(input: &mut R) -> Result<Option<String>> {
    let mut line = String::new();
    if input.read_line(&mut line).browser_io()? == 0 {
        return Ok(None);
    }
    Ok(Some(line.trim().to_string()))
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum MenuResult {
    Back,
    Quit,
}

trait BrowserIo<T> {
    fn browser_io(self) -> Result<T>;
}

impl<T> BrowserIo<T> for io::Result<T> {
    fn browser_io(self) -> Result<T> {
        self.map_err(|source| PersistError::Io {
            operation: "interactive session browser",
            source,
        })
    }
}
