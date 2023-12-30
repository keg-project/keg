use super::utils::{ro_bind_subentries_keep_symlinks, CLONE_NEWTIME};
use super::{Bind, Container, ContainerRunner, ContainerRunnerResponse, Options, SetEnv, Stage};
use crate::bwrap::bwrap;
use crate::cgroup::{cgroup_init, cgroup_postexec, cgroup_preexec};
use crate::filesystem;
use crate::slirp::slirp;
use crate::socket_pair::{set_cloexec, socket_pair};
use crate::{msg_and, msg_ret, ok_or, some_or, true_or};
use bincode;
use libc::{close, unshare};
use std::borrow::Cow;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::ExitStatus;

fn run_slirp(_container: &Container, response: &ContainerRunnerResponse) -> bool {
    let (mut slirp_stream, slirp_sock) = some_or!(
        socket_pair(),
        msg_and!("Cannot create socket pair"; return false)
    );

    let mut args = Vec::<OsString>::new();
    args.push("--configure".into());
    args.push("--ready-fd".into());
    args.push(slirp_sock.to_string().into());
    args.push("--enable-ipv6".into());
    args.push("--disable-host-loopback".into());
    args.push(response.pid.to_string().into());
    args.push("tap0".into());

    let result = slirp(args).map(|_| ());
    true_or!(unsafe { set_cloexec(slirp_sock) }, return false);
    unsafe { close(slirp_sock) };

    ok_or!(
        result,
        msg_and!("Failed to run slirp4netns: {}", result.unwrap_err(); return false)
    );
    ok_or!(
        slirp_stream.read_exact(&mut [0u8]),
        msg_and!("slirp init failed"; return false)
    );
    true
}

fn process_env(container: &Container, env: &[(OsString, OsString)]) -> Vec<(OsString, OsString)> {
    let mut env_map = HashMap::new();
    env_map.insert(
        "PATH".into(),
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
    );

    if container.keep_env {
        env_map = HashMap::with_capacity(env.len());
        for (k, v) in env {
            env_map.insert(k.to_owned(), v.to_owned());
        }
    }
    for option in &container.options {
        match option {
            Options::SetEnv(SetEnv { key, value }) => {
                env_map.insert(key.to_owned(), value.to_owned());
            }
            Options::UnsetEnv(key) => {
                env_map.remove(key);
            }
            _ => (),
        };
    }
    env_map.into_iter().collect()
}

fn cleanup_container(container: &mut Container) {
    container.keep_env = false;
    container.base_image = None;
    container.options.retain_mut(|option| match option {
        Options::SetEnv(_) | Options::UnsetEnv(_) => false,
        Options::Bind(Bind { src, dest: _ }) => {
            *src = "".into();
            true
        }
        Options::DevBind(Bind { src, dest: _ }) => {
            *src = "".into();
            true
        }
        Options::RoBind(Bind { src, dest: _ }) => {
            *src = "".into();
            true
        }
        Options::Symlink(_) => true,
        Options::Dir(_) => true,
    });
}

fn run_nft(rules: &Path) -> bool {
    let mut args = Vec::<OsString>::new();
    args.push("--unshare-ipc".into());
    args.push("--unshare-pid".into());
    args.push("--unshare-uts".into());
    args.push("--unshare-cgroup".into());
    args.push("--uid".into());
    args.push("0".into());
    args.push("--gid".into());
    args.push("0".into());
    args.push("--hostname".into());
    args.push("".into());
    args.push("--chdir".into());
    args.push("/".into());
    args.append(&mut ok_or!(
        ro_bind_subentries_keep_symlinks("/container_staging_image", "/"),
        msg_and!("Failed binding staging image"; return false)
    ));
    args.push("--ro-bind".into());
    args.push(rules.to_owned().into());
    args.push("/container_net_nft_rules".into());
    args.push("--die-with-parent".into());
    args.push("--cap-drop".into());
    args.push("all".into());
    args.push("--cap-add".into());
    args.push("cap_net_admin".into());
    args.push("--".into());

    args.push("/usr/sbin/nft".into());
    args.push("-f".into());
    args.push("/container_net_nft_rules".into());

    let exit_status = ok_or!(ok_or!(bwrap(args, true), return false).wait(), return false);
    true_or!(exit_status.success(), return false);
    true
}

pub fn ro_bind_filesystem<D>(dest: D) -> io::Result<Vec<OsString>>
where
    D: AsRef<Path>,
{
    let dest: &Path = dest.as_ref();

    let mut result = Vec::new();
    filesystem::iterate(false, |file_name, symlink| match symlink {
        None => {
            result.push("--ro-bind".into());
            result.push(Path::new("/").join(file_name).into());
            result.push(dest.join(file_name).into());
        }
        Some(symlink) => {
            result.push("--symlink".into());
            result.push(symlink.into());
            result.push(dest.join(file_name).into());
        }
    })?;
    Ok(result)
}

