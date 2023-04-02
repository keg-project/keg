use libc::{c_int, close, fcntl, socketpair, AF_UNIX, FD_CLOEXEC, F_GETFD, F_SETFD, SOCK_STREAM};
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UnixStream;

pub unsafe fn set_cloexec(fd: c_int) -> bool {
    let flags = fcntl(fd, F_GETFD);
    if flags < 0 {
        return false;
    }
    if fcntl(fd, F_SETFD, flags | FD_CLOEXEC) == -1 {
        return false;
    }
    true
}

/// Creates a socket pair for communication with a child.
pub fn socket_pair() -> Option<(UnixStream, c_int)> {
    let mut socks: [c_int; 2] = [0, 0];
    let stream;
    unsafe {
        if socketpair(AF_UNIX, SOCK_STREAM, 0, socks.as_mut_ptr()) != 0 {
            return None;
        }
        stream = UnixStream::from_raw_fd(socks[0]);
        if !set_cloexec(socks[0]) {
            close(socks[1]);
            return None;
        }
    }
    Some((stream, socks[1]))
}
