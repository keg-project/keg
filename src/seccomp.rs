use crate::ok_or;
use libc;
use libseccomp::{ScmpAction, ScmpArch, ScmpArgCompare, ScmpCompareOp, ScmpFilterContext};

pub fn apply() -> bool {
    let mut filter = ok_or!(
        ScmpFilterContext::new_filter(ScmpAction::Allow),
        return false
    );
    #[cfg(target_arch = "x86_64")]
    ok_or!(filter.add_arch(ScmpArch::X86), return false);
    #[cfg(target_arch = "aarch64")]
    ok_or!(filter.add_arch(ScmpArch::Arm), return false);
    ok_or!(
        filter.add_rule(
            ScmpAction::Errno(libc::EPERM),
            ok_or!(i32::try_from(libc::SYS_add_key), return false)
        ),
        return false
    );
    ok_or!(
        filter.add_rule(
            ScmpAction::Errno(libc::EPERM),
            ok_or!(i32::try_from(libc::SYS_request_key), return false)
        ),
        return false
    );
    ok_or!(
        filter.add_rule_conditional(
            ScmpAction::Errno(libc::EPERM),
            ok_or!(i32::try_from(libc::SYS_ioctl), return false),
            &[ScmpArgCompare::new(
                1,
                ScmpCompareOp::MaskedEqual(0xffffffff),
                ok_or!(u64::try_from(libc::TIOCSTI), return false)
            )],
        ),
        return false
    );
    ok_or!(
        filter.add_rule_conditional(
            ScmpAction::Errno(libc::EPERM),
            ok_or!(i32::try_from(libc::SYS_ioctl), return false),
            &[ScmpArgCompare::new(
                1,
                ScmpCompareOp::MaskedEqual(0xffffffff),
                ok_or!(u64::try_from(libc::TIOCLINUX), return false)
            )],
        ),
        return false
    );
    filter.load().is_ok()
}
