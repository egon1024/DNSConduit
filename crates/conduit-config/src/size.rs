//! SI decimal size parsing for config fields such as `lmdb.map_size`.
//!
//! Accepts a bare integer (bytes) or a decimal coefficient with suffix
//! `KB` / `MB` / `GB` / `TB` / `PB` (powers of ten). Binary IEC suffixes
//! (`KiB`, `MiB`, `GiB`, …) are rejected.

/// Parse a size string into bytes.
///
/// # Examples
/// - `"1024"` → 1024
/// - `"4GB"` → 4_000_000_000
/// - `"4.5GB"` → 4_500_000_000
///
/// Rejects empty input, unknown suffixes, and IEC forms such as `4GiB`.
pub fn parse_si_size(raw: &str) -> Result<u64, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("size must not be empty".into());
    }

    // Reject IEC / binary suffixes early with a clear message.
    let upper = s.to_ascii_uppercase();
    for iec in ["KIB", "MIB", "GIB", "TIB", "PIB"] {
        if upper.ends_with(iec) {
            return Err(format!(
                "size '{raw}' uses binary IEC suffix; use decimal SI (KB/MB/GB/TB/PB) or bare bytes"
            ));
        }
    }

    // Bare integer bytes (no decimal point, no suffix).
    if s.bytes().all(|b| b.is_ascii_digit()) {
        return s
            .parse::<u64>()
            .map_err(|_| format!("size '{raw}' is not a valid byte count"));
    }

    if s.len() < 3 {
        return Err(format!(
            "size '{raw}' must be bare bytes or a coefficient with SI suffix (KB/MB/GB/TB/PB)"
        ));
    }

    let suffix = &upper[upper.len() - 2..];
    let coef_str = &s[..s.len() - 2];
    if coef_str.is_empty() {
        return Err(format!(
            "size '{raw}' is missing a coefficient before the suffix"
        ));
    }

    let multiplier = match suffix {
        "KB" => 1_000u64,
        "MB" => 1_000_000,
        "GB" => 1_000_000_000,
        "TB" => 1_000_000_000_000,
        "PB" => 1_000_000_000_000_000,
        other => {
            return Err(format!(
                "size '{raw}' has unknown suffix '{other}'; use KB/MB/GB/TB/PB or bare bytes"
            ));
        }
    };

    let coef: f64 = coef_str
        .parse()
        .map_err(|_| format!("size '{raw}' has invalid coefficient '{coef_str}'"))?;
    if !coef.is_finite() || coef < 0.0 {
        return Err(format!(
            "size '{raw}' coefficient must be a non-negative finite number"
        ));
    }

    let bytes = coef * (multiplier as f64);
    if !bytes.is_finite() || bytes > (u64::MAX as f64) {
        return Err(format!("size '{raw}' exceeds maximum u64 bytes"));
    }
    Ok(bytes as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_bytes() {
        assert_eq!(parse_si_size("1024").unwrap(), 1024);
        assert_eq!(parse_si_size("0").unwrap(), 0);
        assert_eq!(parse_si_size(" 4096 ").unwrap(), 4096);
    }

    #[test]
    fn si_suffixes() {
        assert_eq!(parse_si_size("1KB").unwrap(), 1_000);
        assert_eq!(parse_si_size("1MB").unwrap(), 1_000_000);
        assert_eq!(parse_si_size("1GB").unwrap(), 1_000_000_000);
        assert_eq!(parse_si_size("1TB").unwrap(), 1_000_000_000_000);
        assert_eq!(parse_si_size("1PB").unwrap(), 1_000_000_000_000_000);
        assert_eq!(parse_si_size("4GB").unwrap(), 4_000_000_000);
        assert_eq!(parse_si_size("4.5GB").unwrap(), 4_500_000_000);
        assert_eq!(parse_si_size("4.5gb").unwrap(), 4_500_000_000);
    }

    #[test]
    fn iec_rejected() {
        for s in ["4GiB", "1MiB", "2KiB", "1TiB", "1PiB", "4gib"] {
            let err = parse_si_size(s).unwrap_err();
            assert!(
                err.contains("IEC") || err.contains("binary"),
                "expected IEC rejection for {s}: {err}"
            );
        }
    }

    #[test]
    fn unknown_suffix_rejected() {
        assert!(parse_si_size("4XB").unwrap_err().contains("unknown suffix"));
        assert!(parse_si_size("abc").is_err());
        assert!(parse_si_size("").is_err());
    }
}
