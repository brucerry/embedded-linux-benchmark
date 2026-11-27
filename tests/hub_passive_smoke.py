"""Exercise plan and evidence correlation with a trusted, locally built Engine."""
import argparse
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


def smoke(binary):
    with tempfile.TemporaryDirectory(prefix="eld-hub-passive-") as directory:
        cap = parse(invoke(binary, "capabilities"))
        profiles = (
            ([], None),
            (["system-health", "timeline"], None),
            # With no dependence on what is actually installed in the runner.
            (["timeline"], "0000:00:00.0=0000:0000"),
        )
        for i, (options, required_pcie) in enumerate(profiles):
            token = secrets.token_hex(16)
            output = Path(directory) / f"evidence-{i}.json"
            preview = plan(cap, token, str(output), options, required_pcie)
            if any(row["status"] != "planned" for row in preview["optional"]):
                raise ValueError("local build did not advertise expected passive options")
            # Use only the deterministic argv from the local capability planner.
            stdout = invoke(binary, *preview["argv"])
            result = verify(read_bounded(output), stdout, token, preview)
            if result["status"] != "correlated" or result["hardware_authenticity"] != "not_verified":
                raise ValueError("unexpected offline validation result")
        token = secrets.token_hex(16)
        output = Path(directory) / "evidence-windows.json"
        stdout = invoke(
            binary, "qualify", "passive", "--run-id", token, "--evidence-json", str(output),
            "--network-interface", "lo", "--network-health-sample-ms", "5",
            "--thermal-sample-ms", "5", "--include-system-health",
            "--pressure-series-samples", "2", "--pressure-series-gap-ms", "100",
            "--include-timeline",
        )
        observation = parse(stdout)
        required = (
            observation["network_health"]["requested_wait_ms"],
            observation["inventory"]["thermal_window"]["requested_wait_ms"],
            observation["system_health"]["pressure_series"]["sample_count"],
        )
        if required != (5, 5, 2) or not observation["observation_timeline"]["phases"]:
            raise ValueError("passive sample windows were missing or did not match the invocation")
        receipt = verify(read_bounded(output), stdout, token)
        if receipt["status"] != "correlated" or receipt["hardware_authenticity"] != "not_verified":
            raise ValueError("unexpected window validation result")
    print("Engine passive evidence smoke passed (baseline, negotiated options, PCIe plan, and read windows)")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("engine", type=Path, help="trusted locally built Engine executable")
    smoke(parser.parse_args().engine.resolve(strict=True))
