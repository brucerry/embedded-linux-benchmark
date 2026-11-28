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
