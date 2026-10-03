use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::ops::Range;
use std::path::Path;

use crate::core::{Error, Result};

/// Resource limits apply before semantic interpretation. No INCLUDE is opened.
/// Reduce these limits when accepting untrusted or unusually large input; they
/// bound source bytes, physical lines, cards, and fields independently.
#[derive(Debug, Clone, Copy)]
pub struct ParseOptions {
    /// Maximum input bytes (default: 256 MiB).
    pub max_bytes: usize,
    /// Maximum bytes in one physical line (default: 1 MiB).
    pub max_line_bytes: usize,
    /// Maximum physical lines (default: two million).
    pub max_lines: usize,
    /// Maximum parsed cards (default: two million).
    pub max_cards: usize,
    /// Maximum data fields in one card (default: 65,536).
    pub max_fields_per_card: usize,
}

impl Default for ParseOptions {
    /// Apply conservative independent limits for source bytes and parse shape.
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
pub(crate) struct Field {
    pub(crate) range: Option<Range<usize>>,
}

/// Indexed BDF card; source data remains owned by its [`Document`].
#[derive(Debug, Clone)]
pub struct Card {
    name: String,
    /// One-based physical line where the card begins.
    pub line: usize,
    pub(crate) fields: Vec<Field>,
}

impl Card {
    /// Uppercase card keyword, independent of its spelling in the source.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Source-preserving BDF document with indexed cards and narrow typed access.
#[derive(Debug, Clone)]
pub struct Document {
    pub(crate) source: Vec<u8>,
    pub(crate) cards: Vec<Card>,
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

/// Find the first unquoted BDF comment marker, or the end of the line.
/// Quoted dollars remain literal in paths such as `INCLUDE` arguments.
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

/// Borrow the non-whitespace span without decoding arbitrary comment bytes.
fn trim_ascii(bytes: &[u8]) -> &[u8] {
    // Keep slices into the original bytes rather than allocating or decoding.
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

/// Decode a data field as ASCII and attach its one-based physical line on error.
/// Source comments may contain arbitrary bytes; typed data fields may not.
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

/// Recognize a `BEGIN BULK` marker across supported comma/space spellings.
fn is_begin(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes).replace(',', " ");
    let mut words = text.split_ascii_whitespace();
    matches!((words.next(), words.next(), words.next()), (Some(a), Some(b), None) if a.eq_ignore_ascii_case("BEGIN") && b.eq_ignore_ascii_case("BULK"))
}

/// Identify executive/case-control markers that make input a full deck.
fn is_full_marker(bytes: &[u8]) -> bool {
    let upper = String::from_utf8_lossy(bytes).to_ascii_uppercase();
    upper == "CEND"
        || upper.starts_with("SOL ")
        || upper.starts_with("SOL,")
        || upper.starts_with("BEGIN")
}

/// Match an `INCLUDE` keyword only at a complete token boundary.
fn is_include(bytes: &[u8]) -> bool {
    let prefix = bytes.get(..7).unwrap_or(&[]);
    prefix.eq_ignore_ascii_case(b"INCLUDE")
        && bytes
            .get(7)
            .is_none_or(|b| b.is_ascii_whitespace() || *b == b',')
}

/// Carry quote state across a physical line, stopping at unquoted comments.
/// This permits multiline `INCLUDE` path detection without opening files.
fn quote_open(bytes: &[u8], mut quote: Option<u8>) -> Option<u8> {
    // A quote can span physical lines of an INCLUDE argument.
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
    tail: String,
}

/// Index one free-, small-, or large-field physical line into source ranges.
/// Tabs and overfull free-field lines fail because their interpretation varies
/// by BDF dialect; source byte ownership remains with `Document`.
fn physical(source: &[u8], line: &Line, content_end: usize) -> Result<Physical> {
    // Preserve byte ranges for data fields; reject tabs before choosing a
    // fixed or free interpretation.
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
        // In free-field form, commas delimit up to eight data slots (four
        // for large cards), with a possible continuation label at the end.
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
            })
            .collect();
        Ok(Physical { head, fields, tail })
    } else {
        // Fixed-field cards use an eight-column head followed by width 8 or
        // width 16 data slots; absent trailing slots remain implied blanks.
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
        Ok(Physical { head, fields, tail })
    }
}

impl Document {
    /// Index source bytes using default resource limits.
    ///
    /// Comments, unknown cards, and line endings remain in the document. An
    /// `INCLUDE` reference is indexed but its path is never opened.
    ///
    /// # Examples
    ///
    /// ```
    /// use caexfer::bdf::Document;
    /// let source = b"$ note\r\nGRID,7,,0.,0.,0.\r\n";
    /// let doc = Document::parse(source)?;
    /// assert_eq!(doc.to_bytes(), source);
    /// assert_eq!(doc.card_counts()["GRID"], 1);
    /// # Ok::<(), caexfer::core::Error>(())
    /// ```
    pub fn parse(input: impl AsRef<[u8]>) -> Result<Self> {
        Self::parse_with_options(input, ParseOptions::default())
    }

