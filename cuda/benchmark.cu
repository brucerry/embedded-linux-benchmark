#include <cuda_runtime.h>

#include <charconv>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <iostream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

using Clock = std::chrono::steady_clock;
constexpr std::size_t mib = 1024 * 1024;

struct Settings {
    bool compute = false;
    int device = -1;
    unsigned elements = 262144;
    unsigned iterations = 64;
    unsigned rounds = 4;
    unsigned size_mib = 8;
    unsigned timeout_ms = 10000;
};

void usage() {
    std::puts("Usage:\n"
              "  eld-engine-cuda compute --device N "
              "[--elements N] [--iterations N] [--rounds N] [--timeout-ms MS]\n"
              "  eld-engine-cuda transfer --device N "
              "[--size-mib N] [--rounds N] [--timeout-ms MS]");
}

unsigned number(std::string_view name, std::string_view input, unsigned min, unsigned max) {
    unsigned value = 0;
    const auto end = input.data() + input.size();
    const auto result = std::from_chars(input.data(), end, value);
    if (result.ec != std::errc{} || result.ptr != end || value < min || value > max) {
        throw std::runtime_error(std::string(name) + " requires an integer in " +
                                 std::to_string(min) + "–" + std::to_string(max));
    }
    return value;
}

Settings options(int argc, char* argv[]) {
    Settings settings;
    std::string_view mode = argv[1];
    if (mode != "compute" && mode != "transfer") {
        throw std::runtime_error("expected 'compute' or 'transfer'");
    }
    settings.compute = mode == "compute";
    const std::string_view labels[] = {"--device", "--rounds", "--timeout-ms",
                                       "--elements", "--iterations", "--size-mib"};
    bool used[6] = {};
    for (int i = 2; i < argc; ++i) {
        const std::string_view flag = argv[i];
        unsigned index = 0;
        while (index < 6 && labels[index] != flag) {
            ++index;
        }
        if (index == 6 || (settings.compute ? index == 5 : index == 3 || index == 4)) {
            throw std::runtime_error("unsupported option for this mode: " + std::string(flag));
        }
        if (used[index] || ++i == argc) {
            throw std::runtime_error("duplicate option or missing value: " + std::string(flag));
        }
        used[index] = true;
        const std::string_view value = argv[i];
        switch (index) {
            case 0: settings.device = static_cast<int>(number(flag, value, 0, 1023)); break;
            case 1: settings.rounds = number(flag, value, 1, 16); break;
            case 2: settings.timeout_ms = number(flag, value, 1, 30000); break;
            case 3: settings.elements = number(flag, value, 1024, 1048576); break;
            case 4: settings.iterations = number(flag, value, 1, 256); break;
            case 5: settings.size_mib = number(flag, value, 1, 64); break;
        }
    }
    if (settings.device < 0) {
        throw std::runtime_error("an explicit '--device' is required");
    }
    return settings;
}

void check(cudaError_t status, const char* operation) {
    if (status != cudaSuccess) {
        throw std::runtime_error(std::string(operation) + ": " + cudaGetErrorString(status));
    }
}

void remaining(Clock::time_point end) {
    if (Clock::now() >= end) {
        throw std::runtime_error("time limit reached");
    }
}

struct DeviceBuffer {
    void* ptr = nullptr;
    explicit DeviceBuffer(std::size_t bytes) {
        check(cudaMalloc(&ptr, bytes), "allocate CUDA device memory");
    }
    ~DeviceBuffer() { cudaFree(ptr); }
    DeviceBuffer(const DeviceBuffer&) = delete;
    DeviceBuffer& operator=(const DeviceBuffer&) = delete;
};

struct Event {
    cudaEvent_t handle = nullptr;
    Event() { check(cudaEventCreate(&handle), "create CUDA event"); }
    ~Event() { cudaEventDestroy(handle); }
    Event(const Event&) = delete;
    Event& operator=(const Event&) = delete;
};

constexpr float multiplier = 1.0009765625f;
constexpr float increment = 0.00048828125f;
constexpr float seed_scale = 0.0009765625f;

__global__ void fma_kernel(float* output, unsigned elements, unsigned iterations) {
    const unsigned index = blockIdx.x * blockDim.x + threadIdx.x;
    if (index < elements) {
        float value = (index & 1023) * seed_scale;
        for (unsigned step = 0; step < iterations; ++step) {
            value = __fmaf_rn(value, multiplier, increment);
        }
        output[index] = value;
    }
}

