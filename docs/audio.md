# Optional ALSA PCM loopback

Build the audio helper separately with the Linux ALSA development headers and library.

```sh
mkdir -p target
cc -std=c11 -O2 -Wall -Wextra -Werror audio/benchmark.c \
  $(pkg-config --cflags --libs alsa) -lm -o target/eld-engine-audio
./target/eld-engine-audio --self-test
./target/eld-engine-audio --playback hw:0,0 --capture hw:1,0 --loopback
```

Use a dedicated, level-compatible playback-to-capture loopback fixture. Choose the exact numeric hardware PCM names for playback and capture. The helper configures only its two streams, uses 48 kHz signed 16-bit PCM, and allows one or two channels with `--channels` (default two). The small test marker is copied to both playback channels. Analysis checks the first capture channel in memory.

- Capture accepts 100–2000 ms with `--duration-ms` and defaults to 1000 ms. The foreground deadline accepts 1–30000 ms with `--timeout-ms` and defaults to 5000 ms. A blocked driver call can extend elapsed time.
- A run verifies the marker through normalized signal correlation, including an inverted signal. It reports captured frames, negotiated ALSA period and buffer sizes, and detected playback underruns and capture overruns. A detected xrun aborts the run.
- The software round-trip estimate runs from submission of the marker to its estimated captured sample time. It uses the capture stream's monotonic status timestamp and queued-frame count. Device buffering, timestamp accuracy and scheduling affect the estimate. The result is unavailable when capture timestamps are unusable. [ALSA PCM status API](https://www.alsa-project.org/alsa-doc/alsa-lib/group___p_c_m___status.html)

Validation used Debian ALSA 1.2.14 development headers and runtime, synthetic correlation and CLI rejection tests, and an AArch64 build with the Jetson Linux 39.2.2 SDK compiler and Ubuntu ALSA 1.2.11 in a matching sample-libc userspace. The AArch64 synthetic self-test passed under QEMU user mode. No capture files were written. PCM device behavior and actual round-trip latency remain unverified.
