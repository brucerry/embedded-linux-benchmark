"""Exercise plan and evidence correlation with a trusted, locally built Engine."""
import argparse
from copy import deepcopy
import hashlib
from pathlib import Path
import secrets
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_hub_evidence import parse, read_bounded, verify  # noqa: E402
from plan_hub_passive import plan  # noqa: E402


def invoke(binary, *args):
    return subprocess.run([str(binary), *args], check=True, capture_output=True,
                          timeout=15).stdout


def smoke_missing(invoke_local, directory, cap):
    # Nonexistent targets only: never substitute a real interface or controller.
    nic = "eld" + secrets.token_hex(5)
    nvme = "nvme9999999"
    for target in (Path("/sys/class/net") / nic, Path("/sys/class/nvme") / nvme):
        if target.exists() or target.is_symlink():
            raise ValueError("test target unexpectedly exists")
    token = secrets.token_hex(16)
    output = Path(directory) / "evidence-missing-nic-nvme.json"
    preview = plan(cap, token, str(output), ["timeline"],
                   network_interface=nic, network_sample_ms=5, nvme_controller=nvme)
    stdout = invoke_local(*preview["argv"])
    saved = read_bounded(output)
    result = verify(saved, stdout, token, preview)
    if result["status"] != "correlated" or result["hardware_authenticity"] != "not_verified":
        raise ValueError("unexpected missing-device validation result")
    actual = parse(stdout)
    window = actual["network_health"]
    if (window["before"]["status"], window["after"]["status"], window["delta"]["status"],
            actual["nvme_health"]["status"], actual["nvme_health"]["smart_sampled"]) != (
                "unavailable", "unavailable", "not_comparable", "unavailable", False):
        raise ValueError("missing devices did not stay explicitly unavailable")
    for flag, alternate in (("--network-interface", "lo"), ("--network-health-sample-ms", "6"),
                            ("--nvme-controller", "nvme0")):
        other = deepcopy(preview)
        other["argv"][other["argv"].index(flag) + 1] = alternate
        try:
            verify(saved, stdout, token, other)
        except ValueError as exc:
            if "differs from trusted plan" not in str(exc):
                raise
        else:
            raise ValueError("device evidence matched a different trusted plan")
    return result


def smoke_selected_pcie(binary, directory, cap, device):
    token = secrets.token_hex(16)
    output = Path(directory) / "planned-pcie-window.json"
    planned = plan(cap, token, str(output), ["timeline"], pcie_health_device=device,
                   pcie_health_sample_ms=5)
    stdout = invoke(binary, *planned["argv"])
    saved = read_bounded(output)
    result = verify(saved, stdout, token, planned)
    record = parse(stdout)
    if (result["status"] != "correlated" or result["hardware_authenticity"] != "not_verified"
            or record["pcie_health"]["device"] != device):
        raise ValueError("unexpected PCIe comparison scope")
    other = deepcopy(planned)
    other["argv"][other["argv"].index("--pcie-health-sample-ms") + 1] = "6"
    try:
        verify(saved, stdout, token, other)
    except ValueError as exc:
        if "window differs from trusted plan" not in str(exc):
            raise
    else:
        raise ValueError("PCIe report was accepted for another sampling interval")
    print("Selected passive PCIe window and mismatched-plan refusal passed")


