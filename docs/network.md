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
interface identity were observed. The statistics directory must also stay the
same across the samples. It reports per-counter differences only with
comparable observations, and withholds all differences if a counter decreases.
Consult `delta.counter_coverage` and `delta.comparable_counters` before interpreting
the results: `none` means no readable counter pair, not measured zeros;
`partial` and `complete` indicate how many of the nine pairs were readable.
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

### Preview a dedicated link run

The host-side planner emits argument arrays and planned byte and latency
probe counts. It does not run commands, discover hosts or set up routes.

```sh
python3 scripts/plan_network_peer.py \
  --sender eth1 172.31.95.1/30 --receiver enP1p1s0 172.31.95.2/30 \
  --port 5555 --session "$(openssl rand -hex 16)"
```

Use independently verified addresses and confirm on both endpoints that the
route uses the dedicated interface and source address, without a gateway
or console tunnel in the measurement path. Start the receiver before the
sender, save ingress/egress health on both sides, and retain the two
process results. A preview does not prove routing, permission, or a
physical measurement. Treat unsolicited traffic and a missing or mismatched
peer as invalid runs, not as performance results.

### Correlate sender and receiver results

Add `--peer-receipt` to both peer commands and separately save each stdout
and exit status. The additional success record binds the session label,
mirrored addresses, listener port, verified byte count and probe count. It
does not replace the process exit status or the normal human output.

```sh
python3 scripts/check_network_peer_evidence.py \
  --sender-output sender.txt --receiver-output receiver.txt \
  --sender-ip 192.168.5.11 --receiver-ip 192.168.5.10 --port 5555 \
  --session "$SESSION" --size-mib 8 --probes 20 \
  --sender-exit "$SENDER_STATUS" --receiver-exit "$RECEIVER_STATUS"
```

Supply the expected parameters and exit statuses from trusted executions,
not inferred from the evidence files. Correlation does not authenticate a
remote endpoint, establish a physical path, or certify link health.

### Check route lookups separately from TCP receipts

On each trusted endpoint, save the IPv4 route lookup for the intended source
and peer, for example `ip -j -4 route get "$PEER_IP" from "$LOCAL_IP"`.
Compare both files with `scripts/check_peer_route_evidence.py --help`.
A local route (including one that names the expected interface), a gateway,
or an unexpected interface cannot qualify a direct-link test. Exit 0 means
the lookups match, 2 means the lookups describe a local or different path,
and 1 means the evidence could not be read or validated.

A lookup is a point-in-time kernel decision, not a trace of the test socket
or proof that a cable was used. A veth result is virtual, not physical.
For hardware qualification retain trusted endpoint identities, interface
and negotiated-link evidence, and observations made during the same isolated
run. Do not use the management SSH tunnel as the measured link.

To check a saved window from a build that reports counter coverage, run
`python3 scripts/check_network_health_window.py --evidence nic.json --interface eth0 --sample-ms 1000`
on the controller, using the same independently chosen interface and interval
as the original run. It checks the exact counter arithmetic, coverage, and
reset/withhold behavior against the two snapshots and returns their evidence
hash. It does not run a command on the target or prove the report came from
the claimed device, that a reset did not occur between samples, or that the
interface identity claim is authentic.

The controller's passive-evidence verifier uses the same arithmetic for nested
NIC windows. Older saved windows without coverage fields are accepted only
when their original counter values and withheld/reset states are consistent.
Matching saved JSON and stdout by themselves cannot establish valid arithmetic.
