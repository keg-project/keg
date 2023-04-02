use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};

fn escape_options(opt: &OsStr) -> OsString {
    let opt = opt.as_bytes();
    let mut ret: Vec<u8> = Vec::with_capacity(opt.len());
    for b in opt {
        if b == &b'\\' {
            ret.extend_from_slice(&b"\\\\"[..]);
        } else if b == &b',' {
            ret.extend_from_slice(&b"\\,"[..]);
        } else if b == &b':' {
            ret.extend_from_slice(&b"\\:"[..]);
        } else {
            ret.push(*b);
        }
    }
    OsString::from_vec(ret)
}

pub fn get_command<'a, L>(
    lowerdirs: L,
    upperdir: &OsStr,
    workdir: &OsStr,
    merged: &OsStr,
) -> Option<Vec<OsString>>
where
    L: IntoIterator<Item = &'a OsStr>,
{
    // This string has to be escaped.
    let mut mount_options: OsString = "squash_to_root,lowerdir=".into();
    // Lower
    let mut has_lowerdir = false;
    for (i, lower) in lowerdirs.into_iter().enumerate() {
        has_lowerdir = true;
        if i > 0 {
            mount_options.push(":");
        }
        if lower.as_bytes().contains(&b':') {
            // fuse-overlayfs does not support escaping ':'
            return None;
        }
        mount_options.push(escape_options(lower));
    }
    if !has_lowerdir {
        return None;
    }
    // Upper
    mount_options.push(",upperdir=");
    if upperdir.as_bytes().contains(&b':') {
        return None;
    }
    mount_options.push(escape_options(upperdir));
    // Work
    mount_options.push(",workdir=");
    if workdir.as_bytes().contains(&b':') {
        return None;
    }
    mount_options.push(escape_options(workdir));

    Some(vec![
        OsStr::new("/usr/bin/fuse-overlayfs").to_owned(),
        OsStr::new("-o").to_owned(),
        mount_options,
        merged.to_owned(),
    ])
}
