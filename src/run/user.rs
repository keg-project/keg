//! The purpose of this program is to create a new user namespace with a different uid and gid.
//! No security is guaranteed.

use crate::die_with_parent::set_die_with_parent;
use crate::{msg_and, msg_ret, msg_retf, ok_or, some_or, true_or};
use indoc::indoc;
use libc::{getgid, getuid, gid_t, uid_t, unshare, CLONE_NEWUSER};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::{self, Command, ExitCode, ExitStatus};

static HELP_MESSAGE: &'static str = indoc! {r#"
Usage: [OPTIONS] [--] [COMMAND]...

Arguments:
    [COMMAND]...        Command and arguments to run in the new namespace. If
                        empty, $SHELL will be used, unless $SHELL is unset in
                        which case /bin/bash will be used.

Options:
    --help              Display this message and exit
    --no-die-with-parent
                        Do not kill child processes when this process dies
    --uid <UID>         Set uid to <UID> in the new user namespace. The
                        default is 1000.
    --gid <GID>         Set gid to <GID> in the new user namespace. The
                        default is 1000.
"#};

struct Args {
    no_die_with_parent: bool,
    uid: uid_t,
    gid: gid_t,
    command: Vec<OsString>,
}

fn handle_args() -> Option<Args> {
    let mut args = env::args_os().peekable();
    some_or!(args.next(), msg_ret!("Argument required"));

    let mut no_die_with_parent = false;
    let mut uid = 1000;
    let mut gid = 1000;
    let mut command: Vec<OsString> = Vec::new();

    while let Some(arg) = args.next() {
        if &arg == "--help" {
            println!("{HELP_MESSAGE}");
            process::exit(0);
        } else if &arg == "--no-die-with-parent" {
            no_die_with_parent = true;
        } else if &arg == "--uid" {
            let uid_arg = some_or!(args.next(), msg_ret!("--uid requires an argument"));
            uid = some_or!(
                (uid_arg.into_string().ok()).and_then(|x| x.parse().ok()),
                msg_ret!("Invalid uid")
            );
        } else if &arg == "--gid" {
            let gid_arg = some_or!(args.next(), msg_ret!("--gid requires an argument"));
            gid = some_or!(
                (gid_arg.into_string().ok()).and_then(|x| x.parse().ok()),
                msg_ret!("Invalid gid")
            );
        } else if &arg == "--" || !arg.as_bytes().starts_with(b"-") {
            debug_assert!(command.is_empty());
            if !arg.as_bytes().starts_with(b"-") {
                command.push(arg);
            }
            while let Some(arg) = args.next() {
                command.push(arg);
            }
            break;
        } else {
            msg_ret!("Unknown argument {}. Try --help.", arg.to_string_lossy());
        }
    }
    if command.is_empty() {
        command = vec![env::var_os("SHELL").unwrap_or("/bin/bash".into())];
    }

    Some(Args {
        no_die_with_parent,
        uid,
        gid,
        command,
    })
}

fn unshare_user(uid: uid_t, gid: gid_t) -> bool {
    let parent_uid = unsafe { getuid() };
    let parent_gid = unsafe { getgid() };
    true_or!(
        unsafe { unshare(CLONE_NEWUSER) } == 0,
        msg_retf!("Cannot create new user ns")
    );
    ok_or!(
        fs::write("/proc/self/uid_map", format!("{uid} {parent_uid} 1\n")),
        msg_retf!("Cannot write to uid_map")
    );
    ok_or!(
        fs::write("/proc/self/setgroups", "deny"),
        msg_retf!("Cannot write to setgroups")
    );
    ok_or!(
        fs::write("/proc/self/gid_map", format!("{gid} {parent_gid} 1\n")),
        msg_retf!("Cannot write to gid_map")
    );
    true
}

fn run_command(command: &[OsString]) -> Option<ExitStatus> {
    let mut child = match Command::new(&command[0]).args(&command[1..]).spawn() {
        Err(e) => msg_ret!(
            "Failed to run command: Running `{}`: {}",
            Path::new(&command[0]).display(),
            e
        ),
        Ok(child) => child,
    };
    let status = match child.wait() {
        Err(e) => msg_ret!("Failed to wait for command before unshare: {e}"),
        Ok(status) => status,
    };
    Some(status)
}

pub fn run() -> ExitCode {
    let args = some_or!(handle_args(), return ExitCode::FAILURE);
    if !args.no_die_with_parent {
        true_or!(
            set_die_with_parent(),
            msg_and!("Failed to set die-with-parent"; return ExitCode::FAILURE)
        );
    }

    true_or!(unshare_user(args.uid, args.gid), return ExitCode::FAILURE);
    let exit_status = some_or!(run_command(&args.command), return ExitCode::FAILURE);

    exit_status
        .code()
        .map(|c| ((((c % 256) + 256) % 256) as u8).into())
        .unwrap_or(ExitCode::FAILURE)
}
