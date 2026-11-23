"""Exercise UART with Linux pseudoterminals, without opening serial hardware."""

import os
import pty
import select
import subprocess
import sys
import termios
import time


def check_transfer(command, mode):
    master, slave = pty.openpty()
    process = None
    try:
        original = termios.tcgetattr(slave)
        path = os.ttyname(slave)
        length = 4099  # Exercises multiple chunks and a partial final chunk.
        args = command + [
            "uart", "--device", path, "--loopback", "--bytes", str(length),
            "--timeout-ms", "250" if mode == "idle" else "3000",
        ]
        process = subprocess.Popen(
            args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        echoed = 0
        end = time.monotonic() + 4
        while mode != "idle" and process.poll() is None and time.monotonic() < end:
            readable, _, _ = select.select([master], [], [], 0.02)
            if not readable:
                continue
            chunk = os.read(master, 512)
            if mode == "corrupt":
                chunk = bytes([chunk[0] ^ 1]) + chunk[1:]
            assert os.write(master, chunk) == len(chunk)
            echoed += len(chunk)
        stdout, stderr = process.communicate(timeout=5)
        assert termios.tcgetattr(slave) == original, (mode, stdout, stderr)
        if mode == "echo":
            assert process.returncode == 0 and not stderr, (stdout, stderr)
            assert echoed == length and f"Bytes verified: {length}\n".encode() in stdout
        else:
            expected = b"payload mismatch" if mode == "corrupt" else b"time limit reached"
            assert process.returncode == 1 and not stdout and expected in stderr, (stdout, stderr)
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait()
        os.close(slave)
        os.close(master)


def check_stream_and_controlling_terminal(command):
    master, slave = pty.openpty()
    try:
        path = os.ttyname(slave)
        original = termios.tcgetattr(slave)
        args = command + ["uart", "--device", path, "--loopback"]
        shared = subprocess.run(args, stdin=slave, capture_output=True, timeout=5)
        assert shared.returncode == 1 and b"standard stream" in shared.stderr, shared
        # Acquire the test PTY as a controlling terminal with all standard streams redirected.
        setup = (
            "import os,sys; os.setsid(); "
            "os.close(os.open(sys.argv[1], os.O_RDONLY)); os.execvp(sys.argv[2], sys.argv[2:])"
        )
        controlling = subprocess.run(
            [sys.executable, "-c", setup, path, *args], stdin=subprocess.DEVNULL,
            capture_output=True, timeout=5,
        )
        assert controlling.returncode == 1 and b"controlling terminal" in controlling.stderr, controlling
        assert termios.tcgetattr(slave) == original
    finally:
        os.close(slave)
        os.close(master)


if __name__ == "__main__":
    if len(sys.argv) < 2:
        raise SystemExit("usage: python3 tests/uart_pty.py [runner ...] benchmark-binary")
    for scenario in ["echo", "idle", "corrupt"]:
        check_transfer(sys.argv[1:], scenario)
    check_stream_and_controlling_terminal(sys.argv[1:])
    print("PTY payload, timeout, corruption, terminal refusal, and tty restoration passed.")