def smoke_standalone_mmc(binary, directory, card, expected_snapshot):
    snapshot = invoke(binary, "storage", "mmc-health", "--card", card)
    if parse(snapshot) != expected_snapshot:
        raise ValueError("standalone and qualification MMC observations differ")
    report = Path(directory) / "mmc-standalone.json"
    report.write_bytes(snapshot)
    digest = hashlib.sha256(snapshot).hexdigest()
    checker = Path(__file__).resolve().parents[1] / "scripts" / "check_mmc_health_evidence.py"
    argv = [sys.executable, "-B", str(checker), "--report", str(report), "--card", card,
            "--expected-sha256", digest]
    result = parse(subprocess.run(argv, check=True, capture_output=True, timeout=15).stdout)
    expected = {
        "schema_version": 1, "kind": "offline_mmc_report_check", "card": card,
        "card_observation": "unavailable", "raw_field_states": {}, "report_sha256": digest,
        "digest_selection": "matched", "hardware_authenticity": "not_verified",
        "wear_assessment": "not_assessed", "device_sampled": False,
    }
    if result != expected:
        raise ValueError("standalone MMC report invented readings or provenance")
    report.write_bytes(snapshot + b" ")
    reject = subprocess.run(argv, capture_output=True, timeout=15)
    if reject.returncode != 1 or reject.stdout or b"differs from expected SHA-256" not in reject.stderr:
        raise ValueError("offline MMC checker did not reject changed report bytes")


