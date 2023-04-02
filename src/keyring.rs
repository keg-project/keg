use core::ptr;
use libc::{c_int, c_ulong, syscall, SYS_keyctl, KEYCTL_JOIN_SESSION_KEYRING};

pub fn apply() -> bool {
    unsafe {
        syscall(
            SYS_keyctl,
            KEYCTL_JOIN_SESSION_KEYRING as c_int,
            ptr::null_mut::<u8>() as usize as c_ulong,
            0 as c_ulong,
            0 as c_ulong,
            0 as c_ulong,
        ) != -1
    }
}
