use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::ops::Range;
use std::path::Path;

use crate::number::format_real;
use caxifer_core::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldFormat {
    Small,
    Large,
    Free,
}

/// Resource limits apply before semantic interpretation. No INCLUDE is opened.
#[derive(Debug, Clone, Copy)]
pub struct ParseOptions {
    pub max_bytes: usize,
    pub max_line_bytes: usize,
    pub max_lines: usize,
    pub max_cards: usize,
    pub max_fields_per_card: usize,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            max_bytes: 256 * 1024 * 1024,
            max_line_bytes: 1024 * 1024,
            max_lines: 2_000_000,
            max_cards: 2_000_000,
            max_fields_per_card: 65_536,
        }
    }
}

/// A field's location in the source. An absent range is an implied blank.
#[derive(Debug, Clone)]
pub struct Field {
    pub(crate) range: Option<Range<usize>>,
    pub(crate) width: Option<usize>,
    pub line: usize,
}

impl Field {
    pub fn source_range(&self) -> Option<Range<usize>> {
        self.range.clone()
    }
}

#[derive(Debug, Clone)]
pub struct Card {
    name: String,
    pub line: usize,
    pub format: FieldFormat,
    pub(crate) fields: Vec<Field>,
}

impl Card {
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Zero-based *data* fields: GRID field 0 is ID, 1 is CP, 2 is X1.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }
}

#[derive(Debug, Clone)]
pub struct Document {
    pub(crate) source: Vec<u8>,
    pub(crate) cards: Vec<Card>,
    pub(crate) options: ParseOptions,
    pub(crate) full_deck: bool,
    pub(crate) has_grdset: bool,
    pub(crate) include_lines: Vec<usize>,
}

#[derive(Debug)]
struct Line {
    start: usize,
    end: usize,
    number: usize,
}

fn comment_end(bytes: &[u8]) -> usize {
    // Quotes matter for INCLUDE paths, where a dollar sign can be literal.
    let mut quote = None;
    for (index, &byte) in bytes.iter().enumerate() {
        match byte {
            b'\'' | b'"' if quote == Some(byte) => quote = None,
            b'\'' | b'"' if quote.is_none() => quote = Some(byte),
            b'$' if quote.is_none() => return index,
            _ => {}
        }
    }
    bytes.len()
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |i| i + 1);
    &bytes[start..end]
}

fn text_ascii(bytes: &[u8], line: usize) -> Result<&str> {
    if !bytes.is_ascii() {
        return Err(Error::new(
            "E_ENCODING",
            "non-ASCII data field; arbitrary comment bytes are supported",
        )
        .at(line));
    }
    std::str::from_utf8(bytes)
        .map_err(|_| Error::new("E_ENCODING", "invalid data encoding").at(line))
}

fn is_begin(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes).replace(',', " ");
    let mut words = text.split_ascii_whitespace();
    matches!((words.next(), words.next(), words.next()), (Some(a), Some(b), None) if a.eq_ignore_ascii_case("BEGIN") && b.eq_ignore_ascii_case("BULK"))
}

fn is_full_marker(bytes: &[u8]) -> bool {
    let upper = String::from_utf8_lossy(bytes).to_ascii_uppercase();
    upper == "CEND"
        || upper.starts_with("SOL ")
        || upper.starts_with("SOL,")
        || upper.starts_with("BEGIN")
}

fn is_include(bytes: &[u8]) -> bool {
    let prefix = bytes.get(..7).unwrap_or(&[]);
    prefix.eq_ignore_ascii_case(b"INCLUDE")
        && bytes
            .get(7)
            .is_none_or(|b| b.is_ascii_whitespace() || *b == b',')
}

fn quote_open(bytes: &[u8], mut quote: Option<u8>) -> Option<u8> {
    for &byte in bytes {
        if (byte == b'\'' || byte == b'"') && quote.is_none() {
            quote = Some(byte);
        } else if quote == Some(byte) {
            quote = None;
        } else if byte == b'$' && quote.is_none() {
            break;
        }
    }
    quote
}

struct Physical {
    head: String,
    fields: Vec<Field>,
    format: FieldFormat,
    tail: String,
}

fn physical(source: &[u8], line: &Line, content_end: usize) -> Result<Physical> {
    let bytes = &source[line.start..content_end];
    if bytes.contains(&b'\t') {
        return Err(Error::new(
            "E_TAB",
            "tab expansion is dialect-dependent; use explicit spaces in data fields",
        )
        .at(line.number));
    }
    let free = bytes.iter().take(10).any(|b| *b == b',');
    if free {
        text_ascii(bytes, line.number)?;
        let mut ranges = Vec::new();
        let mut start = line.start;
        for (i, &byte) in bytes.iter().enumerate() {
            if byte == b',' {
                ranges.push(start..line.start + i);
                start = line.start + i + 1;
            }
        }
        ranges.push(start..content_end);
        let head = text_ascii(&source[ranges[0].clone()], line.number)?
            .trim()
            .to_string();
        let large = head.ends_with('*') || head.starts_with('*');
        let capacity = if large { 4 } else { 8 };
        if ranges.len() > capacity + 2 {
            return Err(Error::new("E_FREE_FIELDS", format!("more than {capacity} data fields on a free-field physical line; add a continuation")).at(line.number));
        }
        let tail = if ranges.len() == capacity + 2 {
            let value = text_ascii(&source[ranges[capacity + 1].clone()], line.number)?.trim();
            if !value.is_empty() && !value.starts_with(['+', '*']) {
                return Err(Error::new(
                    "E_CONTINUATION",
                    "extra free field is not a continuation label",
                )
                .at(line.number));
            }
            value.to_string()
        } else {
            String::new()
        };
        let fields = (0..capacity)
            .map(|i| Field {
                range: ranges.get(i + 1).cloned(),
                width: None,
                line: line.number,
            })
            .collect();
        Ok(Physical {
            head,
            fields,
            format: FieldFormat::Free,
            tail,
        })
    } else {
        let head_end = (line.start + 8).min(content_end);
        let head = text_ascii(&source[line.start..head_end], line.number)?
            .trim()
            .to_string();
        let large = head.ends_with('*') || head.starts_with('*');
        let width = if large { 16 } else { 8 };
        let capacity = if large { 4 } else { 8 };
        text_ascii(
            &source[line.start..content_end.min(line.start + 80)],
            line.number,
        )?;
        let fields = (0..capacity)
            .map(|i| {
                let start = line.start + 8 + i * width;
                let end = (start + width).min(content_end);
                Field {
                    range: if start < content_end {
                        Some(start..end)
                    } else {
                        None
                    },
                    // The entire width must physically exist for an in-place edit.
                    width: Some(width),
                    line: line.number,
                }
            })
            .collect();
        let tail = if bytes.len() > 72 {
            text_ascii(&bytes[72..bytes.len().min(80)], line.number)?
                .trim()
                .to_string()
        } else {
            String::new()
        };
        Ok(Physical {
            head,
            fields,
            format: if large {
                FieldFormat::Large
            } else {
                FieldFormat::Small
            },
            tail,
        })
    }
}

impl Document {
    pub fn parse(input: impl AsRef<[u8]>) -> Result<Self> {
        Self::parse_with_options(input, ParseOptions::default())
    }

