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
