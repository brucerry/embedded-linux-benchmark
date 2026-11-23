# Storage, network, and automatic stress

Use `cargo build --release --locked` and run `./target/release/embedded-linux-benchmark --help` for the commands. All workloads run on the calling thread. The `cpu` and `ram` commands also run independently with their existing settings.

## Storage

Select an existing writable directory and a size of 1–64 MiB. The command creates a uniquely named regular file with owner-only permissions, writes the requested payload sequentially, then reads and verifies the same byte count. Exclusive creation protects existing files. The command removes the scratch file on success or a handled error and reports the path if removal fails.

```sh
./target/release/embedded-linux-benchmark storage --dir /path/to/writable-directory --size-mib 16
./target/release/embedded-linux-benchmark storage --dir /path/to/writable-directory --size-mib 16 --sync
```

The write and read rates are separate. Writes go through buffered filesystem I/O. With `--sync`, write timing includes `sync_data()` on the file. Reads are page-cache eligible and include payload verification. The output measures this filesystem path, including its caching and writeback behavior. The size budget covers application payload bytes, and filesystem metadata and device writeback can add physical I/O. `--timeout-ms` accepts 1–60000 and defaults to 10000.

## TCP loopback

```sh
./target/release/embedded-linux-benchmark network --size-mib 8 --timeout-ms 5000
```

The command connects two TCP sockets through a listener bound to `127.0.0.1` on an ephemeral port. Timing starts after connection setup and covers one-way application throughput, including receiving and verifying the payload. The result covers the local TCP stack. Physical NIC throughput requires a separate measurement. The command uses the system's existing loopback configuration. `--size-mib` accepts 1–64 and defaults to 8. `--timeout-ms` accepts 1–30000 and defaults to 5000.

## Automatic stress

```sh
./target/release/embedded-linux-benchmark stress auto
./target/release/embedded-linux-benchmark stress auto --rounds 2 --duration-ms 500 --network-mib 4
./target/release/embedded-linux-benchmark stress auto --rounds 2 \
  --storage-dir /path/to/writable-directory --storage-mib 16 --storage-sync
```

Each round runs CPU, RAM, and loopback in sequence. Storage runs after loopback when both `--storage-dir` and `--storage-mib` are supplied. `--storage-mib` is the total 1–64 MiB write budget across all rounds. The suite divides it among rounds, with any remaining bytes in the final round. `--storage-sync` is optional with storage enabled.

| Option | Default | Accepted values |
| --- | --- | --- |
| `--rounds` | 3 | 1–5 |
| `--duration-ms` | 250 per CPU or RAM phase | 1–5000 |
| `--buffer-kib` | 1024 per RAM buffer | 4–32768 |
| `--network-mib` | 8 per round | 1–64 |
| `--timeout-ms` | 15000 for the full suite | 1–120000, greater than the combined CPU and RAM durations |

The suite validates settings before starting, prints each completed component's outcome, and stops at the first failure. A failed component or expired deadline produces a nonzero exit status. The RAM working set is bounded by two buffers, with a maximum of 64 MiB. Storage and network use fixed-size application buffers.

Deadlines are checked during the run between CPU batches, RAM copies, I/O chunks, and phases. An in-progress filesystem call such as `sync_data()`, time spent waiting for the OS scheduler, or allocation and page faults can delay the next check. Sudden termination, including default `SIGINT` handling, or power loss can leave a `.embedded-linux-benchmark-*` file in the selected directory.

## Validation

The native Debian 13 release tests cover component output, loopback byte counts, invalid limits and incomplete storage opt-ins, storage budget accounting, and scratch-file removal. A child process with a small file-size limit exercises a write failure both directly and in the stress suite. The default three-round run also passed without creating a scratch file.

| Current build | Execution evidence |
| --- | --- |
| OpenWrt 21.02.7 `armvirt/64` AArch64 IPK | Matching full-system QEMU install, suite checks, removal |
| OpenWrt 25.12.6 `armsr/armv8` AArch64 APK | Matching full-system QEMU install, suite checks, removal |
| OpenWrt 24.10.8 `malta/le` MIPS32 IPK | Matching full-system QEMU install, suite checks, removal |
| Jetson Linux 36.5.2 and 39.2.2 AArch64 GNU/Linux | QEMU user mode with each SDK sysroot and matching NVIDIA sample libraries |

The OpenWrt guests enabled loopback in the test setup. The Jetson tests used the host kernel, filesystem, and TCP stack through QEMU user mode. The SDK checks exercised CPU and RAM, storage with sync and cleanup, TCP loopback, and stress with and without opted-in storage. Physical-board measurements remain unverified. [OpenWrt SDK records](openwrt-compatibility.md) · [MIPS SDK records](openwrt-platforms.md) · [Jetson SDK records](jetson-linux.md)