def smoke(binary, pcie_health_device=None):
    with tempfile.TemporaryDirectory(prefix="eld-hub-passive-") as directory:
        cap = parse(invoke(binary, "capabilities"))
        absent_edac = "mc9999999"
        edac_path = Path("/sys/devices/system/edac/mc") / absent_edac
        if edac_path.exists() or edac_path.is_symlink():
            raise ValueError("test EDAC controller unexpectedly exists")
        profiles = (
            ([], None, None),
            (["system-health", "timeline"], None, None),
            # With no dependence on what is actually installed in the runner.
            (["timeline"], "0000:00:00.0=0000:0000", None),
            (["timeline"], None, absent_edac),
        )
        for i, (options, required_pcie, edac_controller) in enumerate(profiles):
            token = secrets.token_hex(16)
            output = Path(directory) / f"evidence-{i}.json"
            preview = plan(cap, token, str(output), options, required_pcie, edac_controller)
            if any(row["status"] != "planned" for row in preview["optional"]):
                raise ValueError("local build did not advertise expected passive options")
            # Use only the deterministic argv from the local capability planner.
            stdout = invoke(binary, *preview["argv"])
            result = verify(read_bounded(output), stdout, token, preview)
            actual = parse(stdout)
            if edac_controller is None and "edac_health" in actual:
                raise ValueError("unexpected EDAC section by default")
            if edac_controller is not None:
                edac = actual.get("edac_health", {})
                expected = {"schema_version": 1, "kind": "linux_edac_memory",
                            "controller": edac_controller, "source": "linux_sysfs",
                            "result": {"status": "unavailable"}}
                if edac != expected:
                    raise ValueError("missing EDAC controller was not explicitly unavailable")
                standalone = invoke(binary, "ram", "edac", "--controller", absent_edac)
                if parse(standalone) != expected:
                    raise ValueError("standalone and qualification EDAC observations differ")
                capture = Path(directory) / "edac-standalone.json"
                capture.write_bytes(standalone)
                digest = hashlib.sha256(standalone).hexdigest()
                checker = Path(__file__).resolve().parents[1] / "scripts" / "check_edac_health_evidence.py"
                receipt = parse(invoke(sys.executable, "-B", str(checker), "--report", str(capture),
                                       "--controller", absent_edac, "--expected-sha256", digest))
                if (receipt["digest_selection"], receipt["result"], receipt["memory_health"]) != (
                        "matched", {"status": "unavailable"}, "not_assessed"):
                    raise ValueError("standalone EDAC checker altered the unavailable state")
                preview["argv"][preview["argv"].index("--edac-controller") + 1] = "mc0"
                try:
                    verify(read_bounded(output), stdout, token, preview)
                except ValueError as exc:
                    if "EDAC controller differs from plan" not in str(exc):
                        raise
                else:
                    raise ValueError("EDAC evidence matched the wrong controller")

            if result["status"] != "correlated" or result["hardware_authenticity"] != "not_verified":
                raise ValueError("unexpected offline validation result")
        smoke_missing(lambda *args: invoke(binary, *args), directory, cap)
        card = "mmc9999999:abcd"
        card_sysfs = Path("/sys/bus/mmc/devices") / card
        if card_sysfs.exists() or card_sysfs.is_symlink():
            raise ValueError("test card unexpectedly exists")
        if cap["contracts"]["passive_mmc_health"]["scope"] != "cached_sysfs_only":
            raise ValueError("wrong optional MMC capability")
        token = secrets.token_hex(16)
        output = Path(directory) / "evidence-mmc.json"
        planned = plan(cap, token, str(output), ["timeline"], mmc_card=card)
        stdout = invoke(binary, *planned["argv"])
        observation = parse(stdout)
        if observation["mmc_health"] != {"schema_version": 1, "card": card, "status": "unavailable"}:
            raise ValueError("absent MMC was not reported as unavailable")
        smoke_standalone_mmc(binary, directory, card, observation["mmc_health"])
        phases = [phase["phase"] for phase in observation["observation_timeline"]["phases"]]
        if phases != ["mmc_health", "inventory"]:
            raise ValueError("unexpected MMC observation phase order")
        saved = read_bounded(output)
        if verify(saved, stdout, token, planned)["status"] != "correlated":
            raise ValueError("MMC report did not correlate")
        other = deepcopy(planned)
        other["argv"][other["argv"].index("--mmc-card") + 1] = "mmc9999998:abcd"
        try:
            verify(saved, stdout, token, other)
        except ValueError as exc:
            if "selected card mismatch" not in str(exc):
                raise
        else:
            raise ValueError("MMC report was accepted for the wrong card")
        token = secrets.token_hex(16)
        thermal_output = Path(directory) / "planned-thermal.json"
        thermal = plan(cap, token, str(thermal_output), [], thermal_sample_ms=5)
        thermal_stdout = invoke(binary, *thermal["argv"])
        thermal_saved = read_bounded(thermal_output)
        if verify(thermal_saved, thermal_stdout, token, thermal)["status"] != "correlated":
            raise ValueError("planned thermal evidence did not correlate")
        changed_window = deepcopy(thermal)
        changed_window["argv"][-1] = "6"
        try:
            verify(thermal_saved, thermal_stdout, token, changed_window)
        except ValueError as exc:
            if "thermal window differs" not in str(exc):
                raise
        else:
            raise ValueError("planned thermal evidence accepted a different duration")
        token = secrets.token_hex(16)
        output = Path(directory) / "evidence-windows.json"
        planned = plan(cap, token, str(output), ["system-health", "timeline"],
                       network_interface="lo", network_sample_ms=5, thermal_sample_ms=5,
                       pressure_samples=2, pressure_gap_ms=100)
        stdout = invoke(binary, *planned["argv"])
        observation = parse(stdout)
        required = (
            observation["network_health"]["requested_wait_ms"],
            observation["inventory"]["thermal_window"]["requested_wait_ms"],
            observation["system_health"]["pressure_series"]["sample_count"],
        )
        if required != (5, 5, 2) or not observation["observation_timeline"]["phases"]:
            raise ValueError("passive sample windows were missing or did not match the invocation")
        saved = read_bounded(output)
        receipt = verify(saved, stdout, token, planned)
        other = deepcopy(planned)
        other["argv"][other["argv"].index("--pressure-series-gap-ms") + 1] = "101"
        try:
            verify(saved, stdout, token, other)
        except ValueError as exc:
            if "pressure series differs from trusted plan" not in str(exc):
                raise
        else:
            raise ValueError("pressure evidence matched a different trusted plan")
        if receipt["status"] != "correlated" or receipt["hardware_authenticity"] != "not_verified":
            raise ValueError("unexpected window validation result")
        if pcie_health_device is not None:
            smoke_selected_pcie(binary, directory, cap, pcie_health_device)
    print("Engine passive evidence smoke passed (baseline, negotiated options, PCIe/EDAC plan, missing NIC/NVMe, and read windows)")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("engine", type=Path, help="trusted locally built Engine executable")
    parser.add_argument("--pcie-health-device", help="opt in to one known local BDF; no auto-pick")
    parsed = parser.parse_args()
    smoke(parsed.engine.resolve(strict=True), parsed.pcie_health_device)
