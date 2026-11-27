# Network link diagnostics and lab tests

## Passive network link health

Run `eld-engine network health --interface eth0` with the name of the interface
to inspect. The JSON snapshot shows carrier, negotiated speed when available,
duplex, Linux operational state, traffic and error/drop counters, and carrier
changes. Receive CRC errors and receive-buffer misses are reported separately
when available. These counters can help distinguish frame errors from receive
pressure, but do not identify the root cause. Linux can report an explicit
`unknown` operational state even when the attribute is readable. The command
reads only that interface's Linux sysfs observations and sends no traffic.
Counters are cumulative, read separately, and are not a bandwidth or
packet-loss test. See the kernel's [interface statistics](https://docs.kernel.org/networking/statistics.html)
and [operational states](https://docs.kernel.org/networking/operstates.html).

Add `--sample-ms 1000` to take two passive snapshots (allowed: 1–60,000 ms).
The report records the monotonic interval and whether the same boot and
interface identity were observed. It reports per-counter differences only with
comparable observations, and withholds all differences if a counter decreases.
Unchanged endpoint carrier states may still hide transitions recorded by
`carrier_changes`. Sysfs fields are read sequentially; the result is not a
precise rate, loss percentage, or proof that no transient reset occurred.
Boot identifiers are not printed.

## TCP loopback

```sh
./target/release/eld-engine network --size-mib 8 --timeout-ms 5000
```

The command connects two TCP sockets through a listener bound to `127.0.0.1` on an ephemeral port. Timing starts after connection setup and covers one-way application throughput, including receiving and verifying the payload. The result covers the local TCP stack. Physical NIC throughput requires a separate measurement. The command uses the system's existing loopback configuration. `--size-mib` accepts 1–64 and defaults to 8. `--timeout-ms` accepts 1–30000 and defaults to 5000.

## TCP peer throughput and round-trip latency

Run these commands on two machines on an isolated wired or Wi-Fi test network. Use the numeric IPv4 address assigned to the intended interface on each machine. Agree on the port, session value, payload size, and probe count. The example uses addresses in a private subnet.

```sh
# Receiver
./target/release/eld-engine network peer-server --bind 192.168.5.10 \
  --peer 192.168.5.11 --port 5555 --session 0123456789abcdef0123456789abcdef
# Sender
./target/release/eld-engine network peer-client --bind 192.168.5.11 \
  --peer 192.168.5.10 --port 5555 --session 0123456789abcdef0123456789abcdef
```

The receiver accepts one connection from the named peer. The sender binds its source address. Both processes check the matching 32-hex-character session and budgets. The session identifies a test and provides no cryptographic protection. Public addresses, names, wildcard addresses, multicast, and addresses ending in `.0` or `.255` are refused. The receiver can use `--port 0` to print a kernel-assigned port for a local fixture.

`--size-mib` accepts 1–64 (default 8), `--pings` accepts 1–1000 (default 20), and `--timeout-ms` accepts 1–30000 (default 30000). The deadline includes connection setup or waiting for the sender. Latency is the application's TCP echo round trip. The receiver then verifies a one-way payload and reports throughput. The sender times the transfer through the final acknowledgment. Swap roles for the reverse path. Linux routing and the selected addresses determine the actual path. The command does not identify the physical link or measure wire-level latency.

A peer fixture using two distinct loopback addresses passed natively and with the OpenWrt 25.12.6 AArch64 APK and 24.10.8 MIPS little-endian IPK binaries under QEMU user mode. The same test passed with Jetson Linux 39.2.2 SDK and sample libraries. Hardware Ethernet and Wi-Fi measurements remain unverified.
