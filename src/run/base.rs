use super::utils::run_in_scope;
use crate::container::{start_container, Bind, Container, Mount, Options, SetEnv};
use crate::die_with_parent::set_die_with_parent;
use crate::run::inner;
use crate::{msg_and, msg_ret, ok_or, some_or, some_or_ret, true_or};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::process::ExitCode;

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

fn parse_args_or_run_inner() -> Option<Args> {
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
    let mut command: Option<Vec<OsString>> = None;

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
        if &arg == "--no-die-with-parent" {
            no_die_with_parent = true;
        } else if &arg == "--no-new-scope" {
            no_new_scope = true;
        } else if &arg == "--share-net" {
            container.share_net = true;
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
            command = Some(Vec::new());
            if !arg.as_bytes().starts_with(b"-") {
                command.as_mut().unwrap().push(arg);
            }
            while let Some(arg) = args.next() {
                command.as_mut().unwrap().push(arg);
            }
            break;
        } else {
            msg_ret!("Unknown argument {}", arg.to_string_lossy());
        }
    }
    let command = match command {
        Some(x) => x,
        None => vec![env::var_os("SHELL").unwrap_or("/bin/sh".into())],
    };
    true_or!(!command.is_empty(), msg_ret!("Must specify command"));
    container.command = command;

    Some(Args {
        no_die_with_parent,
        no_new_scope,
        container,
    })
}

pub fn run() -> ExitCode {
    let env = env::vars_os().collect::<Vec<_>>();
    let args = some_or!(parse_args_or_run_inner(), return ExitCode::FAILURE);
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
