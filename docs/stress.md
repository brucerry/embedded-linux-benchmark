# Stress tests and qualification profiles

## Automatic stress

```sh
./target/release/eld-engine stress auto
./target/release/eld-engine stress auto --rounds 2 --duration-ms 500 --network-mib 4
./target/release/eld-engine stress auto --rounds 2 \
  --storage-dir /path/to/writable-directory --storage-mib 16 --storage-sync
```

Each automatic round overlaps three sustained workloads: one integer CPU worker, RAM copies, and verified TCP loopback transfers. Workers keep running for at least the configured phase length, subject to the global deadline. Loopback reuses one connection within a round and continuously transfers data after reaching the initial minimum byte count. There is no sleep inserted between rounds.

The suite also reads available PCIe link width, USB link speed, and HDMI encoder status once per round. These are **passive observations, not active hardware benchmarks**. Missing or inaccessible attributes are reported as unavailable or skipped. Device discovery is capped at 256 entries per class and 32 candidates per class, and it runs only once at startup. Camera, bus, GPIO/IIO, accelerator, and network peer workloads stay out of automatic stress because they require an explicit device, fixture, or peer. Use their standalone commands to run them.

Supplying both `--storage-dir` and `--storage-mib` adds bounded filesystem work alongside the other loads. `--storage-mib` is the **total** 1–64 MiB write budget across all rounds, not a per-round allowance. Fixed-round mode divides it among rounds, putting any remainder in the final round. Add `--storage-sync` to include `sync_data()`.

Use one `--test` selector to repeat a single component without passive observations. CPU, RAM and network accept `--duration-ms`, RAM accepts `--buffer-kib`, and network accepts `--network-mib`. Storage-only requires both the directory and the total write budget and accepts `--storage-sync`. Options for other components produce an error before the run starts.

```sh
./target/release/eld-engine stress auto --test cpu --rounds 3 --duration-ms 500
./target/release/eld-engine stress auto --test ram --buffer-kib 2048
./target/release/eld-engine stress auto --test network --network-mib 4
./target/release/eld-engine stress auto --test storage --rounds 2 \
  --storage-dir /path/to/writable-directory --storage-mib 16 --storage-sync
```

| Option | Default | Accepted values |
| --- | --- | --- |
| `--rounds` | 3 | 1–5 |
| `--duration-ms` | 10000 per CPU, RAM and loopback phase | 1–60000 |
| `--buffer-kib` | 1024 per RAM buffer (2048 total) | 4–32768 |
| `--network-mib` | 8 minimum per complete loopback phase | 1–64 |
| `--timeout-ms` | 60000 for the full fixed-round run | 1–360000, greater than round count × phase duration unless storage-only |

## Elapsed-time runs

Use `--duration TIME` or `--duration-secs SEC` for a foreground elapsed target from 1 second through 14 days. `TIME` accepts whole seconds, minutes, hours, days, or weeks, with suffixes `s`, `m`, `h`, `d` and `w`. Either duration option replaces `--rounds` and `--timeout-ms`.

```sh
./target/release/eld-engine stress auto --duration 30m
./target/release/eld-engine stress auto --duration 2w
./target/release/eld-engine stress auto --duration-secs 1209600
./target/release/eld-engine stress auto --test ram --duration 1h --duration-ms 500
./target/release/eld-engine stress auto --test storage --duration 5m \
  --storage-dir /path/to/writable-directory --storage-mib 16
```

A complete round finishes all active phases at their configured duration or byte count. Near the deadline, they still start together but the shortened final round is counted separately as partial, never as complete. Partial CPU/RAM work and payload-verified loopback bytes still contribute to totals. Storage accounts for bytes actually written even if readback did not finish before the deadline, and only completed readbacks contribute to verified bytes. A workload error or a per-operation timeout before the global deadline fails the run.

A timed run keeps fixed-size totals, prints progress at most once every five minutes, and reports the elapsed time and stop reason. CPU, RAM and loopback workers are reused between rounds. Loopback fails if it stalls for five seconds before the global deadline. A timed storage operation uses at most 1 MiB of the total write budget and a ten-second operation deadline, shortened by the global deadline. When the write budget is spent, storage is skipped in subsequent suite rounds; storage-only instead finishes early after verifying its final write. The RAM cap is two 32 MiB buffers. Other workload buffers and passive observations have fixed limits.

