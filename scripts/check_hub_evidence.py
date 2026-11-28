#!/usr/bin/env python3
"""Reference verifier for Hub evidence handoffs, not a deployer or hardware probe."""
import argparse
import hashlib
import json
import os
from pathlib import PurePosixPath
from decimal import Decimal
import re
import stat
import sys

MAX_BYTES = 1024 * 1024
RUN_ID = re.compile("[0-9a-f]{32}")
PCI_BDF = re.compile(r"[0-9a-f]{4}:[0-9a-f]{2}:[0-1][0-9a-f]\.[0-7]")
PCI_ID = re.compile(r"[0-9a-f]{4}")
MMC_CARD = re.compile(r"mmc(?:0|[1-9][0-9]{0,6}):[a-f0-9]{4}", re.ASCII)
NVME_CONTROLLER = re.compile(r"nvme(?:0|[1-9][0-9]{0,6})", re.ASCII)
NIC_INTERFACE = re.compile(r"[A-Za-z0-9_.-]{1,15}", re.ASCII)


def valid_nic_interface(name):
    return type(name) is str and name not in (".", "..") and NIC_INTERFACE.fullmatch(name) is not None


EDAC_CONTROLLER = re.compile(r"mc(?:0|[1-9][0-9]{0,6})")
PCI_NAMES = ("present", "match", "missing", "mismatch", "inaccessible",
             "invalid", "unavailable", "changed")
OPTIONAL_PASSIVE = {
    "system-health": ("passive_system_health", "--include-system-health", "system_health"),
    "timeline": ("passive_observation_timeline", "--include-timeline", "observation_timeline"),
}



def check(condition, reason):
    if not condition:
        raise ValueError(reason)


def check_evidence_path(path):
    check(type(path) is str and 0 < len(path) <= 240 and path.isascii()
          and all(c.isalnum() or c in "/_.-" for c in path),
          "expected a normalized safe absolute evidence path")
    p = PurePosixPath(path)
    check(p.is_absolute() and len(p.parts) >= 3 and ".." not in p.parts
          and str(p) == path and not path.startswith("//"),
          "expected a normalized safe absolute evidence path")


