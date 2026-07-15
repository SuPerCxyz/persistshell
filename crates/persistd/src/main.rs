mod lifecycle;
mod log_writer;
mod server;

fn main() -> std::process::ExitCode {
    server::run(std::env::args())
}