Run a soak on a board with adequate cooling and power. This is a foreground workload, not a watchdog or recovery service: firmware limits, suspend, reboot, power loss, and process termination can interrupt it; it does not restart automatically. The monotonic clock excludes system suspend on Linux. Deadlines are checked between CPU batches, RAM copies, I/O chunks, and phases. An OS-scheduled delay or in-progress kernel I/O such as `sync_data()` can delay a check; the tool does not forcibly stop a driver operation. Sudden termination can leave a `.eld-engine-*` scratch file in a selected storage directory.

The elapsed CPU summary counts work completed; it is not an independent CPU correctness proof. The standalone `cpu` command reports its checksum. The standalone CLI tests independently recompute it from the measured iteration count.

## Optional final JSON summary

Add `--summary-json ./stress-summary.json` to write a versioned summary with
the stop reason, completed rounds, elapsed duration and selected workload
totals. The command reserves a new private file before starting load, never
overwrites an existing file, and leaves normal reports unchanged. The file
is populated at the end; startup failure or interruption can leave it empty
or incomplete. Treat it as successful evidence only when the owned process
exited successfully and the summary says `completed`. Count and measured
nanosecond values are decimal strings. This is not a stream, a peripheral
qualification result, or evidence that every metric remained healthy.

Add `--events-ndjson ./stress-events.ndjson` for a separate private file with
one start line, at most one progress line per five minutes at a completed
round boundary, and one finish line. Missed intervals are not replayed.
Read only complete lines; an interruption or startup/I/O error can leave a
partial stream or no finish line. A finish line records the workload outcome,
not successful file delivery or process exit. Require a successful exit from
the owned command before accepting a complete run. Use a fresh path for each
attempt. This stream does not create any extra workload or device access.

## Preview a qualification profile

```sh
eld-engine qualify plan --profile bring-up
eld-engine qualify plan --profile overnight --include-local-load
```

The versioned JSON plan does not detect hardware, write reports or run any
step. Bring-up is always passive. Quick, manufacturing and overnight preview
one CPU and one RAM phase only when requested with `--include-local-load`;
otherwise those steps are marked skipped. Run selected steps separately on a
suitable test machine. Overnight plans two sequential four-hour targets, not
a guaranteed wall-clock runtime or a pass/fail verdict. Board-specific
checks require an explicit profile. Physical peripherals, a network peer,
and storage writes always require separately selected fixtures or budgets
and are never planned for automatic execution.


With `--include-local-load`, `--local-test cpu|ram` limits the preview
to one workload; otherwise both are shown. Add `--max-local-load-secs`
(1 to 1,209,600 seconds) to reject a plan whose proposed sequential load
targets exceed that budget. The budget does not include setup/cleanup,
does not change the planned workload durations, and does not start or
enforce a running stress test.

For bounded bring-up observations, use `eld-engine qualify passive` or
`eld-engine qualify passive --board-profile board.profile`. This reads one
inventory snapshot, with no timed sampling, stress, or external network.
It writes no files by default. The versioned result wraps the existing inventory report.
Without a profile it reports the board check as not configured. For a
supplied profile read `inventory.board_check` for the scoped PCI result;
an exit code of zero means the observation was emitted, not that the board
passed. This command does not run the quick, manufacturing, or overnight
workloads, or issue an overall qualification verdict. You can also add
`--network-interface eth0` to embed a single read-only NIC observation.
It is not sampled twice, sends no packets, and is independent of the PCIe
board check. If the interface is absent, its observation says unavailable;
that does not imply a qualification failure. Add `--nvme-controller nvme0`
to also include one read-only controller metadata observation. This does not
collect or decode a SMART capture, touch a storage device node, or run a
storage test. A missing controller is reported without changing board
assessment.

For a machine-readable handoff, add `--evidence-json NEW_PATH`. It writes
an owner-only JSON file up to 1 MiB with the exact stdout observation and
wall-clock start/end plus monotonic elapsed milliseconds. It refuses to
overwrite any existing path. Treat the file as complete only after the
command exits successfully and the entire JSON validates; interrupted writes
may leave an incomplete file. The timestamps describe the collection host,
not a trusted external clock or a qualification verdict.

For correlation across machines, `--run-id HEX32` echoes an explicitly
chosen 32-character lowercase hexadecimal token in the observation, including
the observation embedded in the file. Create a fresh random token for each
invocation if you need to distinguish reruns. The identifier is not a path,
and does not by itself authenticate the sender, device, or observed data.
It is omitted if not supplied.
