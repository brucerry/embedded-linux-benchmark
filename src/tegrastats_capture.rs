use std::io::{self, Read};

use crate::pcie_health::json_string;

pub(crate) const LIMIT: usize = 4096;

fn unsigned(text: &str, max: u32) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u32>().ok().filter(|n| *n <= max)
}

fn temperature(text: &str) -> Option<i32> {
    let negative = text.starts_with('-');
    let text = text.strip_prefix('-').unwrap_or(text);
    let (whole, fractional) = text.split_once('.').unwrap_or((text, ""));
    let frac = if fractional.is_empty() && !text.contains('.') {
        0
    } else if (1..=3).contains(&fractional.len()) {
        unsigned(fractional, 999)? as i32 * 10_i32.pow(3 - fractional.len() as u32)
    } else {
        return None;
    };
    let value = unsigned(whole, 999)? as i32 * 1000 + frac;
    Some(if negative { -value } else { value })
}

fn gpu(text: &str) -> Option<(Option<u32>, Vec<u32>)> {
    let (util, freq) = text.split_once('@').unwrap_or((text, ""));
    let util = match util {
        "" if text.starts_with('@') => None,
        _ => Some(unsigned(util.strip_suffix('%')?, 100)?),
    };
    let freq = if freq.is_empty() {
        if text.contains('@') {
            return None;
        }
        Vec::new()
    } else if let Some(group) = freq.strip_prefix('[').and_then(|f| f.strip_suffix(']')) {
        let values = group
            .split(',')
            .take(9)
            .map(|v| unsigned(v, u32::MAX))
            .collect::<Option<Vec<_>>>()?;
        if values.len() > 8 {
            return None;
        }
        values
    } else {
        vec![unsigned(freq, u32::MAX)?]
    };
    Some((util, freq))
}

fn rails(text: &str) -> Option<(u32, u32)> {
    let (instant, average) = text.split_once('/')?;
    Some((
        unsigned(instant.strip_suffix("mW")?, u32::MAX)?,
        unsigned(average.strip_suffix("mW")?, u32::MAX)?,
    ))
}

fn field<T>(value: Option<T>, extra: impl FnOnce(T) -> String) -> String {
    value
        .map(|v| format!(r#"{{"status":"observed","value":{}}}"#, extra(v)))
        .unwrap_or_else(|| r#"{"status":"unavailable"}"#.into())
}

fn decode(text: &str, rail: Option<&str>) -> Result<String, &'static str> {
    let text = text.strip_suffix('\n').unwrap_or(text);
    if text.len() > LIMIT
        || text.trim().is_empty()
        || !text.bytes().all(|b| b == b' ' || b.is_ascii_graphic())
    {
        return Err("expected one bounded ASCII tegrastats observation line");
    }
    if let Some(name) = rail {
        if !(5..=48).contains(&name.len())
            || !(name.starts_with("VDD_") || name.starts_with("VIN_"))
            || !name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err("expected one explicit VDD_ or VIN_ rail label");
        }
    }
    let mut engine = None;
    let mut temp = None;
    let mut power = None;
    let mut tokens = text.split_ascii_whitespace();
    while let Some(token) = tokens.next() {
        if token == "GR3D_FREQ" {
            if engine.is_some() {
                return Err("duplicate GR3D_FREQ");
            }
            engine = Some(tokens.next().and_then(gpu).ok_or("invalid GR3D_FREQ")?);
        } else if let Some(value) = token
            .strip_prefix("gpu@")
            .or_else(|| token.strip_prefix("gpu-thermal@"))
        {
            if temp.is_some() {
                return Err("duplicate GPU thermal observation");
            }
            temp = Some(
                value
                    .strip_suffix('C')
                    .and_then(temperature)
                    .ok_or("invalid GPU temperature")?,
            );
        } else if rail == Some(token) {
            if power.is_some() {
                return Err("duplicate selected power rail");
            }
            power = Some(
                tokens
                    .next()
                    .and_then(rails)
                    .ok_or("invalid selected power rail")?,
            );
        }
    }
    if engine.is_none() && temp.is_none() && power.is_none() {
        return Err("no recognized GPU telemetry on the supplied line");
    }
    let (util, frequencies) = engine.unwrap_or_default();
    let frequency = (!frequencies.is_empty()).then_some(frequencies);
    Ok(format!(
        concat!(
            r#"{{"schema_version":1,"kind":"jetson_tegrastats_capture","live_query":"not_performed","capture_identity":"unverified","#,
            r#""gpu_utilization_percent":{},"gpu_frequency_mhz":{},"gpu_temperature_millicelsius":{},"selected_rail":{}}}"#
        ),
        field(util, |v| v.to_string()),
        field(frequency, |v| format!("{v:?}")),
        field(temp, |v| v.to_string()),
        rail.map(|name| {
            format!(
                r#"{{"label":{},"observation":{}}}"#,
                json_string(name),
                field(power, |(a, b)| format!(
                    r#"{{"instant_mw":{a},"average_mw":{b}}}"#
                ))
            )
        })
        .unwrap_or_else(|| "null".into())
    ))
}

pub(crate) fn run(rail: Option<&str>) -> Result<(), String> {
    let mut raw = String::new();
    io::stdin()
        .lock()
        .take((LIMIT + 2) as u64)
        .read_to_string(&mut raw)
        .map_err(|e| format!("cannot read saved tegrastats line: {e}"))?;
    println!("{}", decode(&raw, rail)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_documented_shapes_and_zero() {
        let text = "RAM 20/100MB GR3D_FREQ 0%@[306,0,1020] gpu@-1.025C VDD_GPU_SOC 7mW/8mW";
        let out = decode(text, Some("VDD_GPU_SOC")).unwrap();
        assert!(out.contains(r#""gpu_utilization_percent":{"status":"observed","value":0}"#));
        assert!(out.contains(r#""gpu_frequency_mhz":{"status":"observed","value":[306, 0, 1020]}"#));
        assert!(
            out.contains(r#""gpu_temperature_millicelsius":{"status":"observed","value":-1025}"#)
        );
        assert!(out.contains(r#""instant_mw":7,"average_mw":8"#));
        assert!(decode("GR3D_FREQ @[550,550]", None)
            .unwrap()
            .contains(r#""gpu_utilization_percent":{"status":"unavailable"}"#));
        assert!(decode("GR3D_FREQ 25%@700", None)
            .unwrap()
            .contains(r#""value":[700]"#));
    }

    #[test]
    fn refuses_ambiguous_and_malformed_input() {
        for text in [
            "gpu@0C\nGR3D_FREQ 12%",
            "GR3D_FREQ 101%",
            "GR3D_FREQ 12%@[]",
            "GR3D_FREQ 12%@",
            "GR3D_FREQ 1% GR3D_FREQ 2%",
            "gpu@12.C",
            "gpu@31C gpu-thermal@32C",
            "gpu@31C VDD_GPU 4mW/5",
            "gpu@31C VDD_GPU 1mW/2mW VDD_GPU 3mW/4mW",
        ] {
            assert!(decode(text, Some("VDD_GPU")).is_err(), "{text}");
        }
        assert!(decode(&"x".repeat(LIMIT + 1), None).is_err());
        assert!(decode("gpu@41C", Some("VDD;GPU")).is_err());
    }
}
