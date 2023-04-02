# Keg

## Dependencies

`keg` works as long as all dependencies listed below are installed:

bubblewrap >= 0.4.0, fuse-overlayfs >= 1.5, linux >= 5.4.0, nftables >= 0.9.3, podman >= 3.4.2,
slirp4netns >= 1.1.8

### Examples

#### Ubuntu >= 22.04

Run the following commands:
```sh
sudo apt-get update -y
sudo apt-get -qq -y install bubblewrap fuse-overlayfs nftables podman slirp4netns
```

#### Ubuntu 20.04

First, follow [these instructions] to install `podman`. Then execute:
```sh
sudo apt-get -qq -y install bubblewrap fuse-overlayfs nftables slirp4netns
```

[these instructions]: https://podman.io/blogs/2021/06/16/install-podman-on-ubuntu.html
