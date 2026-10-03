//! Bounded 32-bit OP2 record codec for real SORT1 OUGV1 displacements.
use crate::core::{Error, Result};
use std::collections::BTreeMap;

/// One selected displacement step with original GRID IDs.
#[derive(Debug)]
pub(crate) struct Decoded {
    /// Positive subcase identifier.
    pub subcase: i64,
    /// Zero-based step index within the subcase.
    pub step: usize,
    /// Static zero or transient time.
    pub time: f64,
    /// True only for a titled table whose six values are all zero.
    pub assumed_zero: bool,
    /// Six real values indexed by original GRID ID.
    pub rows: BTreeMap<u64, [f64; 6]>,
}

/// One raw OUGV1 step before subcase and step selection.
struct TableStep {
    /// Subcase identifier from table-3.
    subcase: i64,
    /// Static zero or transient time.
    time: f64,
    /// Title marker for an explicitly synthetic zero table.
    assumed_zero: bool,
    /// Six values per original GRID ID.
    rows: BTreeMap<u64, [f64; 6]>,
}

/// Byte order of an OP2 file's 32-bit Fortran records.
#[derive(Clone, Copy)]
enum Endian {
    /// Least significant byte first.
    Little,
    /// Most significant byte first.
    Big,
}

impl Endian {
    /// Decode one signed 32-bit word.
    fn i32(self, bytes: &[u8]) -> Result<i32> {
        let word: [u8; 4] = bytes
            .get(..4)
            .ok_or_else(|| error("truncated OP2 word"))?
            .try_into()
            .map_err(|_| error("truncated OP2 word"))?;
        Ok(match self {
            Self::Little => i32::from_le_bytes(word),
            Self::Big => i32::from_be_bytes(word),
        })
    }

    /// Decode one IEEE-754 binary32 word.
    fn f32(self, bytes: &[u8]) -> Result<f32> {
        let word = self.i32(bytes)?;
        Ok(f32::from_bits(u32::from_ne_bytes(word.to_ne_bytes())))
    }
}

/// Cursor over a sequence of checked Fortran records.
struct Cursor<'a> {
    /// Complete input, including record framing.
    bytes: &'a [u8],
    /// Current byte offset.
    offset: usize,
    /// Record byte order.
    endian: Endian,
}

impl<'a> Cursor<'a> {
    /// Read one length-delimited record and verify its closing length.
    fn record(&mut self) -> Result<&'a [u8]> {
        let start = self.offset;
        let length = usize::try_from(
            self.endian.i32(
                self.bytes
                    .get(start..)
                    .ok_or_else(|| error("truncated OP2 record"))?,
            )?,
        )
        .map_err(|_| error("negative OP2 record length"))?;
        let body_start = start
            .checked_add(4)
            .ok_or_else(|| error("OP2 offset overflow"))?;
        let body_end = body_start
            .checked_add(length)
            .ok_or_else(|| error("OP2 record length overflow"))?;
        let end = body_end
            .checked_add(4)
            .ok_or_else(|| error("OP2 record length overflow"))?;
        let body = self
            .bytes
            .get(body_start..body_end)
            .ok_or_else(|| error("truncated OP2 record"))?;
        let tail = self
            .bytes
            .get(body_end..end)
            .ok_or_else(|| error("truncated OP2 record footer"))?;
        if usize::try_from(self.endian.i32(tail)?).ok() != Some(length) {
            return Err(error("OP2 record length mismatch"));
        }
        self.offset = end;
        Ok(body)
    }

    /// Read a one-word marker record.
    fn marker(&mut self) -> Result<i32> {
        let endian = self.endian;
        let record = self.record()?;
        if record.len() != 4 {
            return Err(error("expected OP2 marker"));
        }
        endian.i32(record)
    }

    /// Require a particular marker value.
    fn expect(&mut self, expected: i32) -> Result<()> {
        if self.marker()? != expected {
            return Err(error(format!("unexpected OP2 marker; expected {expected}")));
        }
        Ok(())
    }
}

/// Attach the format-specific diagnostic code.
fn error(message: impl Into<String>) -> Error {
    Error::new("E_OP2", message)
}