pub fn run_container(
    stage: u8,
    container: &Container,
    env: &[(OsString, OsString)],
    wait: bool,
) -> Option<ExitStatus> {
    true_or!(cgroup_init(stage == 0), return None);

    let env: Cow<_> = if stage == 0 {
        Cow::Owned(process_env(container, env))
    } else {
        Cow::Borrowed(env)
    };

    if !container.share_time && stage > 0 {
        true_or!(unsafe { unshare(CLONE_NEWTIME) } == 0, return None);
    }
    if stage == 4 {
        // Load nft rules and **make sure** the load succeeds.
        ok_or!(
            fs::write("/container_net_nft_rules", &container.net_nft_rules),
            return None
        );
        let result = run_nft(Path::new("/container_net_nft_rules"));
        ok_or!(fs::remove_file("/container_net_nft_rules"), return None);
        true_or!(result, return None);
    }

    let mut args = Vec::<OsString>::new();
    args.push("--unshare-user".into());
    args.push("--unshare-ipc".into());
    if stage != 1 && stage != 3 && stage != 5 {
        args.push("--unshare-pid".into());
    }
    if !container.share_net || (stage != 0 && stage != 2 && stage != 4 && stage != 6) {
        args.push("--unshare-net".into());
    }
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
    args.push("cap_setfcap".into());
    args.push("--cap-add".into());
    args.push("cap_sys_admin".into());
    if stage == 3 {
        args.push("--cap-add".into());
        args.push("cap_net_admin".into());
    }

    if stage == 0 {
        if let Some(base_image) = &container.base_image {
            args.append(&mut ok_or!(
                ro_bind_subentries_keep_symlinks(base_image, "/"),
                msg_ret!("Failed binding staging image")
            ));
            args.append(&mut ok_or!(
                ro_bind_subentries_keep_symlinks(base_image, "/container_staging_image"),
                msg_ret!("Failed binding staging image")
            ));
        } else {
            args.append(&mut ok_or!(
                ro_bind_filesystem("/"),
                msg_ret!("Failed binding staging image")
            ));
            args.append(&mut ok_or!(
                ro_bind_filesystem("/container_staging_image"),
                msg_ret!("Failed binding staging image")
            ));
        }
        let current_exe = ok_or!(env::current_exe(), msg_ret!("Failed getting current exe"));
        args.push("--ro-bind".into());
        args.push(current_exe.clone().into());
        args.push("/keg-bin".into());
        args.push("--ro-bind".into());
        args.push(current_exe.into());
        args.push("/container_staging_image/keg-bin".into());
    } else {
        args.append(&mut ok_or!(
            ro_bind_subentries_keep_symlinks("/container_staging_image", "/"),
            msg_ret!("Failed binding staging image")
        ));
        args.append(&mut ok_or!(
            ro_bind_subentries_keep_symlinks(
                "/container_staging_image",
                "/container_staging_image"
            ),
            msg_ret!("Failed binding staging image")
        ));
    }

    args.push("--proc".into());
    args.push("/proc".into());
    args.push("--tmpfs".into());
    args.push("/tmp".into());
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

    let mut bind_index: u64 = 0;
    for option in &container.options {
        // Binds
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
        if let Some(Bind { src, dest: _ }) = bind {
            if stage == 0 {
                args.push(src.clone());
            } else {
                args.push(("/container_bind_".to_owned() + &bind_index.to_string()).into());
            }
            args.push(("/container_bind_".to_owned() + &bind_index.to_string()).into());
            bind_index += 1;
        }
    }
    let (mut stream, sock) = some_or!(socket_pair(), msg_ret!("Cannot create socket pair"));
    // TODO: Close the other socket on error

    args.push("--".into());
    args.push("/keg-bin".into());
    args.push("--inner".into());
    args.push(sock.to_string().into());

    true_or!(cgroup_preexec(stage == 0), return None);
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

    let mut container_clone = container.clone();
    if stage == 0 {
        // Remove information we already applied.
        cleanup_container(&mut container_clone);
    }
    if stage == 4 {
        // nft rules already applied.
        container_clone.net_nft_rules = Vec::new();
    }
    let runner = ContainerRunner {
        stage: if stage <= 5 {
            Stage::Isolation(stage + 1)
        } else {
            Stage::Mounting
        },
        container: container_clone,
        env: env.into_owned(),
    };
    ok_or!(
        stream.write_all(&ok_or!(bincode::serialize(&runner), return None)),
        return None
    );
    let response: ContainerRunnerResponse =
        ok_or!(bincode::deserialize_from(&mut stream), return None);
    true_or!(cgroup_postexec(stage == 0), return None);
    if stage == 1 || stage == 3 || stage == 5 {
        true_or!(run_slirp(&container, &response), return None);
    }
    ok_or!(stream.write_all(&[0u8]), return None);

    if wait {
        child.wait().ok()
    } else {
        Some(ExitStatus::from_raw(0))
    }
}
