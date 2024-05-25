mod exec;
mod isolation;
mod mounting;
mod utils;

use crate::keyring;
use crate::seccomp;
use crate::{msg_ret, true_or};
use libc::{gid_t, pid_t, uid_t};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::process::ExitStatus;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct SetEnv {
    pub key: OsString,
    pub value: OsString,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Bind {
    pub src: OsString,
    pub dest: OsString,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Mount {
    pub path: OsString,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum Options {
    SetEnv(SetEnv),
    UnsetEnv(OsString),
    // Bind
    Bind(Bind),
    DevBind(Bind),
    RoBind(Bind),
    Symlink(Bind),
    // Mount
    Dir(Mount),
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Container {
    pub share_net: bool,
    pub share_time: bool,
    pub keep_env: bool,
    pub base_image: Option<OsString>,
    pub net_nft_rules: Vec<u8>,
    pub unshare_user: Option<(uid_t, gid_t)>,
    pub options: Vec<Options>,
    pub create_dummy_files: bool,
    pub command_before_unshare_user: Vec<OsString>,
    pub command: Vec<OsString>,
}

impl Default for Container {
    fn default() -> Self {
        Self {
            share_net: false,
            share_time: false,
            keep_env: false,
            base_image: None,
            net_nft_rules: Vec::new(),
            unshare_user: None,
            options: Vec::new(),
            create_dummy_files: false,
            command_before_unshare_user: Vec::new(),
            command: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum Stage {
    Isolation(u8),
    Mounting,
    Exec,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ContainerRunner {
    pub stage: Stage,
    pub container: Container,
    pub env: Vec<(OsString, OsString)>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ContainerRunnerResponse {
    pub pid: pid_t,
}

pub fn run_container(
    stage: Stage,
    container: &Container,
    env: &[(OsString, OsString)],
    wait: bool,
) -> Option<ExitStatus> {
    // Stages:
    // Isolation:
    // stage 0: container: cap_setfcap, cap_sys_admin, share net, bind /dev/net/tun.
    // stage 1: unshare time. container: cap_setfcap, cap_sys_admin, share pid, bind /dev/net/tun. run slirp.
    // stage 2: unshare time. container: cap_setfcap, cap_sys_admin, share net, bind /dev/net/tun.
    // stage 3: unshare time. container: cap_setfcap, cap_sys_admin, cap_net_admin, share pid, bind /dev/net/tun. run slirp.
    // stage 4: unshare time. bwrap(nft, cap_net_admin). container: cap_setfcap, cap_sys_admin, share net, bind /dev/net/tun.
    // stage 5: unshare time. container: cap_setfcap, cap_sys_admin, share pid. run slirp.
    // stage 6: unshare time. container: cap_setfcap, cap_sys_admin, share net.
    // Mounting.
    // Exec: Set env. Exec.

    match stage {
        Stage::Isolation(stage) => isolation::run_container(stage, container, env, wait),
        Stage::Mounting => mounting::run_container(container, env, wait),
        Stage::Exec => exec::run_container(container, env, wait),
    }
}

pub fn start_container(container: &Container, env: &[(OsString, OsString)]) -> Option<ExitStatus> {
    true_or!(seccomp::apply(), msg_ret!("Failed to apply seccomp rules"));
    true_or!(
        keyring::apply(),
        msg_ret!("Failed to join new keyring session")
    );
    run_container(Stage::Isolation(0), container, env, true)
}
