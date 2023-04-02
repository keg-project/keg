use keg::run::workspace;
use std::process::ExitCode;

fn main() -> ExitCode {
    workspace::run(true)
}
