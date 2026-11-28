# Offline BMC evidence

For a saved, locally authorized Redfish ComputerSystem JSON response:

```sh
python3 -B scripts/check_redfish_system_capture.py --capture system.json --system-id node-1
```

Replace the sample ID with the known system ID. The checker summarizes
`PowerState`, `Status.Health`, and `Status.HealthRollup` separately. Missing or
null properties are unavailable; unsupported values are not guessed.
`--expected-sha256` optionally checks the exact saved bytes against an
independent digest.

This host-side utility makes no requests and follows no links or actions.
The system ID is a selection check, not a physical identity or freshness
claim. It never exports OEM data, asset serials, or action endpoints in the
decoded output. A reported health value is not an independent machine
health verdict. No BMC or hardware is qualified by the synthetic fixture.

[DMTF ComputerSystem schema](https://redfish.dmtf.org/schemas/v1/ComputerSystem.v1_22_0.json) ·
[DMTF resource properties](https://redfish.dmtf.org/schemas/v1/Resource.json)

## Saved temperature sensor

For an already saved Redfish `Sensor` response:

```sh
python3 -B scripts/check_redfish_temperature_capture.py --capture inlet.json --sensor-id inlet
```

The checker reports an exact decimal Celsius reading only when
`ReadingType` is `Temperature` and `ReadingUnits` is `Cel`. It keeps an
observed zero distinct from missing data or an unsupported unit. It does
not fetch a sensor, follow links, export OEM data, or assess cooling health.
Provide `--expected-sha256` from an independent capture record when available.

[DMTF Redfish data model](https://redfish.dmtf.org/schemas/v1/DSP0268_2025.3.pdf)

## Saved rotational-speed sensor

```sh
python3 -B scripts/check_redfish_rotational_capture.py --capture fan.json --sensor-id fan1
```

For a saved Redfish `Sensor`, this returns RPM only for the `Rotational`
reading type and either documented RPM spelling, `{rev}/min` or `RPM`.
Percent and radians per second stay unsupported, not converted. Zero RPM
is not a cooling verdict. The sensor ID and optional digest select a saved
record, not a live fan or its physical identity. No control is performed.
