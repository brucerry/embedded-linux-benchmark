# Optional CUDA benchmarks

The CUDA helper builds separately with NVIDIA's CUDA Toolkit. Use the Toolkit and host compiler supported by your target Linux release or JetPack SDK. Select the GPU architecture for that target in the build. The example uses the Jetson Orin architecture `sm_87`. [NVIDIA Toolkit installation guide](https://docs.nvidia.com/cuda/cuda-installation-guide-linux/) · [NVIDIA compute capability table](https://developer.nvidia.com/cuda-gpus)

```sh
mkdir -p target
nvcc -std=c++17 -O2 -arch=sm_87 --cudart=static cuda/benchmark.cu \
  -o target/embedded-linux-benchmark-cuda
./target/embedded-linux-benchmark-cuda --help
./target/embedded-linux-benchmark-cuda compute --device 0 --rounds 4
./target/embedded-linux-benchmark-cuda transfer --device 0 --size-mib 8 --rounds 4
```

Choose the GPU explicitly with `--device`. The runtime reports an error if the driver or selected device is unavailable. The core Rust build and automatic stress suite have no CUDA dependency.

- **Compute** runs a bounded FP32 fused multiply-add kernel. CUDA events measure kernel execution. Throughput counts two floating-point operations per FMA. The last result buffer is compared against a CPU reference. `--elements` accepts 1024–1048576 (default 262144) and `--iterations` accepts 1–256 (default 64).
- **Transfer** measures synchronous CUDA host-to-device and device-to-host copies separately, using pageable host buffers. Each round verifies the returned payload. `--size-mib` accepts 1–64 per direction (default 8). On integrated GPUs, the rates reflect the CUDA copy path through shared system memory.

Both modes accept 1–16 rounds (default 4) and a foreground deadline of 1–30000 ms (default 10000). The deadline is checked between CUDA calls and during result verification. An in-progress runtime call can extend the run. [NVIDIA CUDA Runtime memory API](https://docs.nvidia.com/cuda/archive/12.9.1/cuda-runtime-api/group__CUDART__MEMORY.html)

## Build evidence

- Linux x86-64 compiled with the official CUDA 13.5.37 compiler and runtime. Help, invalid inputs, and the unavailable-runtime error passed without a GPU.
- The AArch64 executable linked with the official CUDA 12.9.1 compiler and AArch64 runtime using the NVIDIA Jetson Linux 39.2.2 SDK C++ linker. ELF inspection confirmed the AArch64 GNU/Linux interpreter. Help and argument rejection passed under QEMU user mode with the SDK sysroot.

Physical CUDA compute and transfer performance remain unverified. [CUDA 13.5.0 redistributable manifest](https://developer.download.nvidia.com/compute/cuda/redist/redistrib_13.5.0.json) · [CUDA 12.9.1 redistributable manifest](https://developer.download.nvidia.com/compute/cuda/redist/redistrib_12.9.1.json) · [Jetson SDK build record](jetson-linux.md)
