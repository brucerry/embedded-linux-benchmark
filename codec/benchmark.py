#!/usr/bin/env python3
"""Optional, fixed-fixture NVIDIA Jetson H.264 benchmarks."""

import argparse
import hashlib
import os
from pathlib import Path
import re
import signal
import statistics
import subprocess
import sys
import threading
import time


FRAMES = 60
WIDTH = 640
HEIGHT = 480
FIXTURE_LIMIT = 256 * 1024
ACCESS_UNIT_LIMIT = 1024 * 1024
FIXTURE_SHA256 = "c10bdf30175896287ad0c46b0bb14e3dee88e91f95ec23b8ee2b2295caf79a27"
ANNEX_B_START = re.compile(rb"\x00\x00(?:\x00)?\x01")
ENCODE_FACTORY = "nvv4l2h264enc"
DECODE_FACTORY = "nvv4l2decoder"
NVIDIA_PLUGIN = "nvvideo4linux2"
SINK = "appsink name=result emit-signals=true sync=false max-buffers=2"


def arguments(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("encode", "decode"))
    parser.add_argument("--backend", choices=("jetson-v4l2",), required=True)
    parser.add_argument("--timeout-ms", type=int, default=10000)
    parser.add_argument("--worker", action="store_true", help=argparse.SUPPRESS)
    options = parser.parse_args(argv)
    if not 250 <= options.timeout_ms <= 30000:
        parser.error("'--timeout-ms' must be between 250 and 30000")
    return options


def fixture_bytes():
    with Path(__file__).with_name("fixture-h264.mp4").open("rb") as source:
        data = source.read(FIXTURE_LIMIT + 1)
    if len(data) > FIXTURE_LIMIT or hashlib.sha256(data).hexdigest() != FIXTURE_SHA256:
        raise RuntimeError("the fixed H.264 fixture does not match its recorded digest")
    return data


def is_h264_frame(data):
    return any(
        marker.end() < len(data) and 1 <= data[marker.end()] & 0x1F <= 5
        for marker in ANNEX_B_START.finditer(data)
    )


def has_picture(Gst, buffer):
    if not 0 < buffer.get_size() <= ACCESS_UNIT_LIMIT:
        raise RuntimeError("H.264 access unit exceeded the fixture size limit or was empty")
    mapped, view = buffer.map(Gst.MapFlags.READ)
    if not mapped:
        raise RuntimeError("could not read H.264 access-unit headers")
    try:
        return is_h264_frame(view.data)
    finally:
        buffer.unmap(view)


class Trace:
    """In-flight and matched timestamps are bounded by the fixed frame count."""

    def __init__(self, no_pts):
        self.no_pts = no_pts
        self.lock = threading.Lock()
        self.inputs = {}
        self.outputs = set()
        self.ambiguous = set()
        self.latencies = {}
        self.input_count = 0
        self.output_count = 0
        self.failure = None

    def error(self, message):
        with self.lock:
            if self.failure is None:
                self.failure = message

    def input(self, pts, now):
        with self.lock:
            self.input_count += 1
            if self.input_count > FRAMES:
                raise RuntimeError("the codec consumed more than 60 fixture pictures")
            if pts == self.no_pts:
                return
            if pts in self.inputs:
                self.ambiguous.add(pts)
                self.latencies.pop(pts, None)
            else:
                self.inputs[pts] = now

    def output(self, pts, now):
        with self.lock:
            self.output_count += 1
            if self.output_count > FRAMES:
                raise RuntimeError("the codec delivered more than 60 fixture pictures")
            if pts == self.no_pts:
                return
            if pts in self.outputs:
                self.ambiguous.add(pts)
                self.latencies.pop(pts, None)
                return
            self.outputs.add(pts)
            submitted = self.inputs.get(pts)
            if submitted is not None and pts not in self.ambiguous and now >= submitted:
                self.latencies[pts] = (now - submitted) / 1_000_000

    def results(self):
        with self.lock:
            if self.failure is not None:
                raise RuntimeError(self.failure)
            if (self.input_count, self.output_count) != (FRAMES, FRAMES):
                raise RuntimeError(
                    f"expected {FRAMES} input and output pictures, received "
                    f"{self.input_count} input and {self.output_count} output"
                )
            return list(self.latencies.values())


def hardware_factory(Gst, mode):
    name = ENCODE_FACTORY if mode == "encode" else DECODE_FACTORY
    factory = Gst.ElementFactory.find(name)
    if factory is None or factory.get_plugin_name() != NVIDIA_PLUGIN:
        raise RuntimeError(f"NVIDIA GStreamer hardware factory {name} is unavailable")
    return name


def build_pipeline(Gst, mode):
    name = hardware_factory(Gst, mode)
    if mode == "encode":
        description = (
            f"videotestsrc num-buffers={FRAMES} pattern=smpte ! "
            f"video/x-raw,format=I420,width={WIDTH},height={HEIGHT},framerate=30/1 ! "
            f"nvvidconv ! video/x-raw(memory:NVMM),format=NV12 ! {name} name=hardware "
            f"bitrate=2000000 ! h264parse ! "
            f"video/x-h264,stream-format=byte-stream,alignment=au ! {SINK}"
        )
        fixture = None
    else:
        description = (
            f"appsrc name=fixture format=bytes stream-type=stream max-bytes={FIXTURE_LIMIT} ! "
            f"qtdemux ! h264parse ! video/x-h264,stream-format=byte-stream,alignment=au ! "
            f"{name} name=hardware ! {SINK}"
        )
        fixture = fixture_bytes()
    pipeline = Gst.parse_launch(description)
    return pipeline, name, fixture


