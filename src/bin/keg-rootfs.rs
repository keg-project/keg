use keg::run::rootfs;
use std::process::ExitCode;

fn main() -> ExitCode {
    rootfs::run()
}
