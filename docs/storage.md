# Storage benchmarks and health

## Storage

Select an existing writable directory and a size of 1–64 MiB. The command creates a uniquely named regular file with owner-only permissions, writes the requested payload sequentially, then reads and verifies the same byte count. Exclusive creation protects existing files. The command removes the scratch file on success or a handled error and reports the path if removal fails.

```sh
./target/release/eld-engine storage --dir /path/to/writable-directory --size-mib 16
./target/release/eld-engine storage --dir /path/to/writable-directory --size-mib 16 --sync
```

The write and read rates are separate. Writes go through buffered filesystem I/O. With `--sync`, write timing includes `sync_data()` on the file. Reads are page-cache eligible and include payload verification. The output measures this filesystem path, including its caching and writeback behavior. The size budget covers application payload bytes, and filesystem metadata and device writeback can add physical I/O. `--timeout-ms` accepts 1–60000 and defaults to 10000.

## Passive NVMe controller status

`eld-engine storage nvme-health --controller nvme0` reads the named
controller's Linux sysfs state, transport, model and firmware revision.
It does not read serial numbers, SMART or media-error logs, open raw devices,
change controller state, or benchmark storage. A live controller state does
not certify drive health.

## Decode a previously captured SMART log

`eld-engine storage nvme-smart-decode < captured-smart-log.bin` reads exactly
512 bytes of raw NVMe SMART / Health Information log page 02h from standard
input. Supply a finished file, not a live device path, a growing stream, or
text/JSON. The Engine does not collect the log or open an NVMe device.
The output includes warning bits, composite Kelvin temperature (zero left
uninterpreted as null), normalized spare fields (reserved values are null),
and the raw vendor endurance estimate, which can exceed 100. The 128-bit
counters are exact decimal strings. The data units read and written are
rounded-up groups of one thousand 512-byte units, not exact byte totals.
Their raw zero means unreported, unlike a zero critical-warning mask.
No health verdict is inferred. Keep capture time, controller identity, log
identifier, controller/namespace scope, and a digest of the raw page with
the capture; the raw page does not authenticate them. See the
[NVM Express Base Specification 2.0e](https://nvmexpress.org/wp-content/uploads/NVM-Express-Base-Specification-2.0e-2024.07.29-Ratified.pdf),
SMART / Health Information log.

### Verify a saved SMART decode

After decoding a saved regular file to JSON, verify the copy against the digest
recorded at capture and verify the decoded values against the raw bytes:

```sh
python3 scripts/check_nvme_smart_evidence.py \
  --page captured-smart-log.bin --report decoded-smart.json \
  --expected-sha256 "$CAPTURE_SHA256"
```

The receipt binds the raw page and decoded report by hash. It does not certify
the original capture, controller identity, or drive health. The verifier
does not open device nodes or start I/O against the controller.

## Passive MMC wear observations

`eld-engine storage mmc-health --card mmc0:0001` reads the named
MMC card's cached kernel sysfs `pre_eol_info` and `life_time` when
the MMC driver exposes them. The integers are raw device codes. They
are not fresh readings, exact wear percentages, a health verdict, or
estimated time to failure. Linux only populates those cached fields
for supported Extended CSD revisions (revision 7 or later), so a zero
can also be an unpopulated value on an older card. The tool preserves
zero as a raw observation without interpreting it as healthy. SD cards
are not supported by this command. No block device is opened, no MMC
command is sent, and no serial number is collected. See the Linux
[MMC attribute definitions](https://github.com/torvalds/linux/blob/master/drivers/mmc/core/mmc.c).

A host-side consumer can validate the saved JSON without opening a card:

```sh
python3 scripts/check_mmc_health_evidence.py --report mmc.json --card mmc0:0001
```

Pass `--expected-sha256` if the controller already has a trusted file digest.
The checker binds the report to the selected card identifier and preserves
raw field availability. It does not confirm the source device or infer wear
from raw codes, including zero.


When capabilities advertises `passive_mmc_health` schema 1, the same
cached observation can be included in a qualification report with
`eld-engine qualify passive --mmc-card mmc0:0001`. The selection is opt-in
and can appear in the observation timeline. The offline verifier can
correlate the saved evidence and stdout. Capability planner support for
binding a trusted expected card to that invocation is separate.
