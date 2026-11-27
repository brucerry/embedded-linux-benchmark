"""Run the optional synthetic NVML collector test; never load NVIDIA's library."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile


def check(binary):
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
    invalid = subprocess.run(
        [binary, "--device", "0000:01:20.0"], capture_output=True, text=True, timeout=3
    )
    if (invalid.returncode, invalid.stdout) != (2, ""):
        raise AssertionError("invalid BDF was not rejected")
    print(f"{len(cases) + 1} synthetic NVML ABI cases passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--include-dir", required=True, type=Path, help="official nvml.h directory")
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
        check(binary)


if __name__ == "__main__":
    main()