void compute(const Settings& settings, Clock::time_point end) {
    const auto bytes = settings.elements * sizeof(float);
    DeviceBuffer device(bytes);
    Event begin;
    Event complete;
    double measured_ms = 0;
    for (unsigned round = 0; round < settings.rounds; ++round) {
        remaining(end);
        check(cudaEventRecord(begin.handle), "record CUDA start event");
        fma_kernel<<<(settings.elements + 255) / 256, 256>>>(
            static_cast<float*>(device.ptr), settings.elements, settings.iterations);
        check(cudaGetLastError(), "launch CUDA FMA kernel");
        check(cudaEventRecord(complete.handle), "record CUDA end event");
        check(cudaEventSynchronize(complete.handle), "wait for CUDA kernel");
        float elapsed_ms = 0;
        check(cudaEventElapsedTime(&elapsed_ms, begin.handle, complete.handle), "measure CUDA events");
        measured_ms += elapsed_ms;
    }
    remaining(end);
    std::vector<float> output(settings.elements);
    check(cudaMemcpy(output.data(), device.ptr, bytes, cudaMemcpyDeviceToHost), "read CUDA results");
    float expected[1024];
    for (unsigned seed = 0; seed < 1024; ++seed) {
        float value = seed * seed_scale;
        for (unsigned step = 0; step < settings.iterations; ++step) {
            value = std::fma(value, multiplier, increment);
        }
        expected[seed] = value;
    }
    for (unsigned i = 0; i < settings.elements; ++i) {
        if ((i & 4095) == 0) {
            remaining(end);
        }
        if (output[i] != expected[i & 1023]) {
            throw std::runtime_error("CUDA FMA result mismatch at element " + std::to_string(i));
        }
    }
    remaining(end);
    if (!(measured_ms > 0)) {
        throw std::runtime_error("CUDA kernel time was below event resolution");
    }
    const auto operations = std::uint64_t{2} * settings.elements * settings.iterations * settings.rounds;
    std::printf("CUDA compute (FP32 FMA kernel events)\n"
                "Final result elements verified: %u\nKernel time: %.6f s\n"
                "Counted FP32 operations: %llu\nThroughput: %.3f GFLOP/s\n",
                settings.elements, measured_ms / 1000,
                static_cast<unsigned long long>(operations), operations / measured_ms / 1e6);
}

void transfer(const Settings& settings, Clock::time_point end) {
    const auto bytes = settings.size_mib * mib;
    DeviceBuffer device(bytes);
    std::vector<std::uint8_t> source(bytes), target(bytes);
    double upload = 0, download = 0;
    for (unsigned round = 0; round < settings.rounds; ++round) {
        remaining(end);
        for (std::size_t index = 0; index < bytes; ++index) {
            source[index] = static_cast<std::uint8_t>(index * 79 + (index >> 8) + round);
        }
        remaining(end);
        auto start = Clock::now();
        check(cudaMemcpy(device.ptr, source.data(), bytes, cudaMemcpyHostToDevice), "CUDA host-to-device copy");
        check(cudaDeviceSynchronize(), "wait for CUDA upload");
        upload += std::chrono::duration<double>(Clock::now() - start).count();
        remaining(end);
        start = Clock::now();
        check(cudaMemcpy(target.data(), device.ptr, bytes, cudaMemcpyDeviceToHost), "CUDA device-to-host copy");
        download += std::chrono::duration<double>(Clock::now() - start).count();
        if (target != source) {
            throw std::runtime_error("CUDA transfer payload mismatch");
        }
    }
    remaining(end);
    if (!(upload > 0) || !(download > 0)) {
        throw std::runtime_error("CUDA copy time was below timer resolution");
    }
    const auto total_mib = std::uint64_t{settings.size_mib} * settings.rounds;
    std::printf("CUDA transfers (pageable host buffers, synchronous user time)\n"
                "Bytes verified per direction: %llu\nHost to device: %.2f MiB/s\n"
                "Device to host: %.2f MiB/s\n",
                static_cast<unsigned long long>(total_mib * mib), total_mib / upload, total_mib / download);
}

int main(int argc, char* argv[]) {
    if (argc == 1 || (argc == 2 && std::string_view(argv[1]) == "--help")) {
        usage();
        return 0;
    }
    try {
        const auto settings = options(argc, argv);
        const auto end = Clock::now() + std::chrono::milliseconds(settings.timeout_ms);
        int count = 0;
        check(cudaGetDeviceCount(&count), "CUDA runtime unavailable");
        if (settings.device >= count) {
            throw std::runtime_error("requested CUDA device is unavailable");
        }
        check(cudaSetDevice(settings.device), "select CUDA device");
        cudaDeviceProp device{};
        check(cudaGetDeviceProperties(&device, settings.device), "query CUDA device");
        remaining(end);
        if (settings.compute) {
            compute(settings, end);
        } else {
            transfer(settings, end);
        }
        std::printf("CUDA device %d: %s (compute capability %d.%d)\n",
                    settings.device, device.name, device.major, device.minor);
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "Error: " << error.what() << '\n';
        return 1;
    }
}