/// Find an actual OUGV1 table-name record after a supported OP2 tape header.
fn table_starts(bytes: &[u8], endian: Endian) -> Result<Vec<usize>> {
    let mut header = Cursor {
        bytes,
        offset: 0,
        endian,
    };
    header.expect(3)?;
    if header.record()?.len() != 12 {
        return Err(error("unsupported OP2 tape header"));
    }
    header.expect(7)?;
    if header.record()? != b"NASTRAN FORT TAPE ID CODE - " {
        return Err(error("unsupported OP2 tape identifier"));
    }
    header.expect(2)?;
    if header.record()?.len() != 8 {
        return Err(error("unsupported OP2 version record"));
    }
    header.expect(-1)?;
    header.expect(0)?;

    // Search for framed table-name records, never a bare string in a payload.
    let mut starts = Vec::new();
    let mut pos = header.offset;
    while pos + 16 <= bytes.len() {
        let candidate = bytes
            .get(pos..pos + 16)
            .ok_or_else(|| error("OP2 offset overflow"))?;
        if endian.i32(candidate)? == 8
            && &candidate[4..12] == b"OUGV1   "
            && endian.i32(&candidate[12..])? == 8
        {
            let marker_start = pos
                .checked_sub(12)
                .ok_or_else(|| error("bad OP2 table header"))?;
            let mut marker = Cursor {
                bytes,
                offset: marker_start,
                endian,
            };
            if marker.marker()? == 2 {
                starts.push(pos + 16);
            }
        }
        pos += 4;
    }
    Ok(starts)
}

/// Interpret one 584-byte table-3 record as a supported real displacement step.
fn table3(bytes: &[u8], endian: Endian) -> Result<(i64, f64, bool)> {
    if bytes.len() != 584 {
        return Err(error("invalid OUGV1 table-3 length"));
    }
    let word = |index: usize| endian.i32(&bytes[index * 4..index * 4 + 4]);
    let approach = word(0)?;
    let analysis = approach / 10;
    if !matches!(analysis, 1 | 6)
        || word(1)? != 1
        || word(2)? != 0
        || word(8)? != 1
        || word(9)? != 8
        || word(22)? != 0
    {
        return Err(error(
            "only real SORT1 OUGV1 displacement tables are supported",
        ));
    }
    let subcase = i64::from(word(3)?);
    if subcase <= 0 {
        return Err(error("invalid OP2 displacement subcase"));
    }
    let time = if analysis == 6 {
        f64::from(endian.f32(&bytes[16..20])?)
    } else {
        0.0
    };
    if !time.is_finite() {
        return Err(error("nonfinite OP2 displacement time"));
    }
    let title = &bytes[200..328];
    let assumed_zero = title
        .windows(b"CAEXFER ASSUMED ZERO DISPLACEMENT".len())
        .any(|window| window == b"CAEXFER ASSUMED ZERO DISPLACEMENT");
    Ok((subcase, time, assumed_zero))
}

/// Decode one table-4 record of eight 32-bit words per GRID.
fn table4(bytes: &[u8], endian: Endian) -> Result<BTreeMap<u64, [f64; 6]>> {
    if bytes.is_empty() || bytes.len() % 32 != 0 {
        return Err(error("invalid OUGV1 displacement row length"));
    }
    let mut rows = BTreeMap::new();
    for row in bytes.chunks_exact(32) {
        let encoded_id = endian.i32(row)?;
        let grid_type = endian.i32(&row[4..])?;
        let id = encoded_id / 10;
        if encoded_id <= 0 || id <= 0 || grid_type != 1 {
            return Err(error("invalid OP2 GRID displacement row"));
        }
        let mut values = [0.; 6];
        for (index, value) in values.iter_mut().enumerate() {
            *value = f64::from(endian.f32(&row[8 + index * 4..])?);
            if !value.is_finite() {
                return Err(error(format!("nonfinite result for GRID {id}")));
            }
        }
        let id = u64::try_from(id).map_err(|_| error("invalid OP2 GRID ID"))?;
        if rows.insert(id, values).is_some() {
            return Err(error("duplicate OP2 displacement GRID"));
        }
    }
    Ok(rows)
}

