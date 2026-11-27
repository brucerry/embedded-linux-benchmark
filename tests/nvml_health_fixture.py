"""Run the optional synthetic NVML collector test; never load NVIDIA's library."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def check_decoder(engine, capture, state):
    selected = "0000:01:00.0"
    argv = [engine, "accelerator", "nvidia-health-decode", "--device"]
    # Remove exactly the one line terminator emitted by the C helper.
    literal = capture.removesuffix("\n")
    good = subprocess.run([*argv, selected, "--capture-text", literal],
                          capture_output=True, text=True, timeout=3, check=True)
    data = json.loads(good.stdout)
    expected = {
        "success": ("43", "130000", "9007199254740993", "0"),
        "partial": ("43", "inaccessible", "unsupported", "0"),
        "negative": ("unavailable", "130000", "9007199254740993", "0"),
    }[state]
    obs = lambda x: {"status": "observed", "value": x} if x.isdecimal() else {"status": x}
    for key, value in zip(("temperature_celsius", "power_milliwatts",
                           "corrected_ecc", "uncorrected_ecc"), expected):
        if data.get(key) != obs(value):
            raise AssertionError(f"{state}: unexpected decoded {key}")
    if data.get("bdf") != selected or data.get("live_query") != "not_performed":
        raise AssertionError("decoder did not preserve BDF attribution and offline scope")
    wrong = subprocess.run([*argv, "0000:02:00.0", "--capture-text", literal],
                           capture_output=True, text=True, timeout=3)
    if wrong.returncode == 0 or wrong.stdout:
        raise AssertionError("decoder accepted capture from a different BDF")


def check(binary, engine=None):
    prefix = "eld_nvml_v1,0000:01:00.0,"
    cases = [
        ("success", "43,130000,9007199254740993,0"),
        ("partial", "43,inaccessible,unsupported,0"),
        ("negative", "unavailable,130000,9007199254740993,0"),
        *[(state, None) for state in (
            "wrong-bdf", "changed-bdf", "changed-uuid", "shutdown-failure", "init-failure"
        )],
    ]
    for state, out in cases:
        result = subprocess.run(
            [binary, "--device", "00000000:01:00.0"],
            env={**os.environ, "TEST_STATE": state}, capture_output=True, text=True, timeout=3,
        )
        if "private-" in result.stdout + result.stderr:
            raise AssertionError(f"{state}: leaked fixture identity")
        expected = (0, prefix + out + "\n") if out is not None else (1, "")
        if (result.returncode, result.stdout) != expected:
            raise AssertionError(f"{state}: unexpected result {result}")
        if engine is not None and out is not None:
            check_decoder(engine, result.stdout, state)
    invalid = subprocess.run(
        [binary, "--device", "0000:01:20.0"], capture_output=True, text=True, timeout=3
    )
    if (invalid.returncode, invalid.stdout) != (2, ""):
        raise AssertionError("invalid BDF was not rejected")
    print(f"{len(cases) + 1} synthetic NVML ABI cases passed")
    if engine is not None:
        print("3 synthetic collector-to-CLI decodes and wrong-BDF refusals passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--include-dir", required=True, type=Path, help="official nvml.h directory")
    parser.add_argument("--engine", type=Path, help="trusted eld-engine binary for decoder coverage")
    args = parser.parse_args()
    if not (args.include_dir / "nvml.h").is_file():
        parser.error("--include-dir must contain an NVIDIA NVML header")
    root = Path(__file__).resolve().parent.parent
    with tempfile.TemporaryDirectory(prefix="eld-nvml-fixture-") as work:
        binary = str(Path(work) / "collector")
        subprocess.run(
            [os.getenv("CC", "cc"), "-std=c11", "-Wall", "-Wextra", "-Werror", "-pedantic",
             "-isystem", str(args.include_dir.resolve()), str(root / "cuda/nvml-health.c"),
             str(root / "tests/nvml_health_fixture.c"), "-o", binary],
            check=True, timeout=30,
        )
        check(binary, str(args.engine.resolve(strict=True)) if args.engine else None)


if __name__ == "__main__":
    main()
