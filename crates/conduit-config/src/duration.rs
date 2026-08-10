//! Wall-clock durations for config fields such as `lmdb.sync_interval`.

use std::time::Duration;

/// Parse a duration with an explicit `ms` or `s` unit.
pub fn parse_duration(raw: &str) -> Result<Duration, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("duration must not be empty".into());
    }

    let (number, unit) = split_number_unit(s)?;
    let value = number
        .parse::<u64>()
        .map_err(|_| format!("invalid duration number in '{s}'"))?;
    match unit.to_ascii_lowercase().as_str() {
        "ms" => Ok(Duration::from_millis(value)),
        "s" => Ok(Duration::from_secs(value)),
        other => Err(format!("duration unit '{other}' must be ms or s")),
    }
}

fn split_number_unit(s: &str) -> Result<(&str, &str), String> {
    let split_at = s
        .bytes()
        .position(|byte| !byte.is_ascii_digit())
        .ok_or_else(|| format!("duration '{s}' must include an ms or s unit"))?;
    let (number, unit) = s.split_at(split_at);
    if number.is_empty() {
        return Err(format!("invalid duration number in '{s}'"));
    }
    if unit.is_empty() {
        return Err(format!("duration '{s}' must include an ms or s unit"));
    }
    Ok((number, unit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bare_duration_without_unit() {
        let err = parse_duration("250").unwrap_err();
        assert!(err.contains("must include an ms or s unit"), "{err}");
    }
}
