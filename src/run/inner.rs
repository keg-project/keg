use crate::container::{run_container, ContainerRunner, ContainerRunnerResponse};
use crate::socket_pair::set_cloexec;
use crate::{msg_ret, ok_or, true_or};
use bincode;
use libc::c_int;
use std::env;
use std::io::{Read, Write};
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UnixStream;
use std::process::{self, ExitStatus};

fn read_stream(stream: &mut UnixStream) -> Option<ExitStatus> {
    let runner: ContainerRunner = ok_or!(
        bincode::deserialize_from(&mut *stream),
        msg_ret!("Deserialization failure")
    );
    let response = ContainerRunnerResponse {
        pid: ok_or!(process::id().try_into(), return None),
    };
    ok_or!(
        stream.write_all(&ok_or!(
            bincode::serialize(&response),
            msg_ret!("Send response failure")
        )),
        return None
    );
    ok_or!(stream.read_exact(&mut [0u8]), return None);
    // We can manage our own cgroup at this point, which is required for `run_container`.

    run_container(runner.stage, &runner.container, &runner.env, true)
}

pub fn run() -> ! {
    let mut args = env::args_os();
    for _ in 0..2 {
        if args.next().is_none() {
            process::exit(1);
        }
    }
    let sock = match args.next() {
        Some(sock) => sock,
        None => process::exit(1),
    };
    let sock: c_int = match sock.into_string() {
        Ok(sock) => match sock.parse() {
            Ok(sock) => sock,
            Err(_) => process::exit(1),
        },
        Err(_) => process::exit(1),
    };
    true_or!(unsafe { set_cloexec(sock) }, process::exit(1));

    let mut stream = unsafe { UnixStream::from_raw_fd(sock) };
    let exit_code = if let Some(exit_status) = read_stream(&mut stream) {
        exit_status
            .code()
            .map(|c| ((((c % 256) + 256) % 256) as u8).into())
            .unwrap_or(1)
    } else {
        1
    };
    process::exit(exit_code)
}
