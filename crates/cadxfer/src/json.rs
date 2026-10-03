//! A write-only JSON formatter. User input is never evaluated as JSON.
use std::fmt::Write;

pub fn quote(text: &str) -> String {
    let mut output = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch <= '\u{1f}' => {
                let _ = write!(output, "\\u{:04x}", ch as u32);
            }
            ch => output.push(ch),
        }
    }
    output.push('"');
    output
}

pub fn object<K: AsRef<str>>(fields: impl IntoIterator<Item = (K, String)>) -> String {
    format!(
        "{{{}}}",
        fields
            .into_iter()
            .map(|(key, value)| format!("{}:{value}", quote(key.as_ref())))
            .collect::<Vec<_>>()
            .join(",")
    )
}

pub fn array(values: impl IntoIterator<Item = String>) -> String {
    format!("[{}]", values.into_iter().collect::<Vec<_>>().join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escapes_every_control_character() {
        assert_eq!(quote("a\"b\\c\n\r\t\0"), "\"a\\\"b\\\\c\\n\\r\\t\\u0000\"");
    }
    #[test]
    fn preserves_unicode() {
        assert_eq!(quote("Δ"), "\"Δ\"");
    }
    #[test]
    fn builds_objects() {
        assert_eq!(
            object([("x", "1".into()), ("y", quote("z"))]),
            "{\"x\":1,\"y\":\"z\"}"
        );
    }
}
