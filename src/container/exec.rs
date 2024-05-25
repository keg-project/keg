use super::Container;
use crate::cgroup::{cgroup_init_stage_exec, mount_cgroup};
use crate::{msg_ret, ok_or, true_or};
use core::ptr;
use libc::{c_char, execv, unshare, CLONE_NEWUSER};
use std::env;
use std::ffi::{CString, OsString};
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, ExitStatus};

pub fn run_container(
    container: &Container,
    env: &[(OsString, OsString)],
    wait: bool,
) -> Option<ExitStatus> {
    assert!(wait);

    if container.create_dummy_files {
        ok_or!(
            fs::write("/container_dummy_loadavg", b"1.00 1.00 1.00 1/100 1\n"),
            msg_ret!("Failed to write dummy loadavg")
        );
        ok_or!(
            fs::write(
                "/container_dummy_stat",
                b"cpu  0 0 0 0 0 0 0 0 0 0
cpu0 0 0 0 0 0 0 0 0 0 0
intr 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
ctxt 0
btime 100
processes 100
procs_running 1
procs_blocked 0
softirq 0 0 0 0 0 0 0 0 0 0 0
"
            ),
            msg_ret!("Failed to write dummy stat")
        );
        ok_or!(
            fs::write("/container_dummy_uptime", b"100.00 100.00\n"),
            msg_ret!("Failed to write dummy uptime")
        );
        ok_or!(
            fs::set_permissions(
                "/container_dummy_loadavg",
                fs::Permissions::from_mode(0o444)
            ),
            msg_ret!("Failed to chmod dummy loadavg")
        );
        ok_or!(
            fs::set_permissions("/container_dummy_stat", fs::Permissions::from_mode(0o444)),
            msg_ret!("Failed to chmod dummy stat")
        );
        ok_or!(
            fs::set_permissions("/container_dummy_uptime", fs::Permissions::from_mode(0o444)),
            msg_ret!("Failed to chmod dummy uptime")
        );
    }

    true_or!(
        mount_cgroup("/sys/fs/cgroup"),
        msg_ret!("Failed to mount cgroup")
    );
    true_or!(
        cgroup_init_stage_exec(),
        msg_ret!("Failed to initialize cgroup")
    );

    for (k, v) in env {
        if k.is_empty()
            || k.as_bytes().contains(&b'=')
            || k.as_bytes().contains(&b'\0')
            || v.as_bytes().contains(&b'\0')
        {
            msg_ret!("Invalid environment variable");
        }
        env::set_var(k, v);
    }

    if !container.command_before_unshare_user.is_empty() {
        let mut child = match Command::new(&container.command_before_unshare_user[0])
            .args(&container.command_before_unshare_user[1..])
            .spawn()
        {
            Err(e) => msg_ret!(
                "Failed to run command before unshare: Running `{}`: {}",
                Path::new(&container.command_before_unshare_user[0]).display(),
                e
            ),
            Ok(child) => child,
        };
        let status = match child.wait() {
            Err(e) => msg_ret!("Failed to wait for command before unshare: {e}"),
            Ok(status) => status,
        };
        if !status.success() {
            msg_ret!("Running command before unshare returned {status}");
        }
    }

    if let Some((uid, gid)) = container.unshare_user {
        true_or!(
            unsafe { unshare(CLONE_NEWUSER) } == 0,
            msg_ret!("Cannot create new user ns")
        );
        ok_or!(
            fs::write("/proc/self/uid_map", format!("{uid} 0 1\n")),
            msg_ret!("Cannot write to uid_map")
        );
        ok_or!(
            fs::write("/proc/self/setgroups", "deny"),
            msg_ret!("Cannot write to setgroups")
        );
        ok_or!(
            fs::write("/proc/self/gid_map", format!("{gid} 0 1\n")),
            msg_ret!("Cannot write to gid_map")
        );
    }

    true_or!(
        !container.command.is_empty(),
        msg_ret!("Command cannot be empty")
    );
    let mut argv_c = Vec::new();
    for arg in &container.command {
        argv_c.push(ok_or!(
            CString::new(arg.as_bytes().to_owned()),
            msg_ret!("Bad command")
        ));
    }
    let mut argv_ptr = Vec::new();
    for arg in &argv_c {
        argv_ptr.push(arg.as_bytes().as_ptr() as *const c_char);
    }
    argv_ptr.push(ptr::null());
    unsafe {
        execv(argv_ptr[0], argv_ptr.as_ptr());
    }
    // execv failed.
    let error = io::Error::last_os_error();
    msg_ret!(
        "execv failed: Running `{}`: {}",
        Path::new(&container.command[0]).display(),
        error
    );
}
