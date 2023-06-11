use super::utils::run_in_scope;
use crate::container::{start_container, Bind, Container, Mount, Options, SetEnv};
use crate::die_with_parent::set_die_with_parent;
use crate::run::inner;
use crate::{msg_and, msg_ret, ok_or, some_or, some_or_ret, true_or};
use indoc::indoc;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::process::{self, ExitCode};

static HELP_MESSAGE: &'static str = indoc! {r#"
Usage: [OPTIONS] [--] [COMMAND]...

Arguments:
    [COMMAND]...        Command and arguments to run in the container. If
                        empty, /bin/bash will be used.

                        keg-base does not search $PATH; the command must
                        either be an absolute path, or a path relative to
                        container root.

Options:
    --help              Display this message and exit
    --no-die-with-parent
                        Do not kill child processes when this process dies
    --no-new-scope      Do not run in a new systemd scope
    --share-net         Enable network
    --share-time        Share time namespace
    --keep-env          Keep all environment variables
    --net-nft-rules <PATH>
                        Read and enforce nftables rules from <PATH>
    --unshare-user <UID> <GID>
                        Run within an additional layer of user namespace with
                        uid <UID> and gid <GID>
    --set-env <KEY> <VALUE>
                        Set environment variable <KEY> to <VALUE>
    --unset-env <KEY>   Unset environment variable <KEY>
    --ro-bind <SRC> <DEST>
                        Bind mount <SRC> to <DEST> as read-only
    --rw-bind <SRC> <DEST>
                        Bind mount <SRC> to <DEST> as read-write
    --dev-bind <SRC> <DEST>
                        Bind mount <SRC> to <DEST> as read-write and allow
                        device access
    --symlink <SRC> <DEST>
                        Create a symlink to <SRC> at <DEST>
    --dir <DEST>        Create a directory at <DEST>
"#};

struct Args {
    no_die_with_parent: bool,
    no_new_scope: bool,
    container: Container,
}

fn parse_bind<A>(option_name: &str, args: &mut A) -> Option<Bind>
where
    A: Iterator<Item = OsString>,
{
    let src = some_or!(
        args.next(),
        msg_ret!("{} requires 2 arguments", option_name)
    );
    let dest = some_or!(
        args.next(),
        msg_ret!("{} requires 2 arguments", option_name)
    );
    Some(Bind { src, dest })
}

fn parse_mount<A>(option_name: &str, args: &mut A) -> Option<Mount>
where
    A: Iterator<Item = OsString>,
{
    let path = some_or!(
        args.next(),
        msg_ret!("{} requires an argument", option_name)
    );
    Some(Mount { path })
}

fn handle_args_or_run_inner() -> Option<Args> {
    let mut args = env::args_os().peekable();
    some_or!(args.next(), msg_ret!("Argument required"));

    if let Some(arg) = args.peek() {
        if arg == "--inner" {
            inner::run();
        }
    }

    let mut no_die_with_parent = false;
    let mut no_new_scope = false;
    let mut container = Container::default();
    let mut command: Vec<OsString> = Vec::new();

    while let Some(arg) = args.next() {
        macro_rules! parse_bind {
            ($name: expr, $st: ident) => {{
                container
                    .options
                    .push(Options::$st(some_or_ret!(parse_bind($name, &mut args))));
            }};
        }
        macro_rules! parse_mount {
            ($name: expr, $st: ident) => {{
                container
                    .options
                    .push(Options::$st(some_or_ret!(parse_mount($name, &mut args))));
            }};
        }
        if &arg == "--help" {
            println!("{HELP_MESSAGE}");
            process::exit(0);
        } else if &arg == "--no-die-with-parent" {
            no_die_with_parent = true;
        } else if &arg == "--no-new-scope" {
            no_new_scope = true;
        } else if &arg == "--share-net" {
            container.share_net = true;
        } else if &arg == "--share-time" {
            container.share_time = true;
        } else if &arg == "--keep-env" {
            container.keep_env = true;
        } else if &arg == "--net-nft-rules" {
            let path = some_or!(
                args.next(),
                msg_ret!("--net-nft-rules requires an argument")
            );
            // TODO: Limit rule length
            let rules = ok_or!(fs::read(path), msg_ret!("Failed to read nft rules"));
            container.net_nft_rules = rules;
        } else if &arg == "--unshare-user" {
            let uid = some_or!(args.next(), msg_ret!("--unshare-user requires 2 arguments"));
            let uid = some_or!(
                (uid.into_string().ok()).and_then(|x| x.parse().ok()),
                msg_ret!("Invalid uid")
            );
            let gid = some_or!(args.next(), msg_ret!("--unshare-user requires 2 arguments"));
            let gid = some_or!(
                (gid.into_string().ok()).and_then(|x| x.parse().ok()),
                msg_ret!("Invalid gid")
            );
            container.unshare_user = Some((uid, gid));
        } else if &arg == "--set-env" {
            let key = some_or!(args.next(), msg_ret!("--set-env requires 2 arguments"));
            let value = some_or!(args.next(), msg_ret!("--set-env requires 2 arguments"));
            container
                .options
                .push(Options::SetEnv(SetEnv { key, value }));
        } else if &arg == "--unset-env" {
            container.options.push(Options::UnsetEnv(some_or!(
                args.next(),
                msg_ret!("--unset-env requires an argument")
            )));
        } else if &arg == "--ro-bind" {
            parse_bind!("--ro-bind", RoBind);
        } else if &arg == "--rw-bind" {
            parse_bind!("--rw-bind", Bind);
        } else if &arg == "--dev-bind" {
            parse_bind!("--dev-bind", DevBind);
        } else if &arg == "--symlink" {
            parse_bind!("--symlink", Symlink);
        } else if &arg == "--dir" {
            parse_mount!("--dir", Dir);
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
        command = vec!["/bin/bash".into()];
    }
    container.command = command;

    Some(Args {
        no_die_with_parent,
        no_new_scope,
        container,
    })
}

pub fn run() -> ExitCode {
    let env = env::vars_os().collect::<Vec<_>>();
    let args = some_or!(handle_args_or_run_inner(), return ExitCode::FAILURE);
    if !args.no_die_with_parent {
        true_or!(
            set_die_with_parent(),
            msg_and!("Failed to set die-with-parent"; return ExitCode::FAILURE)
        );
    }
    if !args.no_new_scope {
        return run_in_scope();
    }

    let exit_status = some_or!(
        start_container(&args.container, &env),
        return ExitCode::FAILURE
    );
    exit_status
        .code()
        .map(|c| ((((c % 256) + 256) % 256) as u8).into())
        .unwrap_or(ExitCode::FAILURE)
}
