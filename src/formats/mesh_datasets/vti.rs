//! ASCII VTK XML `ImageData` with one binary cell occupancy array.
#![allow(clippy::too_many_lines, clippy::float_cmp)]
// Exact spacing comparison enforces the supported isotropic subset.

use crate::core::{Error, Result, VoxelGrid};
use quick_xml::{XmlVersion, events::Event, reader::Reader};
use std::io::Write;

fn err(message: impl Into<String>) -> Error {
    Error::new("E_VTI", message)
}

fn attr(start: &quick_xml::events::BytesStart<'_>, name: &str) -> Result<Option<String>> {
    let mut found = None;
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|e| err(e.to_string()))?;
        if attribute.key.as_ref() == name {
            if found.is_some() {
                return Err(err("duplicate XML attribute"));
            }
            found = Some(
                attribute
                    .normalized_value(XmlVersion::Implicit1_0)
                    .map_err(|e| err(e.to_string()))?
                    .into_owned(),
            );
        }
    }
    Ok(found)
}

fn numbers<T: std::str::FromStr>(text: &str, count: usize) -> Result<Vec<T>> {
    let values: Vec<T> = text
        .split_whitespace()
        .map(|x| x.parse().map_err(|_| err("invalid numeric value")))
        .collect::<Result<_>>()?;
    if values.len() != count {
        return Err(err("wrong number of values"));
    }
    Ok(values)
}

