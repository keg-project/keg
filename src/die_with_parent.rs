use libc::{prctl, PR_SET_PDEATHSIG, SIGKILL};

pub fn set_die_with_parent() -> bool {
    unsafe { prctl(PR_SET_PDEATHSIG, SIGKILL, 0, 0, 0) == 0 }
}
