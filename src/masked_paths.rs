pub fn podman_arg() -> &'static str {
    &concat!(
        "--security-opt=mask=",
        "/proc/acpi",
        ":",
        "/proc/asound",
        ":",
        "/proc/bootconfig",
        ":",
        "/proc/buddyinfo",
        ":",
        "/proc/bus",
        ":",
        "/proc/cgroups",
        ":",
        "/proc/cmdline",
        ":",
        "/proc/consoles",
        ":",
        "/proc/crypto",
        ":",
        "/proc/devices",
        ":",
        "/proc/diskstats",
        ":",
        "/proc/dma",
        ":",
        "/proc/driver",
        ":",
        "/proc/dynamic_debug",
        ":",
        "/proc/fb",
        ":",
        "/proc/filesystems",
        ":",
        "/proc/fs",
        ":",
        "/proc/interrupts",
        ":",
        "/proc/iomem",
        ":",
        "/proc/ioports",
        ":",
        "/proc/irq",
        ":",
        "/proc/kcore",
        ":",
        "/proc/key-users",
        ":",
        "/proc/keys",
        ":",
        "/proc/latency_stats",
        ":",
        "/proc/meminfo",
        ":",
        "/proc/misc",
        ":",
        "/proc/modules",
        ":",
        "/proc/partitions",
        ":",
        "/proc/sched_debug",
        ":",
        "/proc/schedstat",
        ":",
        "/proc/scsi",
        ":",
        "/proc/softirqs",
        ":",
        "/proc/swaps",
        ":",
        "/proc/sys",
        ":",
        "/proc/timer_list",
        ":",
        "/proc/timer_stats",
        ":",
        "/proc/tty",
        ":",
        "/proc/vmstat",
        ":",
        "/proc/zoneinfo",
    )
}