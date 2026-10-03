use caexfer_core::{Error, Result};

/// Parse a finite Nastran real, including `1.2-3`, `.7+2`, and `1D+3`.
/// A blank is deliberately an error: defaults belong to the card schema.
pub fn parse_real(input: &str) -> Result<f64> {
    let trimmed = input.trim();
    if trimmed.is_empty() || !trimmed.is_ascii() || trimmed.bytes().any(|b| b.is_ascii_whitespace())
    {
        return Err(Error::new(
            "E_REAL",
            format!("expected a real number, got {input:?}"),
        ));
    }
    let mut normalized = trimmed.replace(['d', 'D'], "E");
    if !normalized.contains(['e', 'E']) {
        let exponent = normalized
            .char_indices()
            .skip(1)
            .find(|(_, c)| *c == '+' || *c == '-')
            .map(|(i, _)| i);
        if let Some(index) = exponent {
            normalized.insert(index, 'E');
        }
    }
    let value = normalized
        .parse::<f64>()
        .map_err(|_| Error::new("E_REAL", format!("invalid Nastran real {input:?}")))?;
    if !value.is_finite() {
        return Err(Error::new(
            "E_NONFINITE",
            format!("non-finite real {input:?}"),
        ));
    }
    Ok(value)
}

/// Represent a value without rounding it to fit a fixed-width field.
/// Returning an error is preferable to quietly changing engineering data.
pub(crate) fn format_real(value: f64, width: Option<usize>) -> Result<String> {
    if !value.is_finite() {
        return Err(Error::new(
            "E_NONFINITE",
            "edited coordinates must be finite",
        ));
    }
    // Nastran distinguishes integer and real fields lexically. Even an exact
    // integer-valued coordinate must retain a decimal point when written.
    let mut plain = value.to_string();
    if !plain.contains('.') {
        plain.push('.');
    }
    let Some(width) = width else {
        return Ok(plain);
    };
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .ok_or_else(|| Error::new("E_REAL", "failed to format scientific real"))?;
    let mut mantissa = mantissa.to_string();
    if !mantissa.contains('.') {
        mantissa.push('.');
    }
    let exp = exponent
        .parse::<i32>()
        .map_err(|_| Error::new("E_REAL", "failed to format real exponent"))?;
    let sci = format!("{mantissa}e{exp}");
    let compact = format!("{mantissa}{exp:+}");
    for candidate in [plain, sci, compact] {
        if candidate.len() <= width {
            if let Ok(parsed) = parse_real(&candidate) {
                if parsed.to_bits() == value.to_bits() {
                    return Ok(format!("{candidate:>width$}"));
                }
            }
        }
    }
    Err(Error::new("E_FIELD_WIDTH", format!("value {value} cannot fit {width} columns without losing precision; use a large/free-field source card")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_notations() {
        for (input, expected) in [
            ("1.2-3", 0.0012),
            (".7+2", 70.0),
            ("1D+3", 1000.0),
            ("-2.5-2", -0.025),
            ("+.1", 0.1),
            ("1", 1.0),
            (" .1d-5 ", 0.000001),
        ] {
            assert!(
                (parse_real(input).unwrap() - expected).abs() <= expected.abs() * 1e-14 + 1e-20,
                "{input}"
            );
        }
    }

    #[test]
    fn rejects_invalid_or_nonfinite() {
        for input in ["", "NaN", "inf", "1e999", "1 2", "1-2-3", "--1", "one"] {
            assert!(parse_real(input).is_err(), "{input}");
        }
    }

    #[test]
    fn preserves_negative_zero() {
        let text = format_real(-0.0, Some(8)).unwrap();
        assert_eq!(parse_real(&text).unwrap().to_bits(), (-0.0f64).to_bits());
    }

    #[test]
    fn refuses_precision_loss() {
        assert_eq!(
            format_real(std::f64::consts::PI, Some(8)).unwrap_err().code,
            "E_FIELD_WIDTH"
        );
    }

    #[test]
    fn writes_decimal_points_for_real_fields() {
        for value in [0.0, -0.0, 1.0, -2.0, 1e20, 1e-20] {
            for width in [None, Some(8), Some(16)] {
                let text = format_real(value, width).unwrap();
                assert!(text.contains('.'), "{text}");
                assert_eq!(parse_real(&text).unwrap().to_bits(), value.to_bits());
            }
        }
    }

    #[test]
    fn compact_exponent_fits() {
        let text = format_real(1.2e20, Some(8)).unwrap();
        assert_eq!(text.len(), 8);
        assert_eq!(parse_real(&text).unwrap(), 1.2e20);
    }
}
