#include <nvml.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>
static int lookups;
static int is(const char *key) {
    const char *value = getenv("TEST_STATE");
    return value && strcmp(value, key) == 0;
}
nvmlReturn_t nvmlInitWithFlags(unsigned int flags) {
    assert(flags == NVML_INIT_FLAG_NO_ATTACH);
    return is("init-failure") ? NVML_ERROR_UNKNOWN : NVML_SUCCESS;
}
nvmlReturn_t nvmlShutdown(void) { return is("shutdown-failure") ? NVML_ERROR_UNKNOWN : NVML_SUCCESS; }
nvmlReturn_t nvmlDeviceGetHandleByPciBusId_v2(const char *id, nvmlDevice_t *out) {
    assert(strcmp(id,"0000:01:00.0") == 0);
    memset(out, 0, sizeof *out);
    return NVML_SUCCESS;
}
nvmlReturn_t nvmlDeviceGetPciInfo_v3(nvmlDevice_t dev, nvmlPciInfo_t *out) {
    (void)dev;
    ++lookups;
    strcpy(out->busId, (is("wrong-bdf") || (is("changed-bdf") && lookups > 1))
           ? "00000000:02:00.0" : "00000000:01:00.0");
    return NVML_SUCCESS;
}
nvmlReturn_t nvmlDeviceGetUUID(nvmlDevice_t dev, char *out, unsigned int len) {
    (void)dev;
    assert(len >= 20);
    strcpy(out, is("changed-uuid") && lookups > 1 ? "private-new" : "private-original");
    return NVML_SUCCESS;
}
nvmlReturn_t nvmlDeviceGetTemperatureV(nvmlDevice_t dev, nvmlTemperature_t *out) {
    (void)dev;
    assert(out->version == nvmlTemperature_v1 && out->sensorType == NVML_TEMPERATURE_GPU);
    out->temperature = is("negative") ? -3 : 43;
    return NVML_SUCCESS;
}
nvmlReturn_t nvmlDeviceGetPowerUsage(nvmlDevice_t dev, unsigned int *out) {
    (void)dev;
    *out = 130000;
    return is("partial") ? NVML_ERROR_NO_PERMISSION : NVML_SUCCESS;
}
nvmlReturn_t nvmlDeviceGetTotalEccErrors(nvmlDevice_t dev, nvmlMemoryErrorType_t type,
                                       nvmlEccCounterType_t scope, unsigned long long *out) {
    (void)dev;
    assert(scope == NVML_VOLATILE_ECC);
    *out = type == NVML_MEMORY_ERROR_TYPE_CORRECTED ? 9007199254740993ULL : 0;
    return is("partial") && type == NVML_MEMORY_ERROR_TYPE_CORRECTED ? NVML_ERROR_NOT_SUPPORTED : NVML_SUCCESS;
}
