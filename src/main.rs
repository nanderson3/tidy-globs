use std::env;
use std::io;
use std::process::ExitCode;

use tidy_globs::{parse_args, run};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let (mode, patterns) = parse_args(&args);
    let stdin = io::stdin();
    let stdout = io::stdout();

    match run(&patterns, mode, stdin.lock(), stdout.lock()) {
        Ok(needs_normalizing) => {
            if needs_normalizing {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(err) => {
            eprintln!("tidy-globs: {}", err);
            ExitCode::FAILURE
        }
    }
}
