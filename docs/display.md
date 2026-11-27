# Camera and HDMI diagnostics

## Camera delivery rate

Use an explicit V4L2 video node from a dedicated, already configured capture source. The command reports the driver's current dimensions, pixel format, plane count, field mode, and frame interval when available. It uses the streaming MMAP queue, with single-plane and multi-plane capture support, and discards buffers without saving or recording frames. It leaves the device's format and controls as configured.

```sh
./target/release/eld-engine camera --device /dev/video0 --frames 120
```

`--frames` accepts 2–10000 (default 120). `--timeout-ms` accepts 1–30000 (default 10000). The command asks the driver for four buffers and limits each mapped buffer to 24 MiB and the total mappings to 96 MiB. The deadline is checked between operations. A stalled driver call can extend the run. Capture currently uses the verified 64-bit V4L2 buffer ABI on Linux x86-64 and AArch64 with glibc or musl. Other ABIs return a clear error before opening a node. [Linux V4L2 streaming API](https://docs.kernel.org/userspace-api/media/v4l/mmap.html)

The delivery rate and interval standard deviation use monotonic userspace dequeue times. Alternating-field mode reports fields per second. Scheduler delays and buffering affect the measurement. Driver sequence gaps and buffers flagged erroneous are reported separately. Gaps can miss frame loss before the driver assigns a sequence, and driver resets or repeated sequences can make gap counts incomplete. [Linux V4L2 buffer semantics](https://docs.kernel.org/userspace-api/media/v4l/buffer.html)

## HDMI connector observations and capture delivery

```sh
./target/release/eld-engine hdmi status --connector card0-HDMI-A-1
./target/release/eld-engine hdmi capture --device /dev/video2 --fixture --frames 120
```

`hdmi status` reads the explicit DRM sysfs connector's connection status, encoder state, and advertised mode names. Those attributes do not expose active timing. The command does not set display modes. [Linux DRM connector attributes](https://github.com/torvalds/linux/blob/v6.12/drivers/gpu/drm/drm_sysfs.c)

`hdmi capture` uses the camera streaming benchmark on an explicitly identified, dedicated HDMI capture device. The result covers delivery rate and jitter at that device. Output-to-capture latency requires a synchronized stimulus and reference. Verification used simulated V4L2 transactions and inert connector attributes. Physical video and HDMI results remain unverified.

V4L2 ABI checks passed with native Linux and the OpenWrt AArch64 and Jetson SDK headers. The Jetson Linux 39.2.2 unit tests exercised mock multi-plane queueing, dequeue, and cleanup under QEMU user mode. OpenWrt 25.12.6 AArch64 and 24.10.8 MIPS32 packages built successfully, and CLI smoke tests in matching userspaces checked device and fixture rejection, including the explicit 32-bit capture error.
