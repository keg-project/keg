#!/bin/sh

set -e

echo Running tests. Make sure no other test is running.
echo ""
rm -rf target/test_container
if [ -e target ]; then
    if [ -e target/test_container ]; then
        echo \"target/test_container\" exists.
        exit 1
    fi
fi

cargo run --bin keg-base -- $KEG_BASE_TEST_ARGS -- /bin/true
! cargo run --bin keg-base -- $KEG_BASE_TEST_ARGS -- /bin/false
cargo run --bin keg-rootfs -- -u target/test_container $KEG_ROOTFS_TEST_ARGS -- /bin/true
! cargo run --bin keg-rootfs -- -u target/test_container $KEG_ROOTFS_TEST_ARGS -- /bin/false

cargo run --bin keg -- $KEG_TEST_ARGS -- sh tests/include/true.sh
! cargo run --bin keg -- $KEG_TEST_ARGS -- sh tests/include/false.sh
cargo run --bin keg-home -- $KEG_HOME_TEST_ARGS -- sh tests/include/true.sh
! cargo run --bin keg-home -- $KEG_HOME_TEST_ARGS -- sh tests/include/false.sh
cargo run --bin keg-user -- $KEG_USER_TEST_ARGS -- sh tests/include/true.sh
! cargo run --bin keg-user -- $KEG_USER_TEST_ARGS -- sh tests/include/false.sh

cargo run --bin keg-rootfs -- -u target/test_container $KEG_ROOTFS_TEST_ARGS -- /bin/sh -c "printf '#!/bin/sh\ntrue\n' > /usr/local/bin/mytrue && chmod +x /usr/local/bin/mytrue"
cargo run --bin keg-rootfs -- -u target/test_container $KEG_ROOTFS_TEST_ARGS -- /bin/sh -c "printf '#!/bin/sh\nfalse\n' > /usr/local/bin/myfalse && chmod +x /usr/local/bin/myfalse"
cargo run --bin keg-rootfs -- -u target/test_container $KEG_ROOTFS_TEST_ARGS -- /bin/sh -c mytrue
! cargo run --bin keg-rootfs -- -u target/test_container $KEG_ROOTFS_TEST_ARGS -- /bin/sh -c myfalse

rm -rf target/test_container
if [ -e target ]; then
    if [ -e target/test_container ]; then
        echo \"target/test_container\" still exists.
        exit 1
    fi
fi