    pub fn parse_with_options(input: impl AsRef<[u8]>, options: ParseOptions) -> Result<Self> {
        let input = input.as_ref();
        if input.len() > options.max_bytes {
            return Err(Error::new("E_LIMIT", "input exceeds max_bytes"));
        }
        // Keep one owned source buffer; fields point into it rather than copy it.
        let source = input.to_vec();
        let mut lines = Vec::new();
        let mut start = 0;
        for raw in source.split_inclusive(|b| *b == b'\n') {
            let mut len = raw.len();
            if raw.last() == Some(&b'\n') {
                len -= 1;
            }
            if len > 0 && raw[len - 1] == b'\r' {
                len -= 1;
            }
            if len > options.max_line_bytes {
                return Err(
                    Error::new("E_LIMIT", "physical line exceeds max_line_bytes")
                        .at(lines.len() + 1),
                );
            }
            if lines.len() >= options.max_lines {
                return Err(Error::new("E_LIMIT", "max_lines exceeded"));
            }
            lines.push(Line {
                start,
                end: start + len,
                number: lines.len() + 1,
            });
            start += raw.len();
        }
        let mut full_deck = false;
        for line in &lines {
            let raw = &source[line.start..line.end];
            let data = trim_ascii(&raw[..comment_end(raw)]);
            if data.eq_ignore_ascii_case(b"ENDDATA") {
                break;
            }
            if is_full_marker(data) {
                full_deck = true;
                break;
            }
        }
        let mut in_bulk = !full_deck;
        let mut began = !full_deck;
        let mut ended = false;
        let mut cards: Vec<Card> = Vec::new();
        let mut current: Option<usize> = None;
        let mut pending = String::new();
        let mut include_lines = Vec::new();
        let mut include_quote = None;
        for line in &lines {
            let raw = &source[line.start..line.end];
            if ended {
                continue;
            }
            if include_quote.is_some() {
                include_quote = quote_open(raw, include_quote);
                continue;
            }
            let content_end = line.start + comment_end(raw);
            let content = trim_ascii(&source[line.start..content_end]);
            if content.is_empty() {
                continue;
            }
            if is_include(content) {
                if !pending.is_empty() {
                    return Err(Error::new(
                        "E_CONTINUATION",
                        "INCLUDE interrupts a pending continuation",
                    )
                    .at(line.number));
                }
                include_lines.push(line.number);
                cards.push(Card {
                    name: "INCLUDE".into(),
                    line: line.number,
                    format: FieldFormat::Free,
                    fields: Vec::new(),
                });
                if cards.len() > options.max_cards {
                    return Err(Error::new("E_LIMIT", "max_cards exceeded").at(line.number));
                }
                include_quote = quote_open(content, None);
                current = None;
                continue;
            }
            if is_begin(content) {
                if began {
                    return Err(
                        Error::new("E_DECK", "multiple bulk sections are not supported")
                            .at(line.number),
                    );
                }
                in_bulk = true;
                began = true;
                continue;
            }
            if !in_bulk {
                continue;
            }
            if content.eq_ignore_ascii_case(b"ENDDATA") {
                if !pending.is_empty() {
                    return Err(Error::new(
                        "E_CONTINUATION",
                        "ENDDATA before expected continuation",
                    )
                    .at(line.number));
                }
                ended = true;
                continue;
            }
            if content.starts_with(b"BEGIN") || content.starts_with(b"begin") {
                return Err(Error::new(
                    "E_DECK",
                    "superelement/alternate BEGIN sections are not supported",
                )
                .at(line.number));
            }
            let part = physical(&source, line, content_end)?;
            let continuation = part.head.is_empty() || part.head.starts_with(['+', '*']);
            if continuation {
                let index = current.ok_or_else(|| {
                    Error::new("E_CONTINUATION", "orphan continuation").at(line.number)
                })?;
                if !pending.is_empty() && pending != part.head {
                    return Err(Error::new(
                        "E_CONTINUATION",
                        format!("expected label {pending:?}, got {:?}", part.head),
                    )
                    .at(line.number));
                }
                cards[index].fields.extend(part.fields);
                if cards[index].fields.len() > options.max_fields_per_card {
                    return Err(
                        Error::new("E_LIMIT", "max_fields_per_card exceeded").at(line.number)
                    );
                }
                pending = part.tail;
            } else {
                if !pending.is_empty() {
                    return Err(Error::new(
                        "E_CONTINUATION",
                        format!("missing continuation {pending:?}"),
                    )
                    .at(line.number));
                }
                let name = part.head.trim_end_matches('*').to_ascii_uppercase();
                if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                {
                    return Err(Error::new(
                        "E_CARD_NAME",
                        format!("invalid card name {:?}", part.head),
                    )
                    .at(line.number));
                }
                if part.fields.len() > options.max_fields_per_card {
                    return Err(
                        Error::new("E_LIMIT", "max_fields_per_card exceeded").at(line.number)
                    );
                }
                cards.push(Card {
                    name,
                    line: line.number,
                    format: part.format,
                    fields: part.fields,
                });
                if cards.len() > options.max_cards {
                    return Err(Error::new("E_LIMIT", "max_cards exceeded").at(line.number));
                }
                current = Some(cards.len() - 1);
                pending = part.tail;
            }
        }
        if include_quote.is_some() {
            return Err(Error::new("E_INCLUDE", "unterminated quoted INCLUDE path"));
        }
        if !began {
            return Err(Error::new(
                "E_DECK",
                "full deck has no supported BEGIN BULK section",
            ));
        }
        if !pending.is_empty() {
            return Err(Error::new(
                "E_CONTINUATION",
                format!("missing continuation {pending:?} at end of file"),
            ));
        }
        let has_grdset = cards.iter().any(|card| card.name() == "GRDSET");
        Ok(Self {
            source,
            cards,
            options,
            full_deck,
            has_grdset,
            include_lines,
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::read_with_options(File::open(path)?, ParseOptions::default())
    }

    pub fn read_with_options(reader: impl Read, options: ParseOptions) -> Result<Self> {
        let limit = options
            .max_bytes
            .checked_add(1)
            .ok_or_else(|| Error::new("E_LIMIT", "max_bytes is too large"))?;
        let mut source = Vec::new();
        reader.take(limit as u64).read_to_end(&mut source)?;
        Self::parse_with_options(source, options)
    }

    pub fn cards(&self) -> &[Card] {
        &self.cards
    }
    pub fn to_bytes(&self) -> &[u8] {
        &self.source
    }
    pub fn is_full_deck(&self) -> bool {
        self.full_deck
    }
    pub fn include_lines(&self) -> &[usize] {
        &self.include_lines
    }
    pub fn write_to(&self, mut writer: impl Write) -> Result<()> {
        writer.write_all(&self.source)?;
        Ok(())
    }

    /// Extract an indexed field from this document. Card must come from this
    /// document's current `cards()` slice; prefer `card_text` in iterator code.
    pub fn field_text(&self, field: &Field) -> &str {
        field
            .range
            .as_ref()
            .and_then(|range| self.source.get(range.clone()))
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .unwrap_or("")
            .trim()
    }

    pub fn card_text(&self, card: &Card, data_index: usize) -> &str {
        card.fields
            .get(data_index)
            .map(|field| self.field_text(field))
            .unwrap_or("")
    }

    pub fn card_counts(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for card in &self.cards {
            *counts.entry(card.name.clone()).or_default() += 1;
        }
        counts
    }

    /// Edit GRID X1/X2/X3 *in its native CP frame*, not in the basic frame.
    /// All other source bytes stay unchanged. All three edits are transactional.
    /// No field is rounded to fit; implicit/missing fields must first be made
    /// explicit in the source. On failure `self` is unchanged.
    pub fn set_grid_coordinates(&mut self, id: u64, coordinates: [f64; 3]) -> Result<()> {
        let mut found = None;
        for (index, card) in self
            .cards
            .iter()
            .enumerate()
            .filter(|(_, card)| card.name() == "GRID")
        {
            let grid = self.parse_grid(card)?;
            if grid.id == id {
                if found.is_some() {
                    return Err(
                        Error::new("E_DUPLICATE_GRID", format!("GRID {id} is duplicated"))
                            .at(card.line),
                    );
                }
                found = Some(index);
            }
        }
        let index = found.ok_or_else(|| Error::new("E_NOT_FOUND", format!("no GRID {id}")))?;
        let card = &self.cards[index];
        let mut edits = Vec::new();
        for (axis, &value) in coordinates.iter().enumerate() {
            let field = card.fields.get(axis + 2).ok_or_else(|| {
                Error::new(
                    "E_IMPLICIT_FIELD",
                    "coordinate has no explicit source field",
                )
                .at(card.line)
            })?;
            let range = field.range.clone().ok_or_else(|| {
                Error::new(
                    "E_IMPLICIT_FIELD",
                    "coordinate has no explicit source field",
                )
                .at(field.line)
            })?;
            if field.width.is_some_and(|width| range.len() != width) {
                return Err(Error::new(
                    "E_IMPLICIT_FIELD",
                    "fixed-width coordinate field is physically truncated; pad it before editing",
                )
                .at(field.line));
            }
            let replacement =
                format_real(value, field.width).map_err(|error| error.at(field.line))?;
            let replacement = if field.width.is_none() {
                let original = &self.source[range.clone()];
                let left = original
                    .iter()
                    .take_while(|b| b.is_ascii_whitespace())
                    .count();
                let right = original
                    .iter()
                    .rev()
                    .take_while(|b| b.is_ascii_whitespace())
                    .count()
                    .min(original.len() - left);
                let mut output = original[..left].to_vec();
                output.extend_from_slice(replacement.as_bytes());
                output.extend_from_slice(&original[original.len() - right..]);
                output
            } else {
                replacement.into_bytes()
            };
            edits.push((range, replacement));
        }
        edits.sort_by_key(|(range, _)| range.start);
        let mut changed = self.source.clone();
        for (range, replacement) in edits.into_iter().rev() {
            changed.splice(range, replacement);
        }
        let replacement = Self::parse_with_options(changed, self.options)?;
        *self = replacement;
        Ok(())
    }
}
