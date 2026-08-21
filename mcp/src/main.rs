use std::io;

fn main() {
    match dicta_mcp::omarchy::run(std::env::args().skip(1).collect()) {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
    let stdin = io::stdin();
    let stdout = io::stdout();
    if let Err(error) = dicta_mcp::server::run(stdin.lock(), stdout.lock()) {
        eprintln!("Dicta MCP transport stopped: {error}");
        std::process::exit(1);
    }
}
