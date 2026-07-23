mod attach_history;
mod dashboard;
mod holder;
mod lifecycle;
#[cfg(test)]
mod log_writer;
mod public_attach;
mod server;
mod shell_history;

#[cfg(test)]
mod shell_history_tests;

fn main() -> std::process::ExitCode {
    server::run(std::env::args())
}
