use super::utils::{ro_bind_subentries_keep_symlinks, CLONE_NEWTIME};
use super::{Bind, Container, ContainerRunner, ContainerRunnerResponse, Mount, Options, Stage};
use crate::bwrap::bwrap;
use crate::cgroup::{cgroup_init, cgroup_postexec, cgroup_preexec};
use crate::socket_pair::{set_cloexec, socket_pair};
use crate::{msg_ret, ok_or, some_or, true_or};
use libc::{close, unshare};
use std::ffi::OsString;
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

pub fn run_container(
    container: &Container,
    env: &[(OsString, OsString)],
    wait: bool,
) -> Option<ExitStatus> {
    true_or!(cgroup_init(false), return None);

    if !container.share_time {
        true_or!(unsafe { unshare(CLONE_NEWTIME) } == 0, return None);
    }

    let mut args = Vec::<OsString>::new();
    args.push("--unshare-user".into());
    args.push("--unshare-ipc".into());
    args.push("--unshare-pid".into());
    args.push("--unshare-uts".into());
    args.push("--unshare-cgroup".into());
    args.push("--uid".into());
    args.push("0".into());
    args.push("--gid".into());
    args.push("0".into());
    args.push("--hostname".into());
    args.push("container".into());
    args.push("--chdir".into());
    args.push("/".into());
    args.push("--die-with-parent".into());
    args.push("--cap-drop".into());
    args.push("all".into());
    args.push("--cap-add".into());
    args.push("all".into());

    args.append(&mut ok_or!(
        ro_bind_subentries_keep_symlinks("/container_staging_image", "/"),
        msg_ret!("Failed binding staging image")
    ));

    args.push("--proc".into());
    args.push("/proc".into());
    args.push("--tmpfs".into());
    args.push("/tmp".into());
    args.push("--tmpfs".into());
    args.push("/run".into());
    args.push("--dir".into());
    args.push("/root".into());
    args.push("--dir".into());
    args.push("/home".into());
    args.push("--dir".into());
    args.push("/home/user".into());
    args.push("--dev".into());
    args.push("/dev".into());
    args.push("--mqueue".into());
    args.push("/dev/mqueue".into());
    args.push("--dev-bind".into());
    args.push("/dev/fuse".into());
    args.push("/dev/fuse".into());
    args.push("--dev-bind".into());
    args.push("/dev/net/tun".into());
    args.push("/dev/net/tun".into());
    args.push("--tmpfs".into());
    args.push("/sys".into());

    let mut bind_index: u64 = 0;
    for option in &container.options {
        match option {
            Options::Symlink(Bind { src, dest }) => {
                args.push("--symlink".into());
                args.push(src.to_owned().into());
                args.push(dest.to_owned().into());
            }
            Options::Dir(Mount { path }) => {
                args.push("--dir".into());
                args.push(path.to_owned().into());
            }
            _ => (),
        }
        let bind = match option {
            Options::Bind(bind) => Some(bind),
            Options::DevBind(bind) => Some(bind),
            Options::RoBind(bind) => Some(bind),
            _ => None,
        };
        match option {
            Options::Bind(_) => args.push("--bind".into()),
            Options::DevBind(_) => args.push("--dev-bind".into()),
            Options::RoBind(_) => args.push("--ro-bind".into()),
            _ => (),
        }
        if let Some(Bind { src: _, dest }) = bind {
            args.push(("/container_bind_".to_owned() + &bind_index.to_string()).into());
            args.push(dest.to_owned().into());
            bind_index += 1;
        }
    }

    args.push("--tmpfs".into());
    args.push("/sys/fs/cgroup".into());

    let (mut stream, sock) = some_or!(socket_pair(), msg_ret!("Cannot create socket pair"));
    // TODO: Close the other socket on error

    args.push("--".into());
    args.push("/keg-bin".into());
    args.push("--inner".into());
    args.push(sock.to_string().into());

    true_or!(cgroup_preexec(false), return None);
    let result = bwrap(args, true);
    true_or!(unsafe { set_cloexec(sock) }, return None);
    unsafe { close(sock) };

    let mut child = match result {
        Ok(child) => child,
        Err(e) => {
            eprintln!("Cannot run bwrap: {e}");
            return None;
        }
    };

    let mut container_clone = Container::default();
    container_clone.unshare_user = container.unshare_user;
    container_clone.command_before_unshare_user = container.command_before_unshare_user.clone();
    container_clone.command = container.command.clone();

    let runner = ContainerRunner {
        stage: Stage::Exec,
        container: container_clone,
        env: env.to_owned(),
    };
    ok_or!(
        stream.write_all(&ok_or!(bincode::serialize(&runner), return None)),
        return None
    );
    let _: ContainerRunnerResponse = ok_or!(bincode::deserialize_from(&mut stream), return None);
    true_or!(cgroup_postexec(false), return None);
    ok_or!(stream.write_all(&[0u8]), return None);

    if wait {
        child.wait().ok()
    } else {
        Some(ExitStatus::from_raw(0))
    }
}
