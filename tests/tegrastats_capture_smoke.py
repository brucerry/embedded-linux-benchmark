"""Exercise the Engine's offline Jetson telemetry entry point using inert text."""
import argparse
import json
from pathlib import Path
import subprocess


def smoke(engine):
    cmd = [engine, "accelerator", "tegrastats-decode", "--rail", "VDD_GPU_SOC"]

    def run(data):
        return subprocess.run(cmd, input=data, capture_output=True, timeout=5)

    sample = b"RAM 20/100MB GR3D_FREQ 0%@[306,0,1020] cpu@54C gpu@41.025C VDD_GPU_SOC 7mW/8mW\n"
    result = run(sample)
    assert result.returncode == 0, result.stderr
    row = json.loads(result.stdout)
    assert row["kind"] == "jetson_tegrastats_capture"
    assert row["live_query"] == "not_performed" and row["capture_identity"] == "unverified"
    assert row["gpu_utilization_percent"] == {"status": "observed", "value": 0}
    assert row["gpu_frequency_mhz"] == {"status": "observed", "value": [306, 0, 1020]}
    assert row["gpu_temperature_millicelsius"] == {"status": "observed", "value": 41025}
    assert row["selected_rail"]["observation"]["value"] == {"instant_mw": 7, "average_mw": 8}
    absent = run(b"GR3D_FREQ @[888,0]\n")
    assert absent.returncode == 0
    fields = json.loads(absent.stdout)
    assert fields["gpu_utilization_percent"] == {"status": "unavailable"}
    assert fields["gpu_temperature_millicelsius"] == {"status": "unavailable"}
    assert fields["selected_rail"]["observation"] == {"status": "unavailable"}
    for data in (sample + sample, b"GR3D_FREQ 101%\n", b"gpu@41C VDD_GPU_SOC 0mW/xx",
                 b" " * 4096 + b"gpu@41C"):
        refused = run(data)
        assert refused.returncode != 0 and not refused.stdout
    print("Offline Jetson capture CLI preserved zero and missing fields and refused malformed data")


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("engine", type=Path, help="trusted locally built Engine executable")
    smoke(str(cli.parse_args().engine.resolve(strict=True)))
