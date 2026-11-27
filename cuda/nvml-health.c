#include <nvml.h>
#include <stdio.h>
#include <string.h>

/* Optional collector. No controls, reset calls, workloads or device enumeration. */
static int hex(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

static int normalize(const char *in, char out[13]) {
    size_t n = strlen(in);
    if (n == 16 && memcmp(in, "0000", 4) == 0) { in += 4; n -= 4; }
    if (n != 12 || in[4] != ':' || in[7] != ':' || in[10] != '.') return 0;
    for (size_t i = 0; i < n; ++i) {
        if (i == 4 || i == 7 || i == 10) { out[i] = in[i]; continue; }
        int h = hex(in[i]);
        if (h < 0) return 0;
        out[i] = "0123456789abcdef"[h];
    }
    out[n] = '\0';
    return hex(out[8]) <= 1 && hex(out[11]) <= 7;
}

static const char *metric(nvmlReturn_t status, unsigned long long value, char text[32]) {
    if (status == NVML_SUCCESS) { snprintf(text, 32, "%llu", value); return text; }
    if (status == NVML_ERROR_NOT_SUPPORTED) return "unsupported";
    if (status == NVML_ERROR_NO_PERMISSION) return "inaccessible";
    return "unavailable";
}

static int observed(nvmlDevice_t gpu, char bdf[13], char uuid[NVML_DEVICE_UUID_BUFFER_SIZE]) {
    nvmlPciInfo_t pci = {0};
    if (nvmlDeviceGetPciInfo_v3(gpu, &pci) != NVML_SUCCESS ||
        !memchr(pci.busId, '\0', sizeof pci.busId) || !normalize(pci.busId, bdf)) return 0;
    memset(uuid, 0, NVML_DEVICE_UUID_BUFFER_SIZE);
    return nvmlDeviceGetUUID(gpu, uuid, NVML_DEVICE_UUID_BUFFER_SIZE) == NVML_SUCCESS &&
           uuid[0] && memchr(uuid, '\0', NVML_DEVICE_UUID_BUFFER_SIZE);
}

int main(int argc, char **argv) {
    char selected[13], before[13], after[13];
    char id_before[NVML_DEVICE_UUID_BUFFER_SIZE], id_after[NVML_DEVICE_UUID_BUFFER_SIZE];
    if (argc != 3 || strcmp(argv[1], "--device") != 0 || !normalize(argv[2], selected)) {
        fprintf(stderr, "usage: eld-engine-nvml --device DOMAIN:BUS:DEVICE.FUNCTION\n");
        return 2;
    }
    if (nvmlInitWithFlags(NVML_INIT_FLAG_NO_ATTACH) != NVML_SUCCESS) {
        fprintf(stderr, "NVML unavailable; no observation recorded\n");
        return 1;
    }
    nvmlDevice_t gpu;
    if (nvmlDeviceGetHandleByPciBusId_v2(selected, &gpu) != NVML_SUCCESS ||
        !observed(gpu, before, id_before) || strcmp(before, selected) != 0) {
        nvmlShutdown();
        fprintf(stderr, "selected NVIDIA GPU identity unavailable\n");
        return 1;
    }
    nvmlTemperature_t temp = { .version = nvmlTemperature_v1, .sensorType = NVML_TEMPERATURE_GPU };
    unsigned int power = 0;
    unsigned long long ce = 0, ue = 0;
    nvmlReturn_t t = nvmlDeviceGetTemperatureV(gpu, &temp);
    /* v1 captures carry unsigned Celsius only; do not present a negative as zero. */
    if (t == NVML_SUCCESS && temp.temperature < 0) t = NVML_ERROR_UNKNOWN;
    nvmlReturn_t p = nvmlDeviceGetPowerUsage(gpu, &power);
    nvmlReturn_t c = nvmlDeviceGetTotalEccErrors(gpu, NVML_MEMORY_ERROR_TYPE_CORRECTED,
                                              NVML_VOLATILE_ECC, &ce);
    nvmlReturn_t u = nvmlDeviceGetTotalEccErrors(gpu, NVML_MEMORY_ERROR_TYPE_UNCORRECTED,
                                              NVML_VOLATILE_ECC, &ue);
    int same = observed(gpu, after, id_after) && strcmp(before, after) == 0 &&
               strcmp(id_before, id_after) == 0;
    if (nvmlShutdown() != NVML_SUCCESS || !same) {
        fprintf(stderr, "NVIDIA GPU identity or shutdown unverified; no observation recorded\n");
        return 1;
    }
    char tb[32], pb[32], cb[32], ub[32];
    if (printf("eld_nvml_v1,%s,%s,%s,%s,%s\n", selected,
               metric(t, temp.temperature < 0 ? 0 : (unsigned)temp.temperature, tb),
               metric(p, power, pb), metric(c, ce, cb), metric(u, ue, ub)) < 0 ||
        fflush(stdout) != 0) {
        fprintf(stderr, "could not write NVIDIA observation\n");
        return 1;
    }
    return 0;
}
