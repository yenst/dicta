#![forbid(unsafe_code)]

use dicta_cli::{
    execute, offline::FileOfflineStore, parse, write_diagnostic, Runtime, SystemControl, SystemHost,
};
use dicta_control::{cli::OutputFormat, ExitCode};
use std::io;

fn main() {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let invocation = match parse(arguments.iter().cloned()) {
        Ok(invocation) => invocation,
        Err(error) => {
            let format = if arguments.iter().any(|argument| argument == "--json") {
                OutputFormat::Json
            } else {
                OutputFormat::Human
            };
            let _ = write_diagnostic(&mut io::stderr().lock(), format, &error);
            std::process::exit(error.kind.exit_code().get().into());
        }
    };
    let control = SystemControl;
    let host = SystemHost;
    let offline = FileOfflineStore::discover();
    let runtime = Runtime {
        control: &control,
        host: &host,
        offline: Some(&offline),
    };
    let code = execute(
        &invocation,
        &runtime,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    );
    if code != ExitCode::Success {
        std::process::exit(code.get().into());
    }
}
