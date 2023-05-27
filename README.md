# Keg

[![Crates.io](https://img.shields.io/crates/v/keg)](https://crates.io/crates/keg)
[![Build Status](https://github.com/keg-project/keg/actions/workflows/ci.yml/badge.svg)](https://github.com/keg-project/keg/actions)

Keg is a portable container without boilerplate.

* Keg is rootless and runs as a regular user.
* Keg doesn't create any hidden directories in `~` and doesn't read/write any file unprompted
  except the current directory.

You can:

* Use your current `/` as the base image and exclude sensitive paths like `/home`. Or import your
  own rootfs for complete isolation.
* Make `/` appear writable in your container with changes either kept in memory, or written to
  another directory, using `overlayfs`.
* Add firewall rules to the container with `nftables`.

Under the hood, Keg runs a Podman container in a separate Linux namespace. Keg isolation is secure
as long as Podman is secure.

## Examples

> **Warning**
>
> If you get an error such as `Cannot run [...]: Operation not permitted (os error 1)`, your kernel
> may have [this bug]. You need to append `--share-time` to all Keg container commands.

[this bug]: https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/commit/?id=2b5f9dad32ed19e8db3b0f10a84aa824a219803b

1. Map `/bin, /etc, /lib, /lib64, /opt, /sbin, /usr, /var` into the container and map current
   directory to `/root/workspace`. All paths will appear writable, but only changes to
   `/root/workspace` are permanent:

   ```sh
   keg --share-net
   ```

2. Map `/bin, /etc, /lib, /lib64, /opt, /sbin, /usr, /var` into the container. All paths will
   appear writable, but changes are written to paths inside `./container`:

   ```sh
   keg-rootfs --share-net
   ```

3. Map `./root` into the container as `/`. Changes are written to paths inside `./my_container`:

   ```sh
   keg-rootfs --share-net -r ./root -u ./my_container
   ```

--------

In addition to all the above, use `--net-nft-rules ./nftables_rules.txt` to import firewall rules
from `./nftables_rules.txt`. Remove `--share-net` to disable network access in the container.

You will appear as `root` (uid 0) in the container. As per usual, this does not give you global
root. Some applications require a non-root user to function correctly. If that's the case, run

```sh
keg-user
```

within the container to create a new user namespace as a non-root user. You can optionally specify
`--uid <uid>` and `--gid <gid>`.

## Installation

Keg works as long as all dependencies listed below are installed:

bubblewrap >= 0.4.0, fuse-overlayfs >= 1.5, libseccomp >= 2.4, linux >= 5.4.0, nftables >= 0.9.3,
podman >= 3.4.2, slirp4netns >= 1.1.8

### Installation Examples

#### Ubuntu >= 22.04

Run the following commands and reboot:
```sh
sudo apt-get update -y
sudo apt-get -qq -y install bubblewrap fuse-overlayfs libseccomp-dev nftables podman slirp4netns

# If not installed already:
sudo apt-get -qq -y install cargo
export PATH="$HOME/.cargo/bin:$PATH"
echo 'PATH="$HOME/.cargo/bin:$PATH"' >> ~/.profile

cargo install keg
```

#### Ubuntu 20.04

First, follow [these instructions] to install `podman`. Then execute the following and reboot:
```sh
sudo apt-get -qq -y install bubblewrap fuse-overlayfs libseccomp-dev nftables slirp4netns

# If not installed already:
sudo apt-get -qq -y install cargo
export PATH="$HOME/.cargo/bin:$PATH"
echo 'PATH="$HOME/.cargo/bin:$PATH"' >> ~/.profile

cargo install keg
```

[these instructions]: https://www.atlantic.net/dedicated-server-hosting/how-to-install-and-use-podman-on-ubuntu-20-04/
