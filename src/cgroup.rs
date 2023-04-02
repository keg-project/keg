/// cgroup v2 support.
use crate::{msg_ret, msg_retf, ok_or, some_or, true_or};
use libc::{c_char, c_void, mount, umount, MS_SILENT};
use std::ffi::{CString, OsStr, OsString};
use std::fs::{create_dir, read, read_link, remove_dir, write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

#[must_use]
pub fn cgroup_init(stage0: bool) -> bool {
    if stage0 {
        cgroup_init_stage0()
    } else {
        cgroup_init_stage_inner()
    }
}

#[must_use]
pub fn cgroup_preexec(stage0: bool) -> bool {
    if stage0 {
        cgroup_preexec_stage0()
    } else {
        cgroup_preexec_stage_inner()
    }
}

#[must_use]
pub fn cgroup_postexec(stage0: bool) -> bool {
    if stage0 {
        cgroup_postexec_stage0()
    } else {
        cgroup_postexec_stage_inner()
    }
}

pub fn mount_cgroup<P: AsRef<Path>>(path: P) -> bool {
    let path = ok_or!(
        CString::new(path.as_ref().as_os_str().to_owned().into_vec()),
        return false
    );
    unsafe {
        mount(
            b"none\0".as_ptr() as *const c_char,
            path.as_bytes().as_ptr() as *const c_char,
            b"cgroup2\0".as_ptr() as *const c_char,
            MS_SILENT,
            b"nsdelegate\0".as_ptr() as *const c_void,
        ) == 0
    }
}

fn umount_cgroup<P: AsRef<Path>>(path: P) -> bool {
    let path = ok_or!(
        CString::new(path.as_ref().as_os_str().to_owned().into_vec()),
        return false
    );
    unsafe { umount(path.as_bytes().as_ptr() as *const c_char) == 0 }
}

/// proc may be "0" to refer to self.
fn move_one(proc: &[u8], to: &Path) -> bool {
    ok_or!(
        write(to.join("cgroup.procs"), proc),
        msg_retf!("Failed to move cgroup process")
    );
    true
}

fn get_cgroup_ns(proc: &[u8]) -> Option<Vec<u8>> {
    let mut path = b"/proc/".to_vec();
    path.extend_from_slice(proc);
    path.extend_from_slice(&b"/ns/cgroup"[..]);
    let path = OsString::from_vec(path);
    Some(
        ok_or!(read_link(path), msg_ret!("Cannot read cgroup ns"))
            .into_os_string()
            .into_vec(),
    )
}

fn move_all(from: &Path, to: &Path) -> bool {
    loop {
        let procs = ok_or!(
            read(from.join("cgroup.procs")),
            msg_retf!("Cannot read cgroup.procs")
        );
        let procs = procs.split(|c| c == &b'\n');
        let mut empty = true;
        for proc in procs {
            if proc.is_empty() {
                continue;
            }
            empty = false;
            if proc == b"0" {
                msg_retf!("Cannot move cgroup process 0");
            }
            true_or!(move_one(proc, to), return false);
        }
        if empty {
            return true;
        }
    }
}

fn move_all_matching_ns(from: &Path, to: &Path, ns: &[u8]) -> bool {
    loop {
        let procs = ok_or!(
            read(from.join("cgroup.procs")),
            msg_retf!("Cannot read cgroup.procs")
        );
        let procs = procs.split(|c| c == &b'\n');
        let mut moved = false;
        for proc in procs {
            if proc.is_empty() {
                continue;
            }
            if proc == b"0" {
                msg_retf!("Cannot move cgroup process 0");
            }
            let this_ns = some_or!(get_cgroup_ns(proc), return false);
            if this_ns == ns {
                moved = true;
                true_or!(move_one(proc, to), return false);
            }
        }
        if !moved {
            return true;
        }
    }
}

fn add_subtree_control(cgroup: &Path) -> bool {
    let mut controllers = ok_or!(
        read(cgroup.join("cgroup.controllers")),
        msg_retf!("Cannot read cgroup.controllers")
    );
    if controllers.last() == Some(&b'\n') {
        controllers.pop();
    }
    if !controllers.is_empty() {
        let controllers = controllers.split(|c| c == &b' ');

        let mut subtree_control = Vec::new();
        for controller in controllers {
            if subtree_control.is_empty() {
                subtree_control.extend_from_slice(&b"+"[..]);
            } else {
                subtree_control.extend_from_slice(&b" +"[..]);
            }
            subtree_control.extend_from_slice(&controller);
        }
        ok_or!(
            write(cgroup.join("cgroup.subtree_control"), subtree_control),
            msg_retf!("Cannot write to cgroup.subtree_control")
        );
    }
    true
}

fn get_cgroup_root_stage0() -> Option<PathBuf> {
    let mut entries = ok_or!(
        read("/proc/self/cgroup"),
        msg_ret!("Cannot read /proc/self/cgroup")
    );
    if entries.last() == Some(&b'\n') {
        entries.pop();
    }
    for entry in entries.split(|x| x == &b'\n') {
        if entry.starts_with(&b"0::"[..]) {
            let cgroup = Path::new(OsStr::from_bytes(&entry[b"0::".len()..]));
            let cgroup = ok_or!(
                cgroup.strip_prefix("/"),
                msg_ret!("cgroup path is not in the current namespace")
            );

            let mut cgroup_root = Path::new("/sys/fs/cgroup/unified");
            if !cgroup_root.exists() {
                cgroup_root = Path::new("/sys/fs/cgroup");
            }
            return Some(cgroup_root.join(cgroup));
        }
    }
    msg_ret!("Only cgroup v2 is supported");
}

fn cgroup_init_stage0() -> bool {
    let cgroup = PathBuf::from(some_or!(get_cgroup_root_stage0(), return false));

    let parent = cgroup.join("unit.container_parent");
    let children = cgroup.join("unit.container_children");
    let spawn = cgroup.join("unit.container_spawn");
    let other = cgroup.join("unit.container_other");
    ok_or!(create_dir(&parent), msg_retf!("Cannot create cgroup"));
    ok_or!(create_dir(&children), msg_retf!("Cannot create cgroup"));
    ok_or!(create_dir(&spawn), msg_retf!("Cannot create cgroup"));
    ok_or!(create_dir(&other), msg_retf!("Cannot create cgroup"));
    true_or!(move_all(&cgroup, &other), return false);
    true_or!(move_one(&b"0"[..], &spawn), return false);
    true_or!(add_subtree_control(&cgroup), return false);

    true
}

fn cgroup_preexec_stage0() -> bool {
    let mut cgroup = PathBuf::from(some_or!(get_cgroup_root_stage0(), return false));
    true_or!(cgroup.pop(), msg_retf!("cgroup path changed"));

    let children = cgroup.join("unit.container_children");
    true_or!(move_one(&b"0"[..], &children), return false);

    true
}

fn cgroup_postexec_stage0() -> bool {
    let mut cgroup = PathBuf::from(some_or!(get_cgroup_root_stage0(), return false));
    true_or!(cgroup.pop(), msg_retf!("cgroup path changed"));

    let parent = cgroup.join("unit.container_parent");
    let children = cgroup.join("unit.container_children");
    let self_ns = some_or!(get_cgroup_ns(&b"self"[..]), return false);
    true_or!(
        move_all_matching_ns(&children, &parent, &self_ns),
        return false
    );

    true
}

fn cgroup_init_stage_inner() -> bool {
    let cgroup = Path::new("/container_cgroup");
    ok_or!(create_dir(&cgroup), return false);
    true_or!(mount_cgroup(&cgroup), {
        drop(remove_dir(&cgroup));
        msg_retf!("Cannot mount cgroup");
    });

    let parent = cgroup.join("unit.container_parent");
    let children = cgroup.join("unit.container_children");
    let spawn = cgroup.join("unit.container_spawn");
    let other = cgroup.join("unit.container_other");
    ok_or!(create_dir(&parent), msg_retf!("Cannot create cgroup"));
    ok_or!(create_dir(&children), msg_retf!("Cannot create cgroup"));
    ok_or!(create_dir(&spawn), msg_retf!("Cannot create cgroup"));
    ok_or!(create_dir(&other), msg_retf!("Cannot create cgroup"));
    true_or!(move_all(&cgroup, &other), return false);
    true_or!(move_one(&b"0"[..], &spawn), return false);
    true_or!(add_subtree_control(&cgroup), return false);

    true_or!(umount_cgroup(&cgroup), msg_retf!("Cannot unmount cgroup"));
    ok_or!(remove_dir(&cgroup), return false);
    true
}

fn cgroup_preexec_stage_inner() -> bool {
    let cgroup = Path::new("/container_cgroup");
    ok_or!(create_dir(&cgroup), return false);
    true_or!(mount_cgroup(&cgroup), {
        drop(remove_dir(&cgroup));
        msg_retf!("Cannot mount cgroup");
    });

    let children = cgroup.join("unit.container_children");
    true_or!(move_one(&b"0"[..], &children), return false);

    true_or!(umount_cgroup(&cgroup), msg_retf!("Cannot unmount cgroup"));
    ok_or!(remove_dir(&cgroup), return false);
    true
}

fn cgroup_postexec_stage_inner() -> bool {
    let cgroup = Path::new("/container_cgroup");
    ok_or!(create_dir(&cgroup), return false);
    true_or!(mount_cgroup(&cgroup), {
        drop(remove_dir(&cgroup));
        msg_retf!("Cannot mount cgroup");
    });

    let parent = cgroup.join("unit.container_parent");
    let children = cgroup.join("unit.container_children");
    let self_ns = some_or!(get_cgroup_ns(b"self"), return false);
    true_or!(
        move_all_matching_ns(&children, &parent, &self_ns),
        return false
    );

    true_or!(umount_cgroup(&cgroup), msg_retf!("Cannot unmount cgroup"));
    ok_or!(remove_dir(&cgroup), return false);
    true
}

pub fn cgroup_init_stage_exec() -> bool {
    let cgroup = Path::new("/sys/fs/cgroup");

    let spawn = cgroup.join("unit.container_spawn");
    let other = cgroup.join("unit.container_other");
    ok_or!(create_dir(&spawn), msg_retf!("Cannot create cgroup"));
    ok_or!(create_dir(&other), msg_retf!("Cannot create cgroup"));
    true_or!(move_all(&cgroup, &other), return false);
    true_or!(move_one(&b"0"[..], &spawn), return false);
    true_or!(add_subtree_control(&cgroup), return false);

    true
}
