# Read-only NUMA inventory

`eld-engine numa inventory` returns version 1 JSON describing online Linux
NUMA nodes. Each entry records its node ID, CPU IDs and total memory from the
kernel's NUMA sysfs interface. Memory bytes are decimal strings to preserve
integer precision. An empty observed CPU list is distinct from a missing or
malformed attribute. A change in the online node set during the read discards
the snapshot; a stable node list does not make all metric reads atomic.

This command is limited to 64 nodes and 8,192 CPU ID observations. It does not
measure bandwidth or latency, bind processes or memory, or interpret NUMA
distance. Hardware or firmware without NUMA sysfs reports unavailable.

When Hub consumes saved `qualify passive --include-system-health` output,
its offline checker bounds the node and CPU lists and preserves the exact
memory byte strings. Missing, inaccessible, invalid and changed-during-read
reports are not empty hardware inventories. Standalone and qualification data
use the same schema; separate reads need not describe the same instant.