/// Decode all steps in one OUGV1 table, verifying its marker sequence.
fn decode_table(bytes: &[u8], endian: Endian, start: usize) -> Result<Vec<TableStep>> {
    let mut cursor = Cursor {
        bytes,
        offset: start,
        endian,
    };
    cursor.expect(-1)?;
    cursor.expect(7)?;
    if cursor.record()?.len() != 28 {
        return Err(error("invalid OUGV1 table header"));
    }
    for expected in [-2, 1, 0, 7] {
        cursor.expect(expected)?;
    }
    if cursor.record()?.len() != 28 {
        return Err(error("invalid OUGV1 subtable header"));
    }
    let mut rows = Vec::new();
    let mut index = -3;
    loop {
        for expected in [index, 1, 0] {
            cursor.expect(expected)?;
        }
        let words = cursor.marker()?;
        if words == 0 {
            break;
        }
        if words != 146 {
            return Err(error("expected OUGV1 table-3 record"));
        }
        let (subcase, time, assumed_zero) = table3(cursor.record()?, endian)?;
        index -= 1;
        for expected in [index, 1, 0] {
            cursor.expect(expected)?;
        }
        let count =
            usize::try_from(cursor.marker()?).map_err(|_| error("invalid OUGV1 data length"))?;
        let data = cursor.record()?;
        if count.checked_mul(4) != Some(data.len()) {
            return Err(error("OUGV1 data length mismatch"));
        }
        let values = table4(data, endian)?;
        if assumed_zero
            && values
                .values()
                .any(|row| row.iter().any(|value| *value != 0.0))
        {
            return Err(error(
                "OP2 assumed-zero title conflicts with nonzero displacement data",
            ));
        }
        rows.push(TableStep {
            subcase,
            time,
            assumed_zero,
            rows: values,
        });
        index -= 1;
    }
    if rows.is_empty() {
        return Err(error("empty OUGV1 displacement table"));
    }
    Ok(rows)
}

/// Decode the selected subcase and step from a 32-bit OP2 file.
pub(crate) fn decode(bytes: &[u8], subcase: Option<i64>, step: Option<usize>) -> Result<Decoded> {
    let endian = if bytes.starts_with(&4_i32.to_le_bytes()) {
        Endian::Little
    } else if bytes.starts_with(&4_i32.to_be_bytes()) {
        Endian::Big
    } else {
        return Err(error("unsupported OP2 record word size or byte order"));
    };
    let mut cases: BTreeMap<i64, Vec<TableStep>> = BTreeMap::new();
    for start in table_starts(bytes, endian)? {
        for table_step in decode_table(bytes, endian, start)? {
            cases
                .entry(table_step.subcase)
                .or_default()
                .push(table_step);
        }
    }
    let key = match subcase {
        Some(key) => key,
        None if cases.len() == 1 => *cases
            .keys()
            .next()
            .ok_or_else(|| error("OP2 contains no displacement table"))?,
        None if cases.is_empty() => {
            return Err(error("OP2 contains no supported OUGV1 displacement table"));
        }
        None => return Err(error("multiple displacement subcases; select --subcase")),
    };
    let steps = cases
        .get_mut(&key)
        .ok_or_else(|| error(format!("subcase {key} has no displacement table")))?;
    let selected_step = match step {
        Some(step) => step,
        None if steps.len() == 1 => 0,
        None => return Err(error("multiple result steps; select --step (zero-based)")),
    };
    if selected_step >= steps.len() {
        return Err(error("result step is out of range"));
    }
    let selected = steps.swap_remove(selected_step);
    Ok(Decoded {
        subcase: key,
        step: selected_step,
        time: selected.time,
        assumed_zero: selected.assumed_zero,
        rows: selected.rows,
    })
}

/// Append one little-endian signed word to an OP2 output buffer.
fn word(out: &mut Vec<u8>, value: i32) {
    out.extend(value.to_le_bytes());
}

/// Append one length-delimited Fortran record.
fn record(out: &mut Vec<u8>, data: &[u8]) -> Result<()> {
    let length = i32::try_from(data.len()).map_err(|_| error("OP2 record exceeds 32-bit size"))?;
    word(out, length);
    out.extend(data);
    word(out, length);
    Ok(())
}

/// Append one 32-bit marker record.
fn marker(out: &mut Vec<u8>, value: i32) {
    word(out, 4);
    word(out, value);
    word(out, 4);
}

/// Append a sequence of marker records.
fn markers(out: &mut Vec<u8>, values: &[i32]) {
    for &value in values {
        marker(out, value);
    }
}

