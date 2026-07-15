mod attach;
mod cli;
mod command;
mod daemon;
mod installer;
mod session;
mod terminal;

fn main() -> std::process::ExitCode {
    cli::run(std::env::args())
}