def measure(Gst, pipeline, mode, fixture, timeout_ms):
    trace = Trace(Gst.CLOCK_TIME_NONE)
    hardware = pipeline.get_by_name("hardware")
    sink = pipeline.get_by_name("result")

    def observe_input(_pad, info):
        try:
            buffer = info.get_buffer()
            if buffer is not None and (mode == "encode" or has_picture(Gst, buffer)):
                trace.input(buffer.pts, time.monotonic_ns())
            return Gst.PadProbeReturn.OK
        except Exception as error:
            trace.error(str(error))
            return Gst.PadProbeReturn.DROP

    def receive_sample(sink_element):
        try:
            sample = sink_element.emit("pull-sample")
            if sample is None:
                raise RuntimeError("the fixture sink did not receive a sample")
            buffer = sample.get_buffer()
            arrived = time.monotonic_ns()
            if buffer.has_flags(Gst.BufferFlags.CORRUPTED) or buffer.has_flags(Gst.BufferFlags.GAP):
                raise RuntimeError("the fixture sink received a corrupted or missing picture")
            if mode == "encode" and not has_picture(Gst, buffer):
                return Gst.FlowReturn.OK
            caps = sample.get_caps().get_structure(0)
            expected_caps = "video/x-h264" if mode == "encode" else "video/x-raw"
            if (
                caps.get_name() != expected_caps
                or caps.get_value("width") != WIDTH
                or caps.get_value("height") != HEIGHT
                or buffer.n_memory() < 1
            ):
                raise RuntimeError("the codec delivered unexpected fixture settings or an empty buffer")
            trace.output(buffer.pts, arrived)
            return Gst.FlowReturn.OK
        except Exception as error:
            trace.error(str(error))
            return Gst.FlowReturn.ERROR

    hardware.get_static_pad("sink").add_probe(Gst.PadProbeType.BUFFER, observe_input)
    sink.connect("new-sample", receive_sample)
    start = time.monotonic_ns()
    limit = start + timeout_ms * 1_000_000
    try:
        if pipeline.set_state(Gst.State.PLAYING) == Gst.StateChangeReturn.FAILURE:
            raise RuntimeError("could not start the hardware pipeline")
        if fixture is not None:
            source = pipeline.get_by_name("fixture")
            buffer = Gst.Buffer.new_allocate(None, len(fixture), None)
            if buffer.fill(0, fixture) != len(fixture):
                raise RuntimeError("could not prepare the fixed input buffer")
            if source.emit("push-buffer", buffer) != Gst.FlowReturn.OK:
                raise RuntimeError("the fixed input clip was refused")
            if source.emit("end-of-stream") != Gst.FlowReturn.OK:
                raise RuntimeError("could not terminate the fixed input clip")
        message = pipeline.get_bus().timed_pop_filtered(
            max(0, limit - time.monotonic_ns()), Gst.MessageType.ERROR | Gst.MessageType.EOS
        )
        if message is None:
            raise RuntimeError("codec pipeline reached the time limit")
        if message.type == Gst.MessageType.ERROR:
            error, _debug = message.parse_error()
            raise RuntimeError(trace.failure or error.message)
        finished = time.monotonic_ns()
        if finished > limit:
            raise RuntimeError("codec pipeline reached the time limit")
        elapsed = (finished - start) / 1_000_000_000
        return elapsed, trace.results()
    finally:
        pipeline.set_state(Gst.State.NULL)


def load_gstreamer():
    try:
        import gi

        gi.require_version("Gst", "1.0")
        from gi.repository import Gst
    except (ImportError, ValueError) as error:
        raise RuntimeError(f"GStreamer Python bindings are unavailable: {error}") from error
    Gst.init(None)
    return Gst


def run_worker(options):
    try:
        Gst = load_gstreamer()
        pipeline, name, fixture = build_pipeline(Gst, options.mode)
        elapsed, latency = measure(Gst, pipeline, options.mode, fixture, options.timeout_ms)
        print(f"Jetson H.264 {options.mode} ({name})")
        print(f"Fixture: {WIDTH}x{HEIGHT}, {FRAMES} pictures at 30 fps")
        print(f"Pictures delivered: {FRAMES}")
        print(f"Pipeline elapsed: {elapsed:.3f} s")
        print(f"Pipeline throughput: {FRAMES / elapsed:.2f} frames/s")
        print(f"Unambiguous input/output PTS matches: {len(latency)}/{FRAMES}")
        if latency:
            print(f"Host codec-input to output mean: {statistics.mean(latency):.3f} ms")
            print(f"Host codec-input to output min / max: {min(latency):.3f} / {max(latency):.3f} ms")
        else:
            print("Host codec-input to output latency: unavailable from fixture timestamps")
        return 0
    except Exception as error:
        print(f"Error: {error}", file=sys.stderr)
        return 1


def main(argv=None):
    options = arguments(sys.argv[1:] if argv is None else argv)
    if options.worker:
        return run_worker(options)
    command = [
        sys.executable, str(Path(__file__).resolve()), options.mode, "--backend", options.backend,
        "--timeout-ms", str(options.timeout_ms), "--worker",
    ]
    try:
        process = subprocess.Popen(command, stdin=subprocess.DEVNULL, start_new_session=True)
    except OSError as error:
        print(f"Error: could not start codec worker: {error}", file=sys.stderr)
        return 1
    try:
        return process.wait(timeout=options.timeout_ms / 1000)
    except (subprocess.TimeoutExpired, KeyboardInterrupt):
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=1)
        except subprocess.TimeoutExpired:
            print("Error: codec worker remains blocked in a driver call", file=sys.stderr)
        else:
            print("Error: codec run timed out or was interrupted", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
