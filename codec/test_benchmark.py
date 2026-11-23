import subprocess
import sys
import unittest

import benchmark


class FixtureTests(unittest.TestCase):
    def test_pinned_clip_and_slice_marker(self):
        self.assertLess(len(benchmark.fixture_bytes()), benchmark.FIXTURE_LIMIT)
        self.assertTrue(benchmark.is_h264_frame(b"\0\0\1\x67\0\0\0\1\x65"))
        self.assertFalse(benchmark.is_h264_frame(b"\0\0\1\x67\0\0\1\x68"))
        self.assertFalse(benchmark.is_h264_frame(b"\0\0\1"))

    def test_only_unambiguous_pts_and_exact_completed_counts(self):
        trace = benchmark.Trace(no_pts=-1)
        for index in range(benchmark.FRAMES):
            pts = index if index >= 2 else -1
            trace.input(pts, index * 1_000_000)
            trace.output(pts, (index + 2) * 1_000_000)
        self.assertEqual(trace.results(), [2.0] * (benchmark.FRAMES - 2))
        with self.assertRaisesRegex(RuntimeError, "more than 60"):
            trace.output(100, 200_000_000)
        trace = benchmark.Trace(no_pts=-1)
        trace.input(42, 0)
        trace.output(42, 1000)
        trace.input(42, 2000)
        self.assertFalse(trace.latencies)
        with self.assertRaisesRegex(RuntimeError, "expected 60"):
            trace.results()

    def test_hardware_name_and_plugin_are_required(self):
        class Factory:
            @staticmethod
            def get_plugin_name():
                return "software"

        class ElementFactory:
            @staticmethod
            def find(_name):
                return Factory()

        class Stub:
            pass

        Stub.ElementFactory = ElementFactory
        with self.assertRaisesRegex(RuntimeError, "hardware factory nvv4l2h264enc is unavailable"):
            benchmark.hardware_factory(Stub, "encode")
        with self.assertRaisesRegex(RuntimeError, "hardware factory nvv4l2decoder is unavailable"):
            benchmark.hardware_factory(Stub, "decode")

    def test_cli_rejects_unknown_backend_and_time_before_importing_gstreamer(self):
        for args in [
            ["encode"],
            ["decode", "--backend", "software"],
            ["decode", "--backend", "jetson-v4l2", "--timeout-ms", "30001"],
        ]:
            result = subprocess.run(
                [sys.executable, benchmark.__file__, *args], capture_output=True, timeout=5
            )
            self.assertEqual(result.returncode, 2)
            self.assertFalse(result.stdout)
            self.assertIn(b"error:", result.stderr)

    def test_gstreamer_encoded_fixture_delivery_without_hardware(self):
        try:
            Gst = benchmark.load_gstreamer()
        except RuntimeError as error:
            self.skipTest(str(error))
        if not all(Gst.ElementFactory.find(name) for name in ("qtdemux", "h264parse", "appsink")):
            self.skipTest("GStreamer fixture parser plugins are unavailable")
        pipeline = Gst.parse_launch(
            "appsrc name=fixture format=bytes stream-type=stream ! "
            "qtdemux ! h264parse ! video/x-h264,stream-format=byte-stream,alignment=au ! "
            "identity name=hardware ! " + benchmark.SINK
        )
        elapsed, latency = benchmark.measure(Gst, pipeline, "encode", benchmark.fixture_bytes(), 3000)
        self.assertGreater(elapsed, 0.0)
        self.assertEqual(len(latency), benchmark.FRAMES)

    def test_decoded_frame_accounting_with_explicit_test_decoder(self):
        try:
            Gst = benchmark.load_gstreamer()
        except RuntimeError as error:
            self.skipTest(str(error))
        names = ("qtdemux", "h264parse", "avdec_h264", "appsink")
        if not all(Gst.ElementFactory.find(name) for name in names):
            self.skipTest("test-only software H.264 decoder is unavailable")
        pipeline = Gst.parse_launch(
            "appsrc name=fixture format=bytes stream-type=stream ! "
            "qtdemux ! h264parse ! video/x-h264,stream-format=byte-stream,alignment=au ! "
            "avdec_h264 name=hardware ! " + benchmark.SINK
        )
        elapsed, latency = benchmark.measure(Gst, pipeline, "decode", benchmark.fixture_bytes(), 3000)
        self.assertGreater(elapsed, 0.0)
        self.assertEqual(len(latency), benchmark.FRAMES)


if __name__ == "__main__":
    unittest.main()