    /// Parse bytes under explicit resource limits.
    ///
    /// # Examples
    ///
    /// ```
    /// use caexfer::bdf::{Document, ParseOptions};
    /// let limits = ParseOptions { max_bytes: 4, ..ParseOptions::default() };
    /// let error = Document::parse_with_options(b"GRID,1,,0,0,0\n", limits).unwrap_err();
    /// assert_eq!(error.code, "E_LIMIT");
    /// ```
    pub fn parse_with_options(input: impl AsRef<[u8]>, options: ParseOptions) -> Result<Self> {
        // Resource limits apply before any card semantics are interpreted.
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

        // Detect a full deck before indexing cards so case-control text is
        // not mistaken for bulk data when BEGIN BULK is required.
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

        // Walk physical lines in source order, preserving their bytes and
        // indexing only cards in the active bulk-data section.
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
                // Record INCLUDE without opening it; quoted paths can span
                // lines, and a pending card continuation cannot cross it.
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
                // Append data fields to the active card only when its
                // continuation label matches the prior physical line.
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
                // A new card cannot silently terminate a labeled
                // continuation expected from the previous line.
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

        // Carry unresolved defaults into later typed GRID and geometry checks.
        let has_grdset = cards.iter().any(|card| card.name() == "GRDSET");
        Ok(Self {
            source,
            cards,
            full_deck,
            has_grdset,
            include_lines,
        })
    }

    /// Open and parse a BDF file using default resource limits.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::read_with_options(File::open(path)?, ParseOptions::default())
    }

    /// Read a bounded byte stream, then parse it under the supplied limits.
    pub fn read_with_options(reader: impl Read, options: ParseOptions) -> Result<Self> {
        // Read one byte beyond the cap to detect oversize streams accurately.
        let limit = options
            .max_bytes
            .checked_add(1)
            .ok_or_else(|| Error::new("E_LIMIT", "max_bytes is too large"))?;
        let mut source = Vec::new();
        reader.take(limit as u64).read_to_end(&mut source)?;
        Self::parse_with_options(source, options)
    }

    /// Indexed cards in source order. Use [`Self::card_text`] to inspect fields.
    pub fn cards(&self) -> &[Card] {
        &self.cards
    }
    /// Original document bytes, unchanged by inspection or projection.
    pub fn to_bytes(&self) -> &[u8] {
        &self.source
    }
    /// Whether the input contained a supported full-deck wrapper.
    pub fn is_full_deck(&self) -> bool {
        self.full_deck
    }
    /// Write current bytes to a caller-owned stream.
    /// An I/O failure can leave a partial output; stage files if needed.
    pub fn write_to(&self, mut writer: impl Write) -> Result<()> {
        writer.write_all(&self.source)?;
        Ok(())
    }

    /// Borrow and trim one indexed data field; an implied blank yields `""`.
    /// Ranges are always interpreted against this document's preserved bytes.
    fn field_text(&self, field: &Field) -> &str {
        // An absent indexed range is an implied blank, not a missing card.
        field
            .range
            .as_ref()
            .and_then(|range| self.source.get(range.clone()))
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .unwrap_or("")
            .trim()
    }

    /// Read a zero-based data field from a card returned by this document's
    /// [`Self::cards`]. For GRID, index 0 is ID, 1 is CP, and 2 is X1. A blank
    /// or absent field returns `""`; this method does not apply card defaults.
    ///
    /// # Examples
    ///
    /// ```
    /// use caexfer::bdf::Document;
    /// let doc = Document::parse("GRID,7,,1.,0.,0.\n")?;
    /// let card = &doc.cards()[0];
    /// assert_eq!(card.name(), "GRID");
    /// assert_eq!(doc.card_text(card, 0), "7");
    /// assert_eq!(doc.card_text(card, 1), ""); // blank CP field
    /// assert_eq!(doc.card_text(card, 2), "1.");
    /// # Ok::<(), caexfer::core::Error>(())
    /// ```
    pub fn card_text(&self, card: &Card, data_index: usize) -> &str {
        card.fields
            .get(data_index)
            .map(|field| self.field_text(field))
            .unwrap_or("")
    }

    /// Count indexed cards by normalized keyword.
    pub fn card_counts(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for card in &self.cards {
            *counts.entry(card.name.clone()).or_default() += 1;
        }
        counts
    }
}
