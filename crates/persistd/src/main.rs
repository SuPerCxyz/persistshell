mod lifecycle;
mod log_writer;
mod server;
mod shell_history;

#[cfg(test)]
mod shell_history_tests;

fn main() -> std::process::ExitCode {
    server::run(std::env::args())
}
