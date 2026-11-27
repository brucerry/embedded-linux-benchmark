#!/usr/bin/env python3
"""Compare two private, opt-in Engine PCIe snapshots without accessing hardware."""
import argparse
import json
import re

from check_hub_evidence import PCI_BDF, check, parse, read_bounded

UUID = re.compile(r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}")
U64 = re.compile(r"(?:0|[1-9][0-9]{0,19})")
NAME = re.compile(r"[a-zA-Z0-9_.-]{1,128}")
DEVICE_KEYS = ("aer_dev_correctable", "aer_dev_nonfatal", "aer_dev_fatal")
ROOT_KEYS = tuple("aer_rootport_total_err_" + k for k in ("cor", "nonfatal", "fatal"))
MISSING = ("unavailable", "inaccessible", "invalid")


def keys(row, names):
    check(type(row) is dict and row.keys() == set(names), "invalid snapshot fields")


def integer(value):
    check(type(value) is str and U64.fullmatch(value) and int(value) < 2**64,
          "invalid unsigned counter or identity")
    return int(value)


def named(value):
    check(type(value) is str and NAME.fullmatch(value), "invalid driver or counter name")
    return value


def observation(row, decoder):
    check(type(row) is dict, "invalid counter observation")
    status = row.get("status")
    if status == "observed":
        keys(row, ("status", "value"))
        return status, decoder(row["value"])
    check(status in MISSING, "unknown observation state")
    keys(row, ("status",))
    return status, None


def messages(value):
    keys(value, ("total_messages", "categories"))
    categories = value["categories"]
    check(type(categories) is dict and len(categories) <= 256, "invalid category count")
    return integer(value["total_messages"]), {named(k): integer(v) for k, v in categories.items()}


def binding(value):
    check(type(value) is dict, "invalid driver binding")
    status = value.get("status")
    if status == "bound":
        keys(value, ("status", "driver", "module"))
        module = observation(value["module"], named)
        return status, named(value["driver"]), module
    keys(value, ("status",))
    check(status in ("unbound", "inaccessible", "invalid", "changed"), "unknown driver state")
    return status,


def bdf(value):
    check(type(value) is str and PCI_BDF.fullmatch(value), "invalid observer BDF")
    return value


def read_snapshot(raw):
    envelope = parse(raw)
    keys(envelope, ("schema_version", "kind", "boot_id", "started_uptime_cs",
                    "finished_uptime_cs", "observer_identity", "snapshot"))
    check(type(envelope["schema_version"]) is int and envelope["schema_version"] == 1
          and envelope["kind"] == "pcie_saved_snapshot", "unsupported saved snapshot")
    boot = envelope["boot_id"]
    check(type(boot) is str and UUID.fullmatch(boot) and any(c not in "0-" for c in boot),
          "invalid boot identity")
    start = integer(envelope["started_uptime_cs"])
    end = integer(envelope["finished_uptime_cs"])
    check(start <= end, "invalid uptime window")
    snapshot = envelope["snapshot"]
    base = ("schema_version", "device", "observations", "root_hierarchy_aer")
    check(type(snapshot) is dict, "missing snapshot")
    keys(snapshot, (*base, "target_link_expectation") if "target_link_expectation" in snapshot else base)
    check(type(snapshot["schema_version"]) is int and snapshot["schema_version"] == 1,
          "unsupported PCIe snapshot")
    target = bdf(snapshot["device"])
    observed, identity = snapshot["observations"], envelope["observer_identity"]
    check(type(observed) is list and type(identity) is list and
          1 <= len(observed) == len(identity) <= 32, "invalid observer count")
    sequence, rows = [], {}
    for idx, (row, marker) in enumerate(zip(observed, identity)):
        keys(row, ("bdf", "role", "driver_binding", "link", "device_aer"))
        keys(marker, ("bdf", "sysfs_dev", "sysfs_inode"))
        name = bdf(row["bdf"])
        check(marker["bdf"] == name and name not in rows and
              row["role"] == ("target" if idx == 0 else "upstream"),
              "invalid observer ordering")
        check(idx != 0 or target == name, "target does not match first observer")
        sequence.append((name, integer(marker["sysfs_dev"]), integer(marker["sysfs_inode"])))
        aer = row["device_aer"]
        keys(aer, DEVICE_KEYS)
        rows[name] = (binding(row["driver_binding"]),
                      {k: observation(aer[k], messages) for k in DEVICE_KEYS})
    roots = {}
    root_rows = snapshot["root_hierarchy_aer"]
    check(type(root_rows) is list and len(root_rows) <= len(rows), "invalid root observer count")
    for row in root_rows:
        keys(row, ("bdf", "counters"))
        name = bdf(row["bdf"])
        check(name in rows and name not in roots, "invalid root observer")
        aer = row["counters"]
        keys(aer, ROOT_KEYS)
        roots[name] = {k: observation(aer[k], integer) for k in ROOT_KEYS}
    return boot, start, end, sequence, rows, roots


