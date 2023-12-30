use super::utils::run_in_scope;
use crate::container::{start_container, Bind, Container, Mount, Options, SetEnv};
use crate::die_with_parent::set_die_with_parent;
use crate::filesystem;
use crate::overlayfs;
use crate::run::inner;
use crate::{msg_and, msg_ret, ok_or, some_or, some_or_ret, true_or};
use indoc::indoc;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;
use std::process::{self, ExitCode};

macro_rules! help_message_part0 {
    () => {
        indoc! {r#"
Usage: [OPTIONS] [--] [COMMAND]...

Arguments:
    [COMMAND]...        Command and arguments to run in the container. If
                        empty, /bin/bash will be used.

Options:
    --help              Display this message and exit
    --no-die-with-parent
                        Do not kill child processes when this process dies
    --no-new-scope      Do not run in a new systemd scope
    -b <PATH>           Use <PATH> as the trusted base image, instead of the
                        default system directories
    --share-net         Enable network
    --share-time        Share time namespace
    --net-nft-rules <PATH>
                        Read and enforce nftables rules from <PATH>. This file
                        will be loaded into memory and keg does not limit its
                        size.
    -r <DIR>            Use <DIR> as the root directory. By default, /bin,
                        /etc, /lib, /opt, /sbin, /usr, /var, and /lib64 (if
                        /lib64 is available) will be made available in the
                        container root directory.
    -l <DIR>            Add <DIR> as a layer of lower directory. The layer is
                        applied after the root directory and previous lower
                        directories. This option can appear multiple times.
    -w <DIR>            Use <DIR> as the workspace directory. This directory
"#}
    };
}

macro_rules! help_message_part1 {
    () => {
        indoc! {r#"
.
    --ro-bind <SRC> <DEST>
                        Bind mount <SRC> to /mnt/<DEST> as read-only before
                        running podman
    --rw-bind <SRC> <DEST>
                        Bind mount <SRC> to /mnt/<DEST> as read-write before
                        running podman
    --dev-bind <SRC> <DEST>
                        Bind mount <SRC> to /mnt/<DEST> as read-write and
                        allow device access, before running podman
    -a <ARG>            Append <ARG> as an argument to the podman. This can be
                        used to make additional changes to the container.
"#}
    };
}

static HELP_MESSAGE_IF_WORKSPACE_IS_NOT_HOME: &'static str = concat! {
    help_message_part0!(),
r#"                        will be mounted at /root/workspace. The default is
                        ".""#,
    help_message_part1!(),
};
static HELP_MESSAGE_IF_WORKSPACE_IS_HOME: &'static str = concat! {
    help_message_part0!(),
r#"                        will be mounted at /root. The default is ".""#,
    help_message_part1!(),
};

struct Args {
    no_die_with_parent: bool,
    no_new_scope: bool,
    root_dir: Option<OsString>,
    lower_dirs: Vec<OsString>,
    workspace_dir: OsString,
    container: Container,
    net_nft_rules_path: Option<OsString>,
    container_args: Vec<OsString>,
    command: Vec<OsString>,
}

fn parse_bind<A>(option_name: &str, args: &mut A) -> Option<Bind>
where
    A: Iterator<Item = OsString>,
{
    let src = some_or!(
        args.next(),
        msg_ret!("{} requires 2 arguments", option_name)
    );
    let mut dest = some_or!(
        args.next(),
        msg_ret!("{} requires 2 arguments", option_name)
    );
    true_or!(
        !dest.as_bytes().contains(&b'/'),
        msg_ret!("Bind destination cannot contain \"/\"")
    );
    true_or!(
        !dest.as_bytes().contains(&b'\0'),
        msg_ret!("Bind destination cannot contain the nul byte")
    );
    true_or!(
        dest.as_bytes() != &b"."[..],
        msg_ret!("Bind destination cannot be \".\"")
    );
    true_or!(
        dest.as_bytes() != &b".."[..],
        msg_ret!("Bind destination cannot be \"..\"")
    );
    true_or!(
        !dest.is_empty(),
        msg_ret!("Bind destination cannot be empty")
    );
    dest = OsString::from_vec([&b"/mnt/"[..], dest.as_bytes()].concat());
    Some(Bind { src, dest })
}