/// Encode one deterministic MSC-style real OUGV1 displacement table.
pub(crate) fn encode(
    rows: &[(u64, [f32; 6])],
    subcase: i32,
    time: Option<f32>,
    assumed_zero: bool,
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut stamp = Vec::new();
    for value in [1, 1, 0] {
        word(&mut stamp, value);
    }
    marker(&mut out, 3);
    record(&mut out, &stamp)?;
    marker(&mut out, 7);
    record(&mut out, b"NASTRAN FORT TAPE ID CODE - ")?;
    marker(&mut out, 2);
    record(&mut out, b"XXXXXXXX")?;
    markers(&mut out, &[-1, 0, 2]);
    record(&mut out, b"OUGV1   ")?;
    marker(&mut out, -1);
    marker(&mut out, 7);
    let mut table1 = Vec::new();
    for value in [102, 0, 0, 0, 512, 0, 0] {
        word(&mut table1, value);
    }
    record(&mut out, &table1)?;
    markers(&mut out, &[-2, 1, 0, 7]);
    let mut table2 = b"OUG1    ".to_vec();
    for value in [1, 1, 0, 0, 1] {
        word(&mut table2, value);
    }
    record(&mut out, &table2)?;
    markers(&mut out, &[-3, 1, 0, 146]);

    let mut table3 = vec![0_u8; 584];
    let values = [if time.is_some() { 62 } else { 12 }, 1, 0, subcase];
    for (index, value) in values.into_iter().enumerate() {
        table3[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    if let Some(value) = time {
        table3[16..20].copy_from_slice(&value.to_le_bytes());
    }
    for (index, value) in [(8, 1_i32), (9, 8), (22, 0)] {
        table3[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    table3[200..584].fill(b' ');
    if assumed_zero {
        let title = b"CAEXFER ASSUMED ZERO DISPLACEMENT - NOT SOLVER RESULTS";
        table3[200..200 + title.len()].copy_from_slice(title);
    }
    record(&mut out, &table3)?;
    let count = rows
        .len()
        .checked_mul(8)
        .ok_or_else(|| error("OP2 data count overflow"))?;
    let count = i32::try_from(count).map_err(|_| error("OP2 data count exceeds 32-bit size"))?;
    markers(&mut out, &[-4, 1, 0, count]);
    let payload_size = rows
        .len()
        .checked_mul(32)
        .ok_or_else(|| error("OP2 data length overflow"))?;
    let mut payload = Vec::with_capacity(payload_size);
    for (id, values) in rows {
        let id = i32::try_from(*id).map_err(|_| error("OP2 GRID ID exceeds 32-bit range"))?;
        let encoded = id
            .checked_mul(10)
            .and_then(|value| value.checked_add(2))
            .ok_or_else(|| error("OP2 GRID ID exceeds encoded range"))?;
        word(&mut payload, encoded);
        word(&mut payload, 1);
        for value in values {
            payload.extend(value.to_le_bytes());
        }
    }
    record(&mut out, &payload)?;
    markers(&mut out, &[-5, 1, 0, 0, 0]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_values(actual: [f64; 6], expected: [f64; 6]) {
        for (left, right) in actual.into_iter().zip(expected) {
            assert!((left - right).abs() < 1e-6);
        }
    }

    #[test]
    fn externally_written_subcases_require_selection() {
        let bytes = include_bytes!("../../tests/fixtures/two-subcases.op2");
        assert!(
            decode(bytes, None, None)
                .unwrap_err()
                .message
                .contains("multiple displacement subcases")
        );
        let selected = decode(bytes, Some(2), None).unwrap();
        assert_eq!(selected.subcase, 2);
        assert_values(selected.rows[&10], [2., 4., 6., 8., 10., 12.]);
        assert_values(selected.rows[&20], [14., 16., 18., 20., 22., 24.]);
    }

    #[test]
    fn externally_written_transient_steps_require_selection() {
        let bytes = include_bytes!("../../tests/fixtures/two-steps.op2");
        assert!(
            decode(bytes, Some(3), None)
                .unwrap_err()
                .message
                .contains("multiple result steps")
        );
        let selected = decode(bytes, Some(3), Some(1)).unwrap();
        assert_eq!(selected.step, 1);
        assert!((selected.time - 0.5).abs() < 1e-6);
        assert_values(selected.rows[&10], [2., 4., 6., 8., 10., 12.]);
    }

    #[test]
    fn record_lengths_and_zero_provenance_are_checked() {
        let rows = [(10, [0_f32; 6])];
        let mut bytes = encode(&rows, 1, None, true).unwrap();
        assert!(decode(&bytes, None, None).unwrap().assumed_zero);
        bytes.truncate(bytes.len() - 40);
        assert!(decode(&bytes, None, None).is_err());

        let mut bytes = encode(&rows, 1, None, true).unwrap();
        let data = bytes
            .windows(4)
            .position(|window| window == 102_i32.to_le_bytes())
            .unwrap();
        bytes[data - 4..data].copy_from_slice(&27_i32.to_le_bytes());
        assert!(decode(&bytes, None, None).is_err());
    }
}
