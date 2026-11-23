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

By default each round runs CPU, RAM, and loopback in sequence. Supplying both `--storage-dir` and `--storage-mib` adds storage after loopback. `--storage-mib` is the total 1–64 MiB write budget across all rounds. The suite divides it among rounds, with any remaining bytes in the final round. `--storage-sync` is optional with storage enabled.

Use one `--test` selector to repeat a single component. Each selection accepts `--rounds` and `--timeout-ms`. CPU and RAM accept `--duration-ms`, RAM accepts `--buffer-kib`, and network accepts `--network-mib`. Storage-only requires both the directory and total write budget and accepts `--storage-sync`. Options for other components produce an error before the run starts.

```sh
./target/release/embedded-linux-benchmark stress auto --test cpu --rounds 3 --duration-ms 500
./target/release/embedded-linux-benchmark stress auto --test ram --buffer-kib 2048
./target/release/embedded-linux-benchmark stress auto --test network --network-mib 4
./target/release/embedded-linux-benchmark stress auto --test storage --rounds 2 \
  --storage-dir /path/to/writable-directory --storage-mib 16 --storage-sync
```

| Option | Default | Accepted values |
| --- | --- | --- |
| `--rounds` | 3 | 1–5 |
| `--duration-ms` | 250 per CPU or RAM phase | 1–5000 |
| `--buffer-kib` | 1024 per RAM buffer | 4–32768 |
| `--network-mib` | 8 per round | 1–64 |
| `--timeout-ms` | 15000 for the full run | 1–120000, greater than the total duration of all scheduled CPU and RAM phases |

### Elapsed-time runs

Use `--duration-secs` for a foreground run with a total elapsed target from 1 through 1209600 seconds (14 days). It applies to the suite or any individual `--test`. This mode replaces `--rounds` and `--timeout-ms`. `--duration-ms` still sets the CPU or RAM phase length, and the last phase may run for less time as the deadline approaches.

```sh
./target/release/embedded-linux-benchmark stress auto --duration-secs 60
./target/release/embedded-linux-benchmark stress auto --test cpu --duration-secs 1209600
./target/release/embedded-linux-benchmark stress auto --test ram --duration-secs 300 --duration-ms 500
./target/release/embedded-linux-benchmark stress auto --test storage --duration-secs 300 \
  --storage-dir /path/to/writable-directory --storage-mib 16
```

Elapsed-time runs keep fixed-size totals, emit progress at most once every five minutes, and print the measured elapsed time and stop reason. CPU and RAM totals include shortened measured phases. Network and verified storage totals include completed operations. The storage write count includes bytes written before a cutoff. Loopback operations retain their configured byte cap and a five-second timeout. Each storage operation uses at most 1 MiB of the explicit total write budget and a ten-second deadline. These operation deadlines are also limited by the remaining run time.

The storage write budget is shared across the entire run at any duration. The suite announces when it reaches that budget and continues the other selected phases. Storage-only finishes early after verifying its final write. The stress suite measures workload activity and leaves thermal and throttling controls to the operating system and firmware.

The suite validates settings before starting and stops at the first failure. An elapsed-time run succeeds when it reaches its requested duration or completes a storage-only write budget. A failed component, a component operation timeout before the run deadline, or a round-based run exceeding its timeout produces a nonzero exit status. The RAM working set is bounded by two buffers, with a maximum of 64 MiB. Storage and network use fixed-size application buffers.

Deadlines are checked during the run between CPU batches, RAM copies, I/O chunks, and phases. An in-progress filesystem call such as `sync_data()`, time spent waiting for the OS scheduler, or allocation and page faults can delay the next check. Sudden termination, including default `SIGINT` handling, or power loss can leave a `.embedded-linux-benchmark-*` file in the selected directory.

## Validation

The native Debian 13 release tests cover component output, loopback byte counts, invalid limits and incomplete storage opt-ins, storage budget accounting, and scratch-file removal. A child process with a small file-size limit exercises a write failure both directly and in the stress suite. The default three-round run also passed without creating a scratch file.

All four individual selectors passed native CLI tests, an OpenWrt 25.12.6 AArch64 full-system QEMU test, and Jetson Linux 39.2.2 QEMU user-mode checks with the matching NVIDIA sample libraries. These runs checked the selected phase count, storage byte accounting and cleanup, and rejection of conflicting settings.

Short elapsed-time runs passed on native Debian 13, in matching OpenWrt 25.12.6 AArch64 and 24.10.8 MIPS32 little-endian QEMU guests, and under QEMU user mode with Jetson Linux 39.2.2 sample libraries. They exercised deadline clamping, the shared storage budget, storage-only early exit, and scratch cleanup. The 14-day boundary and overflow were tested through argument parsing. The full duration was not run.

| Current build | Execution evidence |
| --- | --- |
| OpenWrt 21.02.7 `armvirt/64` AArch64 IPK | Matching full-system QEMU install, suite checks, removal |
| OpenWrt 25.12.6 `armsr/armv8` AArch64 APK | Matching full-system QEMU install, suite checks, removal |
| OpenWrt 24.10.8 `malta/le` MIPS32 IPK | Matching full-system QEMU install, suite checks, removal |
| Jetson Linux 36.5.2 and 39.2.2 AArch64 GNU/Linux | QEMU user mode with each SDK sysroot and matching NVIDIA sample libraries |

The OpenWrt guests enabled loopback in the test setup. The Jetson tests used the host kernel, filesystem, and TCP stack through QEMU user mode. The SDK checks exercised CPU and RAM, storage with sync and cleanup, TCP loopback, and stress with and without opted-in storage. Physical-board measurements remain unverified. [OpenWrt SDK records](openwrt-compatibility.md) · [MIPS SDK records](openwrt-platforms.md) · [Jetson SDK records](jetson-linux.md)
