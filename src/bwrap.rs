use std::ffi::OsStr;
use std::io;
use std::process::{Child, Command};

pub fn bwrap<I, S>(args: I, env_clear: bool) -> io::Result<Child>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    // TODO: fd args
    let mut command = Command::new("/usr/bin/bwrap");
    command.args(args);
    if env_clear {
        command.env_clear();
    }
    command.spawn()
}