def read_bounded(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as stream:
        before = os.fstat(stream.fileno())
        check(stat.S_ISREG(before.st_mode) and 0 < before.st_size <= MAX_BYTES,
              "expected a bounded nonempty regular file")
        data = stream.read(MAX_BYTES + 1)
        after = os.fstat(stream.fileno())
        check(len(data) == before.st_size == after.st_size
              and before.st_mtime_ns == after.st_mtime_ns
              and before.st_ctime_ns == after.st_ctime_ns, "file changed while reading")
        return data


def read_selected_json(path, expected_sha256=None):
    """Read, optionally match an independently selected digest, then parse once."""
    raw = read_bounded(path)
    if expected_sha256 is not None:
        check(type(expected_sha256) is str and re.fullmatch(r"[0-9a-f]{64}", expected_sha256),
              "expected a lowercase capabilities SHA-256")
        check(hashlib.sha256(raw).hexdigest() == expected_sha256,
              "capabilities differ from expected SHA-256")
    return parse(raw)


def parse(data):
    def unique(items):
        out = {}
        for key, value in items:
            check(key not in out, "duplicate JSON key")
            out[key] = value
        return out

    def bad_constant(_):
        raise ValueError("non-finite JSON number")

    check(0 < len(data) <= MAX_BYTES, "JSON byte limit exceeded")
    out = json.loads(data.decode("utf-8"), object_pairs_hook=unique,
                     parse_float=Decimal, parse_constant=bad_constant)
    check(type(out) is dict, "expected a JSON object")
    return out


def equal(x, y):
    # A bool, integer and decimal must not compare equal across JSON types.
    if type(x) is not type(y):
        return False
    if isinstance(x, dict):
        return x.keys() == y.keys() and all(equal(x[k], y[k]) for k in x)
    if isinstance(x, list):
        return len(x) == len(y) and all(equal(a, b) for a, b in zip(x, y))
    return x == y


def verify_numa_inventory(snapshot):
    check(type(snapshot) is dict and snapshot.keys() == {"schema_version", "scope", "nodes"}
          and type(snapshot["schema_version"]) is int and snapshot["schema_version"] == 1
          and snapshot["scope"] == "linux_numa_inventory", "invalid NUMA snapshot envelope")

    def observation(item, extra=()):
        check(type(item) is dict, "invalid NUMA observation")
        if item.get("status") == "observed":
            check(item.keys() == {"status", "value"}, "invalid NUMA observed fields")
            return True, item["value"]
        check(item.keys() == {"status"}
              and item["status"] in ("unavailable", "inaccessible", "invalid", *extra),
              "invalid NUMA withheld status")
        return False, None

    found, nodes = observation(snapshot["nodes"], ("changed_during_read", "limit_exceeded"))
    if not found:
        return
    check(type(nodes) is list and 1 <= len(nodes) <= 64, "invalid NUMA node count")
    previous = -1
    cpu_count = 0
    for row in nodes:
        check(type(row) is dict and row.keys() == {"node_id", "cpu_ids", "memory_total_bytes"},
              "invalid NUMA node fields")
        ident = row["node_id"]
        check(type(ident) is int and previous < ident < 2**32, "invalid NUMA node order or ID")
        previous = ident
        observed, cpus = observation(row["cpu_ids"])
        if observed:
            check(type(cpus) is list and len(cpus) <= 8192 - cpu_count
                  and all(type(cpu) is int and 0 <= cpu < 2**32 for cpu in cpus)
                  and all(a < b for a, b in zip(cpus, cpus[1:])), "invalid NUMA CPU list")
            cpu_count += len(cpus)
        observed, memory = observation(row["memory_total_bytes"])
        if observed:
            check(type(memory) is str and re.fullmatch(r"0|[1-9][0-9]{0,19}", memory)
                  and int(memory) < 2**64 and int(memory) % 1024 == 0, "invalid NUMA memory bytes")


def verify_pressure_snapshot(snapshot):
    resources = ("cpu", "memory", "io")
    averages = ("avg10_percent", "avg60_percent", "avg300_percent")
    check(type(snapshot) is dict and snapshot.keys() == {"schema_version", "scope", *resources}
          and type(snapshot["schema_version"]) is int and snapshot["schema_version"] == 1
          and snapshot["scope"] == "system_proc_pressure", "invalid pressure snapshot envelope")
    for resource in resources:
        item = snapshot[resource]
        check(type(item) is dict, "invalid pressure resource")
        status = item.get("status")
        if status != "observed":
            check(item.keys() == {"status"} and status in ("unavailable", "inaccessible", "invalid"),
                  "invalid pressure resource status")
            continue
        check(item.keys() == {"status", "some", "full"}, "invalid pressure resource fields")
        for stall in ("some", "full"):
            row = item[stall]
            if stall == "full" and resource == "cpu":
                check(equal(row, {"status": "not_applicable"}), "system CPU full is not applicable")
                continue
            if stall == "full" and equal(row, {"status": "unavailable"}):
                continue
            check(type(row) is dict and row.keys() == {"status", "total_stall_us", *averages}
                  and row["status"] == "observed", "invalid pressure stall observation")
            for field in averages:
                val = row[field]
                check(type(val) is str and re.fullmatch(r"(?:0|[1-9][0-9]?)\.[0-9]{2}|100\.00", val),
                      "invalid pressure percentage")
            total = row["total_stall_us"]
            check(type(total) is str and re.fullmatch(r"0|[1-9][0-9]{0,19}", total)
                  and int(total) < 2**64, "invalid pressure total")


def verify_pressure_series(series, elapsed):
    expected = {"schema_version", "kind", "scope", "sample_count",
                "requested_gap_ms", "workload_execution", "samples"}
    check(type(series) is dict and series.keys() == expected
          and type(series["schema_version"]) is int and series["schema_version"] == 1
          and series["kind"] == "linux_pressure_series" and series["scope"] == "single_process"
          and series["workload_execution"] == "none", "unsupported pressure series")
    count, gap, rows = (series[k] for k in ("sample_count", "requested_gap_ms", "samples"))
    check(type(count) is int and 2 <= count <= 8 and type(gap) is int and 100 <= gap <= 1000
          and type(rows) is list and len(rows) == count, "invalid pressure series bounds")
    previous = None
    for index, row in enumerate(rows):
        check(type(row) is dict and row.keys() == {
            "index", "started_monotonic_ms", "finished_monotonic_ms", "pressure"
        } and type(row["index"]) is int and row["index"] == index, "invalid pressure sample")
        start, finish = row["started_monotonic_ms"], row["finished_monotonic_ms"]
        check(type(start) is int and type(finish) is int and 0 <= start <= finish <= elapsed
              and (previous is None or start - previous >= gap), "invalid pressure timing")
        verify_pressure_snapshot(row["pressure"])
        previous = finish


def verify_nvme_controller(snapshot, expected=None):
    common = {"schema_version", "controller", "status", "smart_sampled"}
    attributes = {"state", "transport", "model", "firmware_revision"}
    check(type(snapshot) is dict and common <= snapshot.keys()
          and type(snapshot["schema_version"]) is int and snapshot["schema_version"] == 1
          and type(snapshot["controller"]) is str and NVME_CONTROLLER.fullmatch(snapshot["controller"])
          and snapshot["smart_sampled"] is False, "invalid passive NVMe controller snapshot")
    check(expected is None or snapshot["controller"] == expected,
          "NVMe controller differs from trusted plan")
    unavailable = ("unavailable", "inaccessible", "invalid")
    if snapshot["status"] != "observed":
        check(snapshot.keys() == common and snapshot["status"] in (*unavailable, "changed"),
              "invalid withheld NVMe snapshot")
        return
    check(snapshot.keys() == common | attributes, "unexpected passive NVMe fields")
    for name in attributes:
        field = snapshot[name]
        check(type(field) is dict and "status" in field, "invalid passive NVMe attribute")
        if field["status"] == "observed":
            check(field.keys() == {"status", "value"} and type(field["value"]) is str
                  and 0 < len(field["value"]) <= 128
                  and field["value"] == field["value"].strip()
                  and all(" " <= c <= "~" for c in field["value"]), "invalid NVMe attribute text")
        else:
            check(field.keys() == {"status"} and field["status"] in unavailable,
                  "invalid withheld NVMe attribute")


def verify_network_window(window, elapsed):
    check(window.keys() == {"schema_version", "mode", "requested_wait_ms",
                            "sample_start_interval_ms", "continuity", "before", "after", "delta"}
          and type(window["schema_version"]) is int and window["schema_version"] == 1
          and window["mode"] == "two_passive_snapshots", "unsupported network window")
    wait, interval = window["requested_wait_ms"], window["sample_start_interval_ms"]
    check(type(wait) is int and type(interval) is int and 1 <= wait <= 2000
          and wait <= interval <= elapsed, "invalid network sample timing")
    a, b, delta = (window[k] for k in ("before", "after", "delta"))
    check(all(type(row) is dict for row in (a, b, delta))
          and type(a.get("interface")) is str and a.get("interface") == b.get("interface"),
          "network snapshot interface mismatch")
    # Import locally to share the same pure arithmetic with the standalone CLI.
    from check_network_health_window import recompute
    recompute(window, a["interface"], wait, allow_legacy=True)


def verify_unsigned_observation(row, label):
    check(type(row) is dict and row.get("status") in (
        "observed", "unavailable", "inaccessible", "invalid"
    ), f"invalid {label} observation")
    if row["status"] != "observed":
        check(row.keys() == {"status"}, f"unobserved {label} carries a value")
        return
    value = row.get("value")
    check(row.keys() == {"status", "value"} and type(value) is str
          and re.fullmatch(r"0|[1-9][0-9]{0,19}", value) and int(value) < 2**64,
          f"invalid unsigned {label} observation")


def verify_cpu_frequency(report):
    fields = {"scaling_reported_khz", "cpuinfo_current_khz", "policy_max_khz", "cpuinfo_max_khz"}
    check(type(report) is dict and report.keys() == {"status", "policies"}
          and report["status"] in ("observed", "truncated", "no_policies", "unavailable",
                                   "inaccessible", "invalid")
          and type(report["policies"]) is list and len(report["policies"]) <= 32,
          "invalid CPU frequency observation")
    rows = report["policies"]
    status = report["status"]
    check((status == "observed" and bool(rows))
          or (status == "truncated" and len(rows) in (0, 32))
          or (status not in ("observed", "truncated") and not rows),
          "inconsistent CPU frequency policy coverage")
    last = -1
    for row in rows:
        check(type(row) is dict and row.keys() == {"policy", *fields}
              and type(row["policy"]) is str, "invalid CPU frequency policy fields")
        index = re.fullmatch(r"policy(0|[1-9][0-9]{0,6})", row["policy"])
        check(index and int(index[1]) > last, "invalid, duplicate, or unordered CPU policy")
        last = int(index[1])
        for name in fields:
            item = row[name]
            verify_unsigned_observation(item, "CPU frequency")
            check(item.get("value") != "0", "observed CPU frequency must be positive")


def verify_cpu_throttle(report):
    check(type(report) is dict and report.keys() == {"status", "cpu_observers"}
          and report["status"] in ("observed", "truncated", "unavailable",
                                   "inaccessible", "invalid")
          and type(report["cpu_observers"]) is list and len(report["cpu_observers"]) <= 64,
          "invalid CPU throttle observation")
    rows = report["cpu_observers"]
    status = report["status"]
    check(status == "truncated" or (status == "observed") == bool(rows),
          "inconsistent CPU throttle coverage")
    last = -1
    for row in rows:
        check(type(row) is dict and row.keys() == {"cpu", "core_count", "package_count_at_cpu"}
              and type(row["cpu"]) is int and last < row["cpu"] <= 9_999_999,
              "invalid, duplicate, or unordered CPU observer")
        last = row["cpu"]
        for name in ("core_count", "package_count_at_cpu"):
            verify_unsigned_observation(row[name], "CPU throttle")


def verify_powercap(report):
    states = ("observed", "unavailable", "inaccessible", "invalid")
    check(type(report) is dict and report.keys() == {"status", "zones"}
          and report["status"] in (*states, "truncated", "no_zones")
          and type(report["zones"]) is list and len(report["zones"]) <= 32,
          "invalid powercap observation")
    zones = report["zones"]
    status = report["status"]
    check((status == "observed" and bool(zones))
          or (status == "truncated" and len(zones) in (0, 32))
          or (status not in ("observed", "truncated") and not zones),
          "inconsistent powercap zone coverage")
    energy = {"energy_microjoules", "max_energy_range_microjoules"}
    extra = {"power_microwatts", "max_power_range_microwatts"}
    legacy = {"zone", "name", *energy}
    previous = ""
    for zone in zones:
        check(type(zone) is dict and (zone.keys() == legacy or zone.keys() == legacy | extra),
              "invalid powercap zone fields")
        name = zone["zone"]
        check(type(name) is str and len(name) <= 96 and name > previous
              and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*(?::(?:0|[1-9][0-9]{0,6})){1,4}", name),
              "invalid, duplicate, or unordered powercap zone name")
        previous = name
        for name, row in zone.items():
            if name == "zone":
                continue
            if name != "name":
                verify_unsigned_observation(row, "powercap")
                continue
            check(type(row) is dict and row.get("status") in states,
                  "invalid powercap field observation")
            if row["status"] != "observed":
                check(row.keys() == {"status"}, "unobserved powercap field carries a value")
                continue
            value = row.get("value")
            check(row.keys() == {"status", "value"} and type(value) is str,
                  "powercap observed value must be text")
            check(0 < len(value.encode("utf-8")) <= 256 and value == value.strip()
                  and all(not (ord(c) < 32 or 127 <= ord(c) <= 159) for c in value),
                  "invalid powercap label")


def verify_thermal_window(inventory, elapsed, *, max_wait_ms=2000):
    window = inventory["thermal_window"]
    check(type(window) is dict and window.keys() == {
        "requested_wait_ms", "sample_start_interval_ms", "before", "after", "differences"
    }, "invalid thermal window")
    wait, interval = window["requested_wait_ms"], window["sample_start_interval_ms"]
    check(type(wait) is int and type(interval) is int and 1 <= wait <= max_wait_ms
          and wait <= interval <= elapsed, "invalid thermal timing")

    def indexed(snapshot):
        check(type(snapshot) is dict and snapshot.keys() == {"status", "zones"}
              and snapshot["status"] in ("observed", "truncated", "unavailable",
                                         "inaccessible", "invalid")
              and type(snapshot["zones"]) is list and len(snapshot["zones"]) <= 32,
              "invalid thermal snapshot")
        if snapshot["status"] not in ("observed", "truncated"):
            check(not snapshot["zones"], "unexpected readings in unavailable thermal snapshot")
        rows = {}
        last = -1
        for item in snapshot["zones"]:
            check(type(item) is dict and item.keys() == {
                "zone", "type", "temperature_millicelsius"
            }, "invalid thermal row")
            zone = item["zone"]
            check(type(zone) is str and re.fullmatch(r"thermal_zone(0|[1-9][0-9]{0,6})", zone),
                  "invalid thermal zone name")
            index = int(zone[len("thermal_zone"):])
            check(index > last, "thermal zones must be unique and ordered")
            last = index
            for field in ("type", "temperature_millicelsius"):
                reading = item[field]
                check(type(reading) is dict and reading.get("status") in (
                    "observed", "unavailable", "inaccessible", "invalid"
                ), "invalid thermal reading")
                if reading["status"] != "observed":
                    check(reading.keys() == {"status"}, "unexpected unavailable value")
                    continue
                value = reading.get("value")
                check(reading.keys() == {"status", "value"} and type(value) is str and value,
                      "invalid observed thermal value")
                if field == "temperature_millicelsius":
                    check(re.fullmatch(r"0|-?[1-9][0-9]{0,5}", value)
                          and -273150 <= int(value) <= 300000, "invalid thermal temperature")
            rows[index] = item
        return rows

    first, second = indexed(window["before"]), indexed(window["after"])
    check(equal(inventory.get("thermal"), window["before"]), "thermal inventory baseline differs")
    rows = window["differences"]
    keys = sorted(first.keys() | second.keys())
    check(type(rows) is list and len(rows) == len(keys), "wrong thermal difference count")
    for row, key in zip(rows, keys):
        check(type(row) is dict and row.keys() == {"zone", "difference_millicelsius"}
              and row["zone"] == f"thermal_zone{key}", "invalid thermal difference row")
        value = row["difference_millicelsius"]
        if value == {"status": "not_comparable"}:
            continue
        a, b = first.get(key), second.get(key)
        check(type(value) is dict and value.keys() == {"status", "value"}
              and value["status"] == "observed_difference" and a is not None and b is not None
              and a["type"].get("status") == "observed" and equal(a["type"], b["type"])
              and a["temperature_millicelsius"]["status"] == "observed"
              and b["temperature_millicelsius"]["status"] == "observed",
              "thermal difference lacks matching readings")
        expected = (int(b["temperature_millicelsius"]["value"])
                    - int(a["temperature_millicelsius"]["value"]))
        check(value["value"] == str(expected), "incorrect thermal difference")


def verify_timeline(observation, elapsed):
    timeline = observation["observation_timeline"]
    check(type(timeline) is dict and timeline.keys() == {
        "schema_version", "clock", "execution", "phases"
    } and type(timeline["schema_version"]) is int and timeline["schema_version"] == 1
        and timeline["clock"] == "single_process_monotonic"
        and timeline["execution"] == "sequential", "unsupported observation timeline")
    order = ("network_health", "nvme_health", "mmc_health", "pcie_health", "edac_health", "inventory", "system_health")
    expected = [key for key in order if key in observation]
    rows = timeline["phases"]
    check(type(rows) is list and len(rows) == len(expected), "invalid observation phase count")
    last = 0
    for key, row in zip(expected, rows):
        check(type(row) is dict and row.keys() == {
            "phase", "started_monotonic_ms", "finished_monotonic_ms"
        } and row["phase"] == key, "unexpected observation phase")
        start, finish = row["started_monotonic_ms"], row["finished_monotonic_ms"]
        check(type(start) is int and type(finish) is int and last <= start <= finish <= elapsed,
              "invalid observation phase interval")
        last = finish


def pcie_expectations(request):
    check(type(request) is str and 0 < len(request) <= 383, "invalid PCIe expectation request")
    parsed = []
    seen = set()
    for item in request.split(","):
        bdf, mark, identity = item.partition("=")
        check(len(parsed) < 16 and PCI_BDF.fullmatch(bdf) and bdf not in seen,
              "expected up to 16 unique canonical PCIe addresses")
        expected = None
        if mark:
            vendor, _, device = identity.partition(":")
            check(PCI_ID.fullmatch(vendor) and PCI_ID.fullmatch(device),
                  "expected canonical PCI vendor and device IDs")
            expected = {"vendor": vendor, "device": device}
        parsed.append((bdf, expected))
        seen.add(bdf)
    return parsed


def verify_required_pcie(rows):
    check(type(rows) is list and 1 <= len(rows) <= 16, "invalid PCIe row count")
    addresses = set()

    def ids(obj):
        return (type(obj) is dict and obj.keys() == {"vendor", "device"}
                and all(type(v) is str and PCI_ID.fullmatch(v) for v in obj.values()))

    for row in rows:
        check(type(row) is dict and row.keys() in (
            {"bdf", "status"}, {"bdf", "status", "expected", "observed"}
        ), "invalid PCIe row format")
        bdf, status = row["bdf"], row["status"]
        check(type(bdf) is str and PCI_BDF.fullmatch(bdf) and bdf not in addresses
              and status in PCI_NAMES, "invalid or duplicate PCIe row")
        addresses.add(bdf)
        if "expected" not in row:
            check(status not in ("match", "mismatch"), "PCIe row missing expected IDs")
            continue
        expected, observed = row["expected"], row["observed"]
        check(ids(expected) and status != "present", "invalid PCIe row expectation")
        if status in ("match", "mismatch"):
            check(ids(observed) and (observed == expected) == (status == "match"),
                  "PCIe row status disagrees with IDs")
        else:
            check(observed is None, "unexpected observed IDs in unavailable PCIe row")


def verify_edac_health(value):
    check(type(value) is dict and value.keys() == {
        "schema_version", "kind", "controller", "source", "result"
    } and type(value["schema_version"]) is int and value["schema_version"] == 1
        and value["kind"] == "linux_edac_memory" and value["source"] == "linux_sysfs"
        and type(value["controller"]) is str and EDAC_CONTROLLER.fullmatch(value["controller"]),
        "invalid EDAC selected observation")
    reading = value["result"]
    check(type(reading) is dict, "invalid EDAC result")
    status = reading.get("status")
    if status != "observed":
        check(reading.keys() == {"status"} and status in (
            "unavailable", "inaccessible", "invalid", "controller_changed",
            "reset_age_decreased", "observation_state_changed"), "invalid withheld EDAC result")
        return
    check(reading.keys() == {"status", "seconds_since_reset", "counters"}
          and type(reading["counters"]) is dict
          and reading["counters"].keys() == {"ce_count", "ue_count"}, "invalid EDAC fields")
    for field in (reading["seconds_since_reset"], *reading["counters"].values()):
        check(type(field) is dict, "invalid EDAC field")
        if field.get("status") == "observed":
            raw = field.get("value")
            check(field.keys() == {"status", "value"} and type(raw) is str
                  and re.fullmatch(r"0|[1-9][0-9]{0,19}", raw) and int(raw) < 2**64,
                  "invalid EDAC observation value")
        else:
            check(field.keys() == {"status"} and field["status"] in
                  ("unavailable", "inaccessible", "invalid"), "invalid EDAC field status")


PCIE_LINK_FIELDS = ("current_link_speed", "current_link_width", "max_link_speed", "max_link_width")


def verify_pcie_link(link):
    check(type(link) is dict and link.keys() == set(PCIE_LINK_FIELDS),
          "invalid PCIe link fields")
    for value in link.values():
        check(type(value) is dict, "invalid PCIe link observation")
        status = value.get("status")
        if status == "observed":
            raw = value.get("value")
            # pcie_health.rs trims sysfs values. Keep the remaining opaque
            # driver string, without guessing a unit or generation.
            check(value.keys() == {"status", "value"} and type(raw) is str
                  and re.fullmatch(r"[!-~](?:[ -~]{0,126}[!-~])?", raw),
                  "invalid PCIe link observation value")
        else:
            check(value.keys() == {"status"} and status in ("unavailable", "inaccessible", "invalid"),
                  "invalid PCIe link observation status")


def verify_planned_pcie_health(snapshot, device):
    check(type(snapshot) is dict
          and snapshot.keys() == {"schema_version", "device", "observations", "root_hierarchy_aer"}
          and type(snapshot["schema_version"]) is int and snapshot["schema_version"] == 1
          and snapshot["device"] == device, "PCIe health snapshot differs from trusted plan")
    from compare_pcie_snapshots import DEVICE_KEYS, ROOT_KEYS, binding, integer, messages, observation
    rows, roots = snapshot["observations"], snapshot["root_hierarchy_aer"]
    check(type(rows) is list and 1 <= len(rows) <= 32 and type(roots) is list
          and len(roots) <= len(rows), "invalid PCIe health observer count")
    known = {}
    for i, row in enumerate(rows):
        check(type(row) is dict and row.keys() == {"bdf", "role", "driver_binding", "link", "device_aer"}
              and type(row["bdf"]) is str and PCI_BDF.fullmatch(row["bdf"])
              and row["bdf"] not in known and row["role"] == ("target" if i == 0 else "upstream")
              and (i != 0 or row["bdf"] == device),
              "invalid PCIe health observer")
        verify_pcie_link(row["link"])
        counters = row["device_aer"]
        check(type(counters) is dict and counters.keys() == set(DEVICE_KEYS), "invalid PCIe device AER")
        known[row["bdf"]] = (binding(row["driver_binding"]),
                             {k: observation(v, messages) for k, v in counters.items()})
    seen_roots = {}
    for row in roots:
        check(type(row) is dict and row.keys() == {"bdf", "counters"}
              and type(row["bdf"]) is str and row["bdf"] in known and row["bdf"] not in seen_roots,
              "unknown or duplicate root observer")
        counters = row["counters"]
        check(type(counters) is dict and counters.keys() == set(ROOT_KEYS), "invalid PCIe root AER")
        seen_roots[row["bdf"]] = {k: observation(v, integer) for k, v in counters.items()}
    return known, seen_roots


def verify_planned_pcie_window(window, device, wait, elapsed):
    check(type(window) is dict and window.keys() == {
        "schema_version", "device", "requested_wait_ms", "sample_start_interval_ms",
        "before", "after", "delta",
    } and type(window["schema_version"]) is int and window["schema_version"] == 1
        and window["device"] == device and type(window["requested_wait_ms"]) is int
        and window["requested_wait_ms"] == wait, "PCIe health window differs from trusted plan")
    interval = window["sample_start_interval_ms"]
    check(type(interval) is int and wait <= interval <= elapsed, "invalid PCIe health window timing")
    a, roots_a = verify_planned_pcie_health(window["before"], device)
    b, roots_b = verify_planned_pcie_health(window["after"], device)
    reported = window["delta"]
    check(type(reported) is dict, "invalid PCIe delta")
    if reported.get("status") == "not_comparable":
        check(reported.keys() == {"status", "reason"} and reported["reason"] in (
            "boot_changed", "boot_identity_unavailable", "device_identity_changed",
            "driver_binding_changed", "driver_binding_indeterminate"
        ), "invalid withheld PCIe delta")
        return
    check(list(a) == list(b), "PCIe delta lacks consistent observer sequence")
    from compare_pcie_snapshots import DEVICE_KEYS, ROOT_KEYS, delta
    withheld = ("unavailable", None)
    rows = []
    for name, (bind_a, dev_a) in a.items():
        bind_b, dev_b = b[name]
        check(bind_a == bind_b and (bind_a[0] == "unbound" or
              (bind_a[0] == "bound" and bind_a[2][0] in ("observed", "unavailable"))),
              "PCIe delta lacks consistent driver binding")
        root_a, root_b = roots_a.get(name, {}), roots_b.get(name, {})
        rows.append({"bdf": name, "device_aer": {
            k: delta(dev_a[k], dev_b[k], True) for k in DEVICE_KEYS
        }, "root_hierarchy_aer": {
            k: delta(root_a.get(k, withheld), root_b.get(k, withheld), False) for k in ROOT_KEYS
        }})
    check(equal(reported, {"status": "compared", "by_observer": rows}),
          "reported PCIe delta disagrees with the snapshots")


def verify_plan(plan, observation, run_id, evidence_bytes, elapsed):
    check(type(plan) is dict and plan.keys() == {
        "plan_schema_version", "argv", "execution", "max_evidence_bytes", "optional"
    } and type(plan["plan_schema_version"]) is int and plan["plan_schema_version"] == 1
        and plan["execution"] == "none", "invalid passive plan")
    expected = ["qualify", "passive", "--run-id", run_id, "--evidence-json"]
    argv = plan["argv"]
    check(type(argv) is list and len(argv) >= 6 and argv[:5] == expected,
          "plan request does not match run identity")
    check_evidence_path(argv[5])
    expected.append(argv[5])
    required = None
    if argv[6:7] == ["--require-pcie"]:
        check(len(argv) >= 8, "expected an explicit PCIe request")
        required = pcie_expectations(argv[7])
        expected.extend(argv[6:8])
    edac = None
    pos = len(expected)
    if argv[pos:pos+1] == ["--edac-controller"]:
        check(len(argv) > pos + 1 and type(argv[pos+1]) is str
              and EDAC_CONTROLLER.fullmatch(argv[pos+1]), "invalid planned EDAC controller")
        edac = argv[pos+1]
        expected.extend(argv[pos:pos+2])
    nic = None
    pos = len(expected)
    if argv[pos:pos+1] == ["--network-interface"]:
        check(len(argv) >= pos + 4 and valid_nic_interface(argv[pos+1])
              and argv[pos+2] == "--network-health-sample-ms"
              and type(argv[pos+3]) is str and re.fullmatch(r"[1-9][0-9]{0,3}", argv[pos+3])
              and int(argv[pos+3]) <= 2000, "invalid planned NIC window")
        nic = (argv[pos+1], int(argv[pos+3]))
        expected.extend(argv[pos:pos+4])
    nvme = None
    pos = len(expected)
    if argv[pos:pos+1] == ["--nvme-controller"]:
        check(len(argv) > pos + 1 and type(argv[pos+1]) is str
              and NVME_CONTROLLER.fullmatch(argv[pos+1]), "invalid planned NVMe controller")
        nvme = argv[pos+1]
        expected.extend(argv[pos:pos+2])
    mmc = None
    pos = len(expected)
    if argv[pos:pos+1] == ["--mmc-card"]:
        check(len(argv) > pos + 1 and type(argv[pos+1]) is str
              and MMC_CARD.fullmatch(argv[pos+1]), "invalid planned MMC card")
        mmc = argv[pos+1]
        expected.extend(argv[pos:pos+2])
    thermal = None
    pos = len(expected)
    if argv[pos:pos+1] == ["--thermal-sample-ms"]:
        check(len(argv) > pos + 1 and type(argv[pos+1]) is str
              and re.fullmatch(r"[1-9][0-9]{0,3}", argv[pos+1])
              and int(argv[pos+1]) <= 2000, "invalid planned thermal window")
        thermal = int(argv[pos+1])
        expected.extend(argv[pos:pos+2])
    pcie_health = None
    pcie_wait = None
    pos = len(expected)
    if argv[pos:pos+1] == ["--pcie-health-device"]:
        check(len(argv) > pos + 1 and type(argv[pos+1]) is str
              and PCI_BDF.fullmatch(argv[pos+1]), "invalid planned PCIe health device")
        pcie_health = argv[pos+1]
        expected.extend(argv[pos:pos+2])
    pos = len(expected)
    if argv[pos:pos+1] == ["--pcie-health-sample-ms"]:
        check(pcie_health is not None and len(argv) > pos + 1 and type(argv[pos+1]) is str
              and re.fullmatch(r"[1-9][0-9]{0,3}", argv[pos+1])
              and int(argv[pos+1]) <= 2000, "invalid planned PCIe health wait")
        pcie_wait = int(argv[pos+1])
        expected.extend(argv[pos:pos+2])
    pressure_plan = None
    pos = len(expected)
    if argv[pos:pos+1] == ["--pressure-series-samples"]:
        check(len(argv) >= pos + 4 and type(argv[pos+1]) is str
              and re.fullmatch(r"[2-8]", argv[pos+1])
              and argv[pos+2] == "--pressure-series-gap-ms" and type(argv[pos+3]) is str
              and re.fullmatch(r"[1-9][0-9]{2}|1000", argv[pos+3]),
              "invalid planned pressure series")
        pressure_plan = (int(argv[pos+1]), int(argv[pos+3]))
        expected.extend(argv[pos:pos+4])
    cap = plan["max_evidence_bytes"]
    check(type(cap) is int and 0 < evidence_bytes <= cap <= MAX_BYTES,
          "evidence exceeds plan byte limit")
    rows = plan["optional"]
    check(type(rows) is list and len(rows) <= len(OPTIONAL_PASSIVE), "invalid planned options")
    planned = {"edac_health"} if edac is not None else set()
    if edac is not None:
        actual_edac = observation.get("edac_health")
        check(type(actual_edac) is dict and actual_edac.get("controller") == edac,
              "EDAC controller differs from plan")
    if nvme is not None:
        verify_nvme_controller(observation.get("nvme_health"), nvme)
        planned.add("nvme_health")
    if nic is not None:
        network = observation.get("network_health")
        check(type(network) is dict and type(network.get("before")) is dict
              and type(network.get("after")) is dict
              and network["before"].get("interface") == network["after"].get("interface") == nic[0]
              and type(network.get("requested_wait_ms")) is int
              and network["requested_wait_ms"] == nic[1], "NIC window differs from trusted plan")
        planned.add("network_health")
    if mmc is not None:
        from check_mmc_health_evidence import validate_snapshot
        validate_snapshot(observation.get("mmc_health"), mmc)
        planned.add("mmc_health")
    if pcie_health is not None:
        if pcie_wait is None:
            verify_planned_pcie_health(observation.get("pcie_health"), pcie_health)
        else:
            verify_planned_pcie_window(observation.get("pcie_health"), pcie_health, pcie_wait, elapsed)
        planned.add("pcie_health")
    seen = set()
    for row in rows:
        check(type(row) is dict and row.keys() == {"feature", "status"}
              and type(row["feature"]) is str and row["feature"] in OPTIONAL_PASSIVE
              and row["feature"] not in seen and row["status"] in ("planned", "unsupported"),
              "invalid planned option")
        name = row["feature"]
        seen.add(name)
        if row["status"] == "planned":
            _, flag, field = OPTIONAL_PASSIVE[name]
            expected.append(flag)
            planned.add(field)
    check(argv == expected, "argv disagrees with planned options")
    observed = set(observation) - {
        "qualification_schema_version", "kind", "scope", "workload_execution",
        "run_id", "inventory", "board_check",
    }
    check(observed == planned and all(type(observation[k]) is dict for k in planned),
          "observation features disagree with plan")
    health = observation.get("system_health", {})
    if "system_health" in planned:
        required_health = {"schema_version", "numa", "pressure"}
        check(health.keys() in (required_health, required_health | {"pressure_series"})
              and type(health["schema_version"]) is int and health["schema_version"] == 1,
              "planned system health must include schema 1 NUMA and pressure")
    series = health.get("pressure_series")
    if pressure_plan is None:
        check("pressure_series" not in health, "unplanned pressure series")
    else:
        check("system_health" in planned and type(series) is dict
              and (series.get("sample_count"), series.get("requested_gap_ms")) == pressure_plan,
              "pressure series differs from trusted plan")
    inventory = observation["inventory"]
    if thermal is None:
        check("thermal_window" not in inventory, "unplanned thermal sample")
    else:
        window = inventory.get("thermal_window")
        check(type(window) is dict and type(window.get("requested_wait_ms")) is int
              and window["requested_wait_ms"] == thermal,
              "thermal window differs from trusted plan")
    check("board_profile" not in inventory, "unplanned board profile")
    if required is None:
        check("required_pcie" not in inventory
              and observation.get("board_check", "not_configured") == "not_configured",
              "unplanned board expectation")
    else:
        rows = inventory.get("required_pcie")
        check(observation.get("board_check") == "see_required_pcie" and type(rows) is list,
              "planned PCIe rows missing")
        actual = [(row["bdf"], row.get("expected")) for row in rows]
        check(actual == required, "PCIe rows disagree with plan")


def verify(evidence, stdout, run_id, plan=None):
    check(isinstance(run_id, str) and RUN_ID.fullmatch(run_id)
          and run_id != "0" * 32, "invalid expected run ID")
    saved = parse(evidence)
    expected = {"evidence_schema_version", "kind", "started_unix_ms",
                "finished_unix_ms", "elapsed_monotonic_ms", "qualification"}
    check(saved.keys() == expected and type(saved["evidence_schema_version"]) is int
          and saved["evidence_schema_version"] == 1
          and saved["kind"] == "passive_qualification", "unsupported evidence envelope")
    for field in ("started_unix_ms", "finished_unix_ms", "elapsed_monotonic_ms"):
        check(type(saved[field]) is int and saved[field] >= 0, "invalid timestamp or duration")
    q = parse(stdout)
    check(type(q.get("qualification_schema_version")) is int
          and q["qualification_schema_version"] == 1
          and q.get("kind") == "passive_qualification_observation"
          and q.get("scope") == "bring-up" and q.get("workload_execution") == "none"
          and type(q.get("inventory")) is dict, "unsupported passive observation")
    check(q.get("run_id") == run_id, "run ID mismatch")
    check(equal(saved["qualification"], q), "saved qualification differs from command output")
    health = q.get("system_health")
    if type(health) is dict and "numa" in health:
        verify_numa_inventory(health["numa"])
    if type(health) is dict and "pressure" in health:
        verify_pressure_snapshot(health["pressure"])
    if type(health) is dict and "pressure_series" in health:
        verify_pressure_series(health["pressure_series"], saved["elapsed_monotonic_ms"])
    if "network_health" in q:
        network = q["network_health"]
        check(type(network) is dict, "invalid passive network observation")
        if "mode" in network:
            verify_network_window(network, saved["elapsed_monotonic_ms"])
        else:
            # Released v0.3.0 could emit a single snapshot. It has no interval
            # or delta proof, but every reported field still needs validation.
            from check_network_health_window import snapshot
            interface = network.get("interface")
            check(valid_nic_interface(interface), "invalid snapshot interface")
            snapshot(network, interface)
    if "nvme_health" in q:
        verify_nvme_controller(q["nvme_health"])
    if "mmc_health" in q:
        from check_mmc_health_evidence import validate_snapshot
        mmc = q["mmc_health"]
        check(type(mmc) is dict, "invalid MMC report object")
        validate_snapshot(mmc, mmc.get("card"))
    if "edac_health" in q:
        verify_edac_health(q["edac_health"])
    if "required_pcie" in q["inventory"]:
        verify_required_pcie(q["inventory"]["required_pcie"])
    if q.get("board_check") == "see_required_pcie":
        check("required_pcie" in q["inventory"], "requested PCIe rows were omitted")
    if "cpu_frequency" in q["inventory"]:
        verify_cpu_frequency(q["inventory"]["cpu_frequency"])
    if "cpu_thermal_throttle" in q["inventory"]:
        verify_cpu_throttle(q["inventory"]["cpu_thermal_throttle"])
    if "powercap" in q["inventory"]:
        verify_powercap(q["inventory"]["powercap"])
    if "thermal_window" in q["inventory"]:
        verify_thermal_window(q["inventory"], saved["elapsed_monotonic_ms"])
    if "observation_timeline" in q:
        verify_timeline(q, saved["elapsed_monotonic_ms"])
    if plan is not None:
        verify_plan(plan, q, run_id, len(evidence), saved["elapsed_monotonic_ms"])
    return {"status": "correlated", "run_id": run_id,
            "evidence_bytes": len(evidence), "sha256": hashlib.sha256(evidence).hexdigest(),
            "hardware_authenticity": "not_verified"}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--evidence", required=True)
    p.add_argument("--stdout", required=True)
    p.add_argument("--run-id", required=True)
    p.add_argument("--plan", help="previously generated trusted passive plan; never executed")
    a = p.parse_args()
    try:
        plan_data = read_bounded(a.plan) if a.plan else None
        plan = parse(plan_data) if plan_data is not None else None
        receipt = verify(read_bounded(a.evidence), read_bounded(a.stdout), a.run_id, plan)
        if plan_data is not None:
            receipt["plan_sha256"] = hashlib.sha256(plan_data).hexdigest()
    except (OSError, ValueError, RecursionError) as exc:
        p.exit(1, f"Evidence rejected: {exc}\n")
    print(json.dumps(receipt, sort_keys=True))


if __name__ == "__main__":
    main()
