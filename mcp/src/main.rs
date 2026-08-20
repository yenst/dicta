use std::io;

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    if let Err(error) = dicta_mcp::server::run(stdin.lock(), stdout.lock()) {
        eprintln!("Dicta MCP transport stopped: {error}");
    }
}
