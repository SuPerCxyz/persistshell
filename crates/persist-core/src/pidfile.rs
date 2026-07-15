use std::fs::File;
use std::io::Read;
use std::path::Path;

pub fn read_pid(path: &Path) -> Option<u32> {
    let mut content = String::new();
    File::open(path).ok()?.read_to_string(&mut content).ok()?;
    content.trim().parse::<u32>().ok()
}

pub fn is_process_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

pub fn is_running(path: &Path) -> bool {
    if let Some(pid) = read_pid(path) {
        is_process_alive(pid)
    } else {
        false
    }
}

pub fn send_signal(pid: u32, signal: i32) -> std::io::Result<()> {
    let result = unsafe { libc::kill(pid as i32, signal) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
