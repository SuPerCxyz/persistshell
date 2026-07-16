mod attach;
mod cli;
mod command;
mod command_history;
mod daemon;
mod installer;
mod session;
mod session_browser;
mod terminal;

#[cfg(test)]
mod session_browser_tests;

fn main() -> std::process::ExitCode {
    cli::run(std::env::args())
}
