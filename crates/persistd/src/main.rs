mod server;

fn main() -> std::process::ExitCode {
    server::run(std::env::args())
}
