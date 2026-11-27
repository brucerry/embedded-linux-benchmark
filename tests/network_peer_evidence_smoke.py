"""Verify real Engine peer receipts on loopback with no physical network traffic."""
import argparse
from pathlib import Path
import secrets
import select
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_network_peer_evidence import correlate  # noqa: E402


def smoke(exe):
    a, b = "127.0.0.1", "127.0.0.2"
    token = secrets.token_hex(16)
    common = ["--session", token, "--size-mib", "1", "--pings", "4",
              "--timeout-ms", "5000", "--peer-receipt"]
    with subprocess.Popen([exe, "network", "peer-server", "--bind", b, "--peer", a,
                           "--port", "0", *common], stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE) as server:
        try:
            if not select.select([server.stdout], [], [], 3)[0]:
                raise ValueError("peer fixture did not publish its loopback listener")
            first = server.stdout.readline()
            prefix = f"Peer listener: {b}:".encode()
            if not first.startswith(prefix):
                raise ValueError("peer fixture did not bind the expected loopback address")
            port = int(first[len(prefix):])
            sender = subprocess.run([exe, "network", "peer-client", "--bind", a, "--peer", b,
                                     "--port", str(port), *common], capture_output=True, timeout=6)
            rest, _ = server.communicate(timeout=6)
            receiver = first + rest
        finally:
            if server.poll() is None:
                server.kill()
                server.communicate()
    args = (a, b, port, token, 1, 4, sender.returncode, server.returncode)
    match = correlate(sender.stdout, receiver, *args)
    if (match["status"], match["physical_path"]) != ("correlated", "not_verified"):
        raise ValueError("unexpected peer receipt or physical path claim")
    for tampered in (receiver.replace(b'"probes":4', b'"probes":5'),
                     receiver.replace(token.encode(), b"0" * 32),
                     receiver + receiver):
        if tampered == receiver:
            raise ValueError("tamper fixture did not alter the captured receipt")
        try:
            correlate(sender.stdout, tampered, *args)
        except ValueError:
            continue
        raise ValueError("peer verifier accepted a mismatched or duplicate receipt")
    print("Engine peer send/receive receipts matched on loopback; all mismatch fixtures refused")


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("engine", type=Path, help="trusted locally built Engine executable")
    smoke(str(cli.parse_args().engine.resolve(strict=True)))
