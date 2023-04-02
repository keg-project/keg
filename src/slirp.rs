use std::ffi::OsStr;
use std::io;
use std::process::{Child, Command, Stdio};

pub fn slirp<I, S>(args: I) -> io::Result<Child>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new("/usr/bin/slirp4netns")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}
