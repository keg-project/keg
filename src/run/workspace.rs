use super::utils::run_in_scope;
use crate::container::{start_container, Bind, Container, Mount, Options, SetEnv};
use crate::die_with_parent::set_die_with_parent;
use crate::filesystem;
use crate::overlayfs;
use crate::run::inner;
use crate::{msg_and, msg_ret, ok_or, some_or, true_or};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::ExitCode;

struct Args {
    no_die_with_parent: bool,
    no_new_scope: bool,
    root_dir: Option<OsString>,
    lower_dirs: Vec<OsString>,
    workspace_dir: OsString,
    mount_root: bool,
    container: Container,
    container_args: Vec<OsString>,
    command: Vec<OsString>,
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
    let mut root_dir = None;
    let mut lower_dirs = Vec::new();
    let mut workspace_dir = ".".into();
    let mut mount_root = false;
    let mut container = Container::default();
    let mut container_args: Vec<OsString> = Vec::new();
    let mut command = Vec::new();

    while let Some(arg) = args.next() {
        if &arg == "--no-die-with-parent" {
            no_die_with_parent = true;
        } else if &arg == "--no-new-scope" {
            no_new_scope = true;
        } else if &arg == "-r" {
            root_dir = Some(some_or!(args.next(), msg_ret!("-r requires an argument")));
        } else if &arg == "-l" {
            lower_dirs.push(some_or!(args.next(), msg_ret!("-l requires an argument")));
        } else if &arg == "-w" {
            workspace_dir = some_or!(args.next(), msg_ret!("-w requires an argument"));
        } else if &arg == "-m" {
            mount_root = true;
        } else if &arg == "--share-net" {
            container.share_net = true;
        } else if &arg == "--net-nft-rules" {
            let path = some_or!(
                args.next(),
                msg_ret!("--net-nft-rules requires an argument")
            );
            // TODO: Limit rule length
            let rules = ok_or!(fs::read(path), msg_ret!("Failed to read nft rules"));
            container.net_nft_rules = rules;
        } else if &arg == "-a" {
            container_args.push(some_or!(args.next(), msg_ret!("-a requires an argument")));
        } else if &arg == "--" || !arg.as_bytes().starts_with(b"-") {
            if !arg.as_bytes().starts_with(b"-") {
                command.push(arg);
            }
            while let Some(arg) = args.next() {
                command.push(arg);
            }
            break;
        } else {
            msg_ret!("Unknown argument {}", arg.to_string_lossy());
        }
    }

    Some(Args {
        no_die_with_parent,
        no_new_scope,
        root_dir,
        lower_dirs,
        workspace_dir,
        mount_root,
        container,
        container_args,
        command,
    })
}

pub fn run(workspace_is_home: bool) -> ExitCode {
    let env = env::vars_os().collect::<Vec<_>>();
    let mut args = some_or!(parse_args_or_run_inner(), return ExitCode::FAILURE);
    if !args.no_die_with_parent {
        true_or!(
            set_die_with_parent(),
            msg_and!("Failed to set die-with-parent"; return ExitCode::FAILURE)
        );
    }
    if !args.no_new_scope {
        return run_in_scope();
    }

    args.container.unshare_user = Some((1000, 1000));
    args.container.options.push(Options::SetEnv(SetEnv {
        key: "USER".into(),
        value: "user".into(),
    }));
    args.container.options.push(Options::SetEnv(SetEnv {
        key: "HOME".into(),
        value: "/home/user".into(),
    }));
    if args.mount_root {
        args.container.options.push(Options::DevBind(Bind {
            src: "/".into(),
            dest: "/mnt".into(),
        }));
    }

    if let Some(root_dir) = args.root_dir {
        args.container.options.push(Options::RoBind(Bind {
            src: root_dir,
            dest: Path::new("/container_overlay_lower_0").into(),
        }));
    } else {
        let r = filesystem::iterate(|file_name, symlink| match symlink {
            None => args.container.options.push(Options::RoBind(Bind {
                src: Path::new("/").join(file_name).into(),
                dest: Path::new("/container_overlay_lower_0")
                    .join(file_name)
                    .into(),
            })),
            Some(symlink) => args.container.options.push(Options::Symlink(Bind {
                src: symlink.into(),
                dest: Path::new("/container_overlay_lower_0")
                    .join(file_name)
                    .into(),
            })),
        });
        if let Err(e) = r {
            msg_and!("Failed to iterate filesystem: {e}"; return ExitCode::FAILURE);
        }
    }

    let mut container_lowers = vec!["/container_overlay_lower_0".into()];
    for (i, lower) in args.lower_dirs.into_iter().enumerate() {
        let dest: OsString = format!("/container_overlay_lower_{}", i + 1).into();
        args.container.options.push(Options::RoBind(Bind {
            src: lower,
            dest: dest.clone(),
        }));
        container_lowers.push(dest);
    }
    args.container.options.push(Options::Dir(Mount {
        path: "/container_overlay_upper_tree".into(),
    }));
    args.container.options.push(Options::Dir(Mount {
        path: "/container_overlay_upper_work".into(),
    }));
    args.container.options.push(Options::Dir(Mount {
        path: "/container_rootfs".into(),
    }));
    args.container.options.push(Options::Bind(Bind {
        src: args.workspace_dir,
        dest: Path::new("/container_root_workspace").into(),
    }));

    let overlay_command = some_or!(
        overlayfs::get_command(
            container_lowers.iter().map(|x| &x[..]),
            OsStr::new("/container_overlay_upper_tree"),
            OsStr::new("/container_overlay_upper_work"),
            OsStr::new("/container_rootfs")
        ),
        msg_and!("Failed to get overlayfs command"; return ExitCode::FAILURE)
    );
    args.container.command_before_unshare_user = overlay_command;

    args.container.command.push("/usr/bin/podman".into());
    args.container.command.push("run".into());
    args.container.command.push("-i".into());
    args.container.command.push("-t".into());
    args.container
        .command
        .push("--mount=type=tmpfs,dst=/tmp".into());
    if !workspace_is_home {
        args.container
            .command
            .push("--mount=type=bind,src=/container_root_workspace,dst=/root/workspace".into());
        args.container.command.push("-w=/root/workspace".into());
    } else {
        args.container
            .command
            .push("--mount=type=bind,src=/container_root_workspace,dst=/root".into());
        args.container.command.push("-w=/root".into());
    }
    args.container.command.push("--rootfs".into());
    for arg in args.container_args {
        args.container.command.push(arg);
    }
    args.container.command.push("/container_rootfs".into());
    if args.command.is_empty() {
        args.container.command.push("/bin/bash".into());
    } else {
        for arg in args.command {
            args.container.command.push(arg);
        }
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