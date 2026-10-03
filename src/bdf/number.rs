use crate::core::{Error, Result};

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
}
