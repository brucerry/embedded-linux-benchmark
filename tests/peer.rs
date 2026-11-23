use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};

const SESSION: &str = "0123456789abcdef0123456789abcdef";

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_embedded-linux-benchmark"))
}

struct Server {
    child: Child,
    output: BufReader<std::process::ChildStdout>,
    port: String,
}

impl Server {
    fn start() -> Self {
        let mut child = command()
            .args([
                "network",
                "peer-server",
                "--bind",
                "127.0.0.2",
                "--peer",
                "127.0.0.1",
                "--port",
                "0",
                "--session",
                SESSION,
                "--size-mib",
                "1",
                "--pings",
                "4",
                "--timeout-ms",
                "3000",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut ready = String::new();
        output.read_line(&mut ready).unwrap();
        let port = ready
            .trim()
            .strip_prefix("Peer listener: 127.0.0.2:")
            .unwrap()
            .to_owned();
        Self {
            child,
            output,
            port,
        }
    }

    fn client(&self, pings: &str) -> std::process::Output {
        command()
            .args([
                "network",
                "peer-client",
                "--bind",
                "127.0.0.1",
                "--peer",
                "127.0.0.2",
                "--port",
                &self.port,
                "--session",
                SESSION,
                "--size-mib",
                "1",
                "--pings",
                pings,
                "--timeout-ms",
                "3000",
            ])
            .output()
            .unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn peers_measure_verified_bytes_and_round_trips() {
    let mut server = Server::start();
    let client = server.client("4");
    assert!(client.status.success(), "{client:?}");
    let output = String::from_utf8(client.stdout).unwrap();
    assert!(output.contains("Probes: 4\n"));
    assert!(output.contains("RTT min / mean / p95:"));
    assert!(output.contains("Bytes verified by peer: 1048576\n"));

    let mut output = String::new();
    server.output.read_to_string(&mut output).unwrap();
    assert!(server.child.wait().unwrap().success());
    assert!(output.contains("Bytes verified: 1048576\n"));
}

#[test]
fn peer_refuses_mismatched_budget_and_unsafe_addresses() {
    let mut server = Server::start();
    let client = server.client("5");
    assert!(!client.status.success());
    assert!(!server.child.wait().unwrap().success());
    let mut error = String::new();
    server
        .child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut error)
        .unwrap();
    assert!(error.contains("session, or budgets do not match"));

    for peer in [
        "0.0.0.0",
        "224.0.0.1",
        "192.0.2.55",
        "192.168.1.255",
        "localhost",
    ] {
        let rejected = command()
            .args([
                "network",
                "peer-client",
                "--bind",
                "127.0.0.1",
                "--peer",
                peer,
                "--port",
                "1",
                "--session",
                SESSION,
            ])
            .output()
            .unwrap();
        assert!(!rejected.status.success(), "{peer}");
        assert!(rejected.stdout.is_empty());
    }
}
