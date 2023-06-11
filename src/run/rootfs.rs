use super::utils::run_in_scope;
use crate::container::{start_container, Bind, Container, Mount, Options, SetEnv};
use crate::die_with_parent::set_die_with_parent;
use crate::filesystem;
use crate::overlayfs;
use crate::run::inner;
use crate::{msg_and, msg_ret, ok_or, some_or, true_or};
use indoc::indoc;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::{self, ExitCode};

static HELP_MESSAGE: &'static str = indoc! {r#"
Usage: [OPTIONS] [--] [COMMAND]...

Arguments:
    [COMMAND]...        Command and arguments to run in the container. If
                        empty, /bin/bash will be used.

Options:
    --help              Display this message and exit
    --no-die-with-parent
                        Do not kill child processes when this process dies
    --no-new-scope      Do not run in a new systemd scope
    -r <DIR>            Use <DIR> as the root directory. By default, /bin,
                        /etc, /lib, /opt, /sbin, /usr, /var, and /lib64 (if
                        /lib64 is available) will be made available in the
                        container root directory.
    -l <DIR>            Add <DIR> as a layer of lower directory. The layer is
                        applied after the root directory and previous lower
                        directories. This option can appear multiple times.
    -u <DIR>            Use <DIR> as the upper directory. This layer is
                        applied after the root and lower directories. The
                        upper directory will contain a tree directory and a
                        work directory. The default is "container".
    --tree <PATH>       Use <PATH> in the upper directory as the tree
                        directory. <PATH> must be a relative path. The default
                        is "tree".
    --work <PATH>       Use <PATH> in the upper directory as the work
                        directory. <PATH> must be a relative path. The default
                        is "work".
    -m                  Mount host root to /mnt before running podman. /mnt
                        will be available to podman (but not to the
                        container).
    --share-net         Enable network
    --share-time        Share time namespace
    --net-nft-rules <PATH>
                        Read and enforce nftables rules from <PATH>
    -a <ARG>            Append <ARG> as an argument to the podman. This can be
                        used to make additional changes to the container.
"#};

struct Args {
    no_die_with_parent: bool,
    no_new_scope: bool,
    root_dir: Option<OsString>,
    lower_dirs: Vec<OsString>,
    upper_dir: OsString,
    tree: OsString,
    work: OsString,
    mount_root: bool,
    container: Container,
    container_args: Vec<OsString>,
    command: Vec<OsString>,
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
    let mut root_dir = None;
    let mut lower_dirs = Vec::new();
    let mut upper_dir = "container".into();
    let mut tree = "tree".into();
    let mut work = "work".into();
    let mut mount_root = false;
    let mut container = Container::default();
    let mut container_args: Vec<OsString> = Vec::new();
    let mut command = Vec::new();

    while let Some(arg) = args.next() {
        if &arg == "--help" {
            println!("{HELP_MESSAGE}");
            process::exit(0);
        } else if &arg == "--no-die-with-parent" {
            no_die_with_parent = true;
        } else if &arg == "--no-new-scope" {
            no_new_scope = true;
        } else if &arg == "-r" {
            root_dir = Some(some_or!(args.next(), msg_ret!("-r requires an argument")));
        } else if &arg == "-l" {
            lower_dirs.push(some_or!(args.next(), msg_ret!("-l requires an argument")));
        } else if &arg == "-u" {
            upper_dir = some_or!(args.next(), msg_ret!("-u requires an argument"));
        } else if &arg == "--tree" {
            tree = some_or!(args.next(), msg_ret!("--tree requires an argument"));
        } else if &arg == "--work" {
            work = some_or!(args.next(), msg_ret!("--work requires an argument"));
        } else if &arg == "-m" {
            mount_root = true;
        } else if &arg == "--share-net" {
            container.share_net = true;
        } else if &arg == "--share-time" {
            container.share_time = true;
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
        upper_dir,
        tree,
        work,
        mount_root,
        container,
        container_args,
        command,
    })
}

pub fn run() -> ExitCode {
    let env = env::vars_os().collect::<Vec<_>>();
    let mut args = some_or!(handle_args_or_run_inner(), return ExitCode::FAILURE);
    if !args.no_die_with_parent {
        true_or!(
            set_die_with_parent(),
            msg_and!("Failed to set die-with-parent"; return ExitCode::FAILURE)
        );
    }
    if !args.no_new_scope {
        return run_in_scope();
    }

    true_or!(
        Path::new(&args.tree).is_relative(),
        msg_and!("--tree must specify a relative path"; return ExitCode::FAILURE)
    );
    true_or!(
        Path::new(&args.work).is_relative(),
        msg_and!("--work must specify a relative path"; return ExitCode::FAILURE)
    );

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
    args.container.options.push(Options::Bind(Bind {
        src: args.upper_dir.clone(),
        dest: "/container_overlay_upper".into(),
    }));
    args.container.options.push(Options::Dir(Mount {
        path: "/container_rootfs".into(),
    }));

    let overlay_command = some_or!(
        overlayfs::get_command(
            container_lowers.iter().map(|x| &x[..]),
            OsStr::new(&Path::new("/container_overlay_upper").join(&args.tree)),
            OsStr::new(&Path::new("/container_overlay_upper").join(&args.work)),
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

    if !Path::new(&args.upper_dir).exists() {
        ok_or!(
            fs::create_dir(&args.upper_dir),
            msg_and!(
                "Failed to create directory \"{}\"", Path::new(&args.upper_dir).display();
                return ExitCode::FAILURE
            )
        );
    }
    let upper_tree = Path::new(&args.upper_dir).join(&args.tree);
    if !upper_tree.exists() {
        ok_or!(
            fs::create_dir(&upper_tree),
            msg_and!(
                "Failed to create directory \"{}\"", upper_tree.display();
                return ExitCode::FAILURE
            )
        );
    }
    let upper_work = Path::new(&args.upper_dir).join(&args.work);
    if !upper_work.exists() {
        ok_or!(
            fs::create_dir(&upper_work),
            msg_and!(
                "Failed to create directory \"{}\"", upper_work.display();
                return ExitCode::FAILURE
            )
        );
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