/// Read a single-piece ASCII `ImageData` file with `CellData/occupancy` `UInt8`.
/// Other arrays, compression, nonzero extents, and anisotropic spacing fail.
pub fn read(source: &str) -> Result<VoxelGrid> {
    let mut reader = Reader::from_str(source);
    let mut stack = Vec::<String>::new();
    let mut origin = None;
    let mut spacing = None;
    let mut dims = None;
    let mut cells = None;
    let mut array_text = String::new();
    let mut piece_count = 0;
    loop {
        match reader.read_event().map_err(|e| err(e.to_string()))? {
            Event::Start(start) => {
                let name = start.name().as_ref().to_owned();
                if stack.is_empty() {
                    if name != "VTKFile" || attr(&start, "type")?.as_deref() != Some("ImageData") {
                        return Err(err("expected VTK ImageData"));
                    }
                } else if name == "ImageData" {
                    if stack.last().map(String::as_str) != Some("VTKFile") || origin.is_some() {
                        return Err(err("unexpected ImageData"));
                    }
                    if let Some(direction) = attr(&start, "Direction")? {
                        if numbers::<f64>(&direction, 9)?
                            != [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
                        {
                            return Err(err("rotated ImageData is unsupported"));
                        }
                    }
                    let o = attr(&start, "Origin")?.ok_or_else(|| err("missing Origin"))?;
                    let s = attr(&start, "Spacing")?.ok_or_else(|| err("missing Spacing"))?;
                    let e =
                        attr(&start, "WholeExtent")?.ok_or_else(|| err("missing WholeExtent"))?;
                    origin = Some(
                        numbers::<f64>(&o, 3)?
                            .try_into()
                            .map_err(|_| err("invalid Origin"))?,
                    );
                    let s = numbers::<f64>(&s, 3)?;
                    if s[0] != s[1] || s[1] != s[2] {
                        return Err(err("anisotropic spacing is unsupported"));
                    }
                    spacing = Some(s[0]);
                    let e = numbers::<i64>(&e, 6)?;
                    if e[0] != 0 || e[2] != 0 || e[4] != 0 {
                        return Err(err("nonzero extent origin is unsupported"));
                    }
                    dims = Some([e[1], e[3], e[5]].map(|v| usize::try_from(v).unwrap_or(0)));
                } else if name == "Piece" {
                    piece_count += 1;
                    if piece_count != 1 || stack.last().map(String::as_str) != Some("ImageData") {
                        return Err(err("expected one Piece"));
                    }
                    let extent =
                        attr(&start, "Extent")?.ok_or_else(|| err("missing Piece Extent"))?;
                    let e = numbers::<i64>(&extent, 6)?;
                    let d = dims.ok_or_else(|| err("missing ImageData"))?;
                    if e != [
                        0,
                        i64::try_from(d[0]).map_err(|_| err("extent too large"))?,
                        0,
                        i64::try_from(d[1]).map_err(|_| err("extent too large"))?,
                        0,
                        i64::try_from(d[2]).map_err(|_| err("extent too large"))?,
                    ] {
                        return Err(err("Piece Extent differs from WholeExtent"));
                    }
                } else if name == "CellData" {
                    if stack.last().map(String::as_str) != Some("Piece") {
                        return Err(err("unexpected CellData"));
                    }
                } else if name == "PointData" {
                    if stack.last().map(String::as_str) != Some("Piece") {
                        return Err(err("unexpected PointData"));
                    }
                } else if name == "DataArray" {
                    if stack.last().map(String::as_str) != Some("CellData")
                        || cells.is_some()
                        || attr(&start, "Name")?.as_deref() != Some("occupancy")
                        || attr(&start, "type")?.as_deref() != Some("UInt8")
                        || attr(&start, "format")?.as_deref() != Some("ascii")
                    {
                        return Err(err("only ASCII UInt8 CellData occupancy is supported"));
                    }
                    array_text.clear();
                } else {
                    return Err(err("unsupported XML element"));
                }
                stack.push(name);
            }
            Event::Empty(start) => {
                if start.name().as_ref() != "PointData"
                    || stack.last().map(String::as_str) != Some("Piece")
                {
                    return Err(err("unsupported empty XML element"));
                }
            }
            Event::Text(value) => {
                let text = value.xml10_content();
                if stack.last().map(String::as_str) == Some("DataArray") {
                    array_text.push_str(&text);
                } else if !text.trim().is_empty() {
                    return Err(err("unexpected XML text"));
                }
            }
            Event::End(end) => {
                let name = stack.pop().ok_or_else(|| err("unexpected XML close"))?;
                if name != end.name().as_ref() {
                    return Err(err("mismatched XML close"));
                }
                if name == "DataArray" {
                    let count = dims
                        .ok_or_else(|| err("missing dimensions"))?
                        .iter()
                        .try_fold(1usize, |a, &b| a.checked_mul(b))
                        .filter(|&n| n <= 2_000_000)
                        .ok_or_else(|| err("voxel grid exceeds limit"))?;
                    cells = Some(numbers::<u8>(&array_text, count)?);
                }
            }
            Event::Decl(_) | Event::Comment(_) => {}
            Event::Eof => break,
            _ => return Err(err("unsupported XML construct")),
        }
    }
    if !stack.is_empty() || piece_count != 1 {
        return Err(err("incomplete ImageData"));
    }
    let grid = VoxelGrid {
        origin: origin.ok_or_else(|| err("missing Origin"))?,
        spacing: spacing.ok_or_else(|| err("missing Spacing"))?,
        dims: dims.ok_or_else(|| err("missing WholeExtent"))?,
        occupied: cells.ok_or_else(|| err("missing occupancy"))?,
    };
    grid.validate()?;
    Ok(grid)
}

/// Write a single-piece ASCII `ImageData` occupancy grid.
pub fn write(grid: &VoxelGrid, mut writer: impl Write) -> Result<()> {
    grid.validate()?;
    let [nx, ny, nz] = grid.dims;
    let [ox, oy, oz] = grid.origin;
    writeln!(writer, "<?xml version=\"1.0\"?>")?;
    writeln!(
        writer,
        "<VTKFile type=\"ImageData\" version=\"0.1\" byte_order=\"LittleEndian\">"
    )?;
    writeln!(
        writer,
        "<ImageData WholeExtent=\"0 {nx} 0 {ny} 0 {nz}\" Origin=\"{ox} {oy} {oz}\" Spacing=\"{0} {0} {0}\">",
        grid.spacing
    )?;
    writeln!(
        writer,
        "<Piece Extent=\"0 {nx} 0 {ny} 0 {nz}\"><PointData/><CellData Scalars=\"occupancy\">"
    )?;
    writeln!(
        writer,
        "<DataArray type=\"UInt8\" Name=\"occupancy\" format=\"ascii\">"
    )?;
    for row in grid.occupied.chunks(nx) {
        for value in row {
            write!(writer, "{value} ")?;
        }
        writeln!(writer)?;
    }
    writeln!(
        writer,
        "</DataArray></CellData></Piece></ImageData></VTKFile>"
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occupancy_origin_and_spacing_round_trip() {
        let grid = VoxelGrid {
            origin: [-2.0, 3.0, 4.0],
            spacing: 0.5,
            dims: [2, 1, 1],
            occupied: vec![1, 0],
        };
        let mut bytes = Vec::new();
        write(&grid, &mut bytes).unwrap();
        assert_eq!(read(std::str::from_utf8(&bytes).unwrap()).unwrap(), grid);
        let vtk_style = String::from_utf8(bytes)
            .unwrap()
            .replace("<PointData/>", "<PointData>\n</PointData>");
        assert_eq!(read(&vtk_style).unwrap(), grid);
    }

    #[test]
    fn rejects_unsupported_or_malformed_data() {
        let grid = VoxelGrid {
            origin: [0.0; 3],
            spacing: 1.0,
            dims: [1, 1, 1],
            occupied: vec![1],
        };
        let mut bytes = Vec::new();
        write(&grid, &mut bytes).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert_eq!(
            read(&text.replace("UInt8", "Float64")).unwrap_err().code,
            "E_VTI"
        );
        assert_eq!(
            read(&text.replace("0 1 0 1 0 1", "0 999999999 0 999999999 0 999999999"))
                .unwrap_err()
                .code,
            "E_VTI"
        );

        let cases = [
            (
                text.replace("<PointData/>", "<FieldData/>"),
                "unsupported empty XML element",
            ),
            (
                text.replace("<PointData/>", "<PointData/>unexpected"),
                "unexpected XML text",
            ),
            (
                text.replace("</DataArray>", "</OtherArray>"),
                "ill-formed document",
            ),
            (text.replace("</VTKFile>", ""), "incomplete ImageData"),
            (
                text.replace(
                    "<PointData/>",
                    "<CellData><PointData></PointData></CellData>",
                ),
                "unexpected PointData",
            ),
            (
                text.replace(
                    "<PointData/>",
                    "<PointData><CellData></CellData></PointData>",
                ),
                "unexpected CellData",
            ),
            (
                text.replace("<PointData/>", "<PointData/><Other/>"),
                "unsupported empty XML element",
            ),
            (
                text.replace("1 \n</DataArray>", "1 0 \n</DataArray>"),
                "wrong number of values",
            ),
        ];
        for (source, expected) in cases {
            let Err(failure) = read(&source) else {
                panic!("expected {expected} rejection");
            };
            assert!(failure.message.contains(expected), "{expected}: {failure}");
        }

        let oriented = text.replace(
            "Spacing=\"1 1 1\"",
            "Spacing=\"1 1 1\" Direction=\"1 0 0 0 1 0 0 0 1\"",
        );
        assert_eq!(read(&oriented).unwrap(), grid);
    }

    #[test]
    fn rejects_geometry_metadata_and_occupancy_that_cannot_be_preserved() {
        let grid = VoxelGrid {
            origin: [0.0; 3],
            spacing: 1.0,
            dims: [1, 1, 1],
            occupied: vec![1],
        };
        let mut bytes = Vec::new();
        write(&grid, &mut bytes).unwrap();
        let valid = String::from_utf8(bytes).unwrap();
        for (needle, replacement, expected) in [
            ("type=\"ImageData\"", "type=\"PolyData\"", "E_VTI"),
            (
                "<ImageData ",
                "<ImageData Direction=\"0 1 0 1 0 0 0 0 1\" ",
                "E_VTI",
            ),
            ("Spacing=\"1 1 1\"", "Spacing=\"1 2 1\"", "E_VTI"),
            (
                "WholeExtent=\"0 1 0 1 0 1\"",
                "WholeExtent=\"1 2 0 1 0 1\"",
                "E_VTI",
            ),
            (
                "<Piece Extent=\"0 1 0 1 0 1\"",
                "<Piece Extent=\"0 2 0 1 0 1\"",
                "E_VTI",
            ),
            ("Name=\"occupancy\"", "Name=\"density\"", "E_VTI"),
            ("format=\"ascii\"", "format=\"binary\"", "E_VTI"),
            ("Origin=\"0 0 0\" ", "", "E_VTI"),
            ("1 \n</DataArray>", "2 \n</DataArray>", "E_VOXEL"),
        ] {
            assert!(valid.contains(needle), "missing fixture substring {needle}");
            let changed = valid.replacen(needle, replacement, 1);
            assert_eq!(read(&changed).unwrap_err().code, expected, "{needle}");
        }
        let start = valid.find("<DataArray").unwrap();
        let end = valid.find("</DataArray>").unwrap() + "</DataArray>".len();
        let without_occupancy = format!("{}{}", &valid[..start], &valid[end..]);
        assert_eq!(read(&without_occupancy).unwrap_err().code, "E_VTI");
    }
}
