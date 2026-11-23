# Optional Jetson hardware H.264 benchmarks

Run the helper on a Jetson Linux system with Python GObject bindings, GStreamer, and NVIDIA's accelerated GStreamer plug-ins installed for that SDK. The device must provide the selected hardware codec. The helper requires the NVIDIA `nvvideo4linux2` factory for the requested operation, and encode also uses `nvvidconv` for NVMM input. The helper and core Rust crate have no CUDA Toolkit dependency. [NVIDIA accelerated GStreamer guide](https://docs.nvidia.com/jetson/archives/r39.2.2/DeveloperGuide/SD/Multimedia/AcceleratedGstreamer.html)

```sh
python3 codec/benchmark.py encode --backend jetson-v4l2
python3 codec/benchmark.py decode --backend jetson-v4l2
```

**Encode** uses `nvv4l2h264enc` at 2 Mbit/s with 60 generated SMPTE pictures at 640×480 and 30 fps. It verifies that the parsed output delivers 60 H.264 picture access units and discards them in memory.

**Decode** feeds the included 60-picture H.264 clip into `nvv4l2decoder`. The helper checks the bounded clip's SHA-256 before feeding it, then checks the delivered frame count, dimensions and corruption flags. It discards the decoded buffers in memory. The clip is a generated test pattern with one H.264 video stream and presentation timestamps. No recording files are created.

Pipeline frame rate includes startup, conversion or parsing, and delivery until end of stream. The separate host latency starts at the selected codec's input pad and ends when the output picture arrives at the sink. It includes plug-in queueing and output delivery. It is reported only for unambiguous input/output presentation timestamp matches. If timestamps cannot be matched, the latency is unavailable. These are host-side measurements. [GStreamer app sink](https://gstreamer.freedesktop.org/documentation/app/appsink.html) · [H.264 parser](https://gstreamer.freedesktop.org/documentation/videoparsersbad/h264parse.html)

The worker deadline defaults to 10000 ms. Use `--timeout-ms` for a value from 250 to 30000. Driver calls can remain blocked in the kernel after the worker is signaled. The selected hardware factory must be available. There is no software fallback.

## Verification

- Native GStreamer 1.26.2 tests delivered all 60 pictures from the pinned fixture through a parser and through an explicitly test-only decoder. They verified output caps, counts, PTS matching and CLI rejection. The production CLI reported the missing NVIDIA backend on the native test host.
- The NVIDIA `nvidia-l4t-gstreamer` 39.2.2-20261112230500 AArch64 package matched the published SHA-256 index. Both `nvvideo4linux2` and `nvvidconv` loaded for descriptor checks under QEMU user mode using the Jetson Linux 39.2.2 sample loader, release-matched NVIDIA libraries, and Ubuntu AArch64 GStreamer 1.24.2. Both reported the GStreamer 1.24 plug-in ABI. Physical hardware encoding and decoding remain unverified. [NVIDIA Jetson APT repository](https://repo.download.nvidia.com/jetson/) · [GStreamer 1.24.2 plug-in ABI](https://github.com/GStreamer/gstreamer/blob/1.24.2/subprojects/gstreamer/gst/gstplugin.h)

```sh
python3 -B -m unittest discover -s codec
```

The test suite can run without Jetson hardware. The GStreamer pipeline tests are skipped when their optional parser or decoder plug-ins are unavailable.
