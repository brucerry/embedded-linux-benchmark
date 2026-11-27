# Board power-rail observations

## Selected INA3221 power-rail readings

`eld-engine power ina3221 --node 1-0040` reads the rail label, bus voltage
(mV), and current (mA) of a selected INA3221 kernel device, where present
on Jetson Orin and other supported boards. Use your board’s actual bus and
address; there is no probing across buses or hard-coded rail mapping.
Readings are sequential, not simultaneous. The tool does not estimate
power, set current limits, or contact a raw I2C device. Missing metrics
remain unavailable, not zero.
[Kernel driver ABI](https://docs.kernel.org/hwmon/ina3221.html)