def refuse(reason):
    return {"status": "not_comparable", "reason": reason}


def delta(before, after, is_message):
    status, first = before
    last_status, last = after
    if status != last_status:
        return refuse("observation_state_changed")
    if status != "observed":
        return {"status": status}
    if is_message:
        total0, categories0 = first
        total1, categories1 = last
        if categories0.keys() != categories1.keys():
            return refuse("category_set_changed")
        if total1 < total0 or any(categories1[k] < v for k, v in categories0.items()):
            return refuse("counter_decreased")
        value = {"total_messages": str(total1 - total0),
                 "categories": {k: str(categories1[k] - v) for k, v in categories0.items()}}
    else:
        if last < first:
            return refuse("counter_decreased")
        value = str(last - first)
    return {"status": "observed", "value": value}


def compare(before, after):
    boot0, _, end, order0, rows0, roots0 = read_snapshot(before)
    boot1, start, _, order1, rows1, roots1 = read_snapshot(after)
    out = {"schema_version": 1, "kind": "pcie_saved_snapshot_comparison",
           "hardware_authenticity": "not_verified"}
    if boot0 != boot1:
        return {**out, **refuse("boot_changed")}
    if end >= start:
        return {**out, **refuse("windows_not_strictly_ordered")}
    if order0 != order1:
        return {**out, **refuse("observer_identity_changed")}
    by_observer = []
    for name, _, _ in order0:
        bind0, counters0 = rows0[name]
        bind1, counters1 = rows1[name]
        if "changed" in (bind0[0], bind1[0]):
            return {**out, **refuse("driver_binding_changed")}
        if any(x[0] not in ("bound", "unbound") or
               (x[0] == "bound" and x[2][0] not in ("observed", "unavailable"))
               for x in (bind0, bind1)):
            return {**out, **refuse("driver_binding_indeterminate")}
        if bind0 != bind1:
            return {**out, **refuse("driver_binding_changed")}
        root0, root1 = roots0.get(name, {}), roots1.get(name, {})
        missing = ("unavailable", None)
        by_observer.append({"bdf": name,
                            "device_aer": {k: delta(counters0[k], counters1[k], True)
                                           for k in DEVICE_KEYS},
                            "root_hierarchy_aer": {k: delta(root0.get(k, missing),
                                                           root1.get(k, missing), False)
                                                   for k in ROOT_KEYS}})
    return {**out, "status": "compared", "by_observer": by_observer}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("before")
    cli.add_argument("after")
    args = cli.parse_args()
    try:
        result = compare(read_bounded(args.before), read_bounded(args.after))
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Snapshot rejected: {exc}\n")
    print(json.dumps(result, sort_keys=True))
    return 0 if result["status"] == "compared" else 2


if __name__ == "__main__":
    raise SystemExit(main())