fn handle_args_or_run_inner(workspace_is_home: bool) -> Option<Args> {
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
    let mut container = Container::default();
    let mut net_nft_rules_path = None;
    let mut container_args: Vec<OsString> = Vec::new();
    let mut command = Vec::new();

    while let Some(arg) = args.next() {
        macro_rules! parse_bind {
            ($name: expr, $st: ident) => {{
                container
                    .options
                    .push(Options::$st(some_or_ret!(parse_bind($name, &mut args))));
            }};
        }
        if &arg == "--help" {
            if !workspace_is_home {
                println!("{HELP_MESSAGE_IF_WORKSPACE_IS_NOT_HOME}");
            } else {
                println!("{HELP_MESSAGE_IF_WORKSPACE_IS_HOME}");
            }
            process::exit(0);
        } else if &arg == "--no-die-with-parent" {
            no_die_with_parent = true;
        } else if &arg == "--no-new-scope" {
            no_new_scope = true;
        } else if &arg == "-b" {
            container.base_image = Some(some_or!(args.next(), msg_ret!("-b requires an argument")));
        } else if &arg == "-r" {
            root_dir = Some(some_or!(args.next(), msg_ret!("-r requires an argument")));
        } else if &arg == "-l" {
            lower_dirs.push(some_or!(args.next(), msg_ret!("-l requires an argument")));
        } else if &arg == "-w" {
            workspace_dir = some_or!(args.next(), msg_ret!("-w requires an argument"));
        } else if &arg == "--ro-bind" {
            parse_bind!("--ro-bind", RoBind);
        } else if &arg == "--rw-bind" {
            parse_bind!("--rw-bind", Bind);
        } else if &arg == "--dev-bind" {
            parse_bind!("--dev-bind", DevBind);
        } else if &arg == "--share-net" {
            container.share_net = true;
        } else if &arg == "--share-time" {
            container.share_time = true;
        } else if &arg == "--net-nft-rules" {
            net_nft_rules_path = Some(some_or!(
                args.next(),
                msg_ret!("--net-nft-rules requires an argument")
            ));
        } else if &arg == "-a" {
            container_args.push(some_or!(args.next(), msg_ret!("-a requires an argument")));
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

    Some(Args {
        no_die_with_parent,
        no_new_scope,
        root_dir,
        lower_dirs,
        workspace_dir,
        container,
        net_nft_rules_path,
        container_args,
        command,
    })
}

pub fn run(workspace_is_home: bool) -> ExitCode {
    let env = env::vars_os().collect::<Vec<_>>();
    let mut args = some_or!(
        handle_args_or_run_inner(workspace_is_home),
        return ExitCode::FAILURE
    );
    if !args.no_die_with_parent {
        true_or!(
            set_die_with_parent(),
            msg_and!("Failed to set die-with-parent"; return ExitCode::FAILURE)
        );
    }
    if !args.no_new_scope {
        return run_in_scope();
    }

    if let Some(path) = args.net_nft_rules_path {
        let rules = ok_or!(
            fs::read(path),
            msg_and!("Failed to read nft rules"; return ExitCode::FAILURE)
        );
        args.container.net_nft_rules = rules;
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
    args.container.options.push(Options::SetEnv(SetEnv {
        key: "TMPDIR".into(),
        value: "/tmp".into(),
    }));
    args.container.options.push(Options::DevBind(Bind {
        src: "/dev/null".into(),
        dest: "/etc/subuid".into(),
    }));
    args.container.options.push(Options::DevBind(Bind {
        src: "/dev/null".into(),
        dest: "/etc/subgid".into(),
    }));

    if let Some(root_dir) = args.root_dir {
        args.container.options.push(Options::RoBind(Bind {
            src: root_dir,
            dest: Path::new("/container_overlay_lower_0").into(),
        }));
    } else {
        let r = filesystem::iterate(true, |file_name, symlink| match symlink {
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
    // cap_sys_chroot: https://github.com/containers/podman/issues/17504
    args.container.command.push("--cap-add".into());
    args.container.command.push("sys_chroot".into());
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
