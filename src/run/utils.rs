use crate::some_or;
use std::env;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode};

pub fn run_in_scope() -> ExitCode {
    // Run in a new systemd scope.
    let mut args = Vec::<OsString>::new();
    args.push("--user".into());
    args.push("--scope".into());
    args.push("-q".into());
    args.push("--".into());
    let mut caller_args = env::args_os();
    args.push(some_or!(caller_args.next(), return ExitCode::FAILURE));
    args.push("--no-new-scope".into());
    while let Some(arg) = caller_args.next() {
        args.push(arg);
    }
    let error = Command::new("systemd-run").args(args).exec();
    eprintln!("Failed to run `systemd-run --user --scope ...`: {error}");
    ExitCode::FAILURE
}
