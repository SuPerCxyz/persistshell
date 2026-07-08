mod attach;
mod cli;
mod command;
mod terminal;

fn main() -> std::process::ExitCode {
    cli::run(std::env::args())
}
