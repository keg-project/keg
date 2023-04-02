#[macro_export]
macro_rules! msg_and {
    ($fmt: expr $(, $arg: expr)*; $a: expr) => {{
        eprintln!($fmt, $($arg,)*);
        { $a }
    }}
}

#[macro_export]
macro_rules! msg_ret {
    ($fmt: expr $(, $arg: expr)*) => {{
        $crate::msg_and!($fmt $(, $arg)*; return None);
    }}
}

#[macro_export]
macro_rules! msg_retf {
    ($fmt: expr $(, $arg: expr)*) => {{
        $crate::msg_and!($fmt $(, $arg)*; return false);
    }}
}

#[macro_export]
macro_rules! true_or {
    ($cond: expr, $a: expr) => {{
        if !($cond) {
            $a
        }
    }};
}

#[macro_export]
macro_rules! some_or {
    ($e: expr, $a: expr) => {{
        match ($e) {
            Some(r) => r,
            None => $a,
        }
    }};
}

#[macro_export]
macro_rules! some_or_ret {
    ($e: expr) => {{
        match ($e) {
            Some(r) => r,
            None => return None,
        }
    }};
}

#[macro_export]
macro_rules! ok_or {
    ($e: expr, $a: expr) => {{
        match ($e) {
            Ok(r) => r,
            Err(_) => $a,
        }
    }};
}
