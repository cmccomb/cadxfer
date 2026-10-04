//! `MagicaVoxel` VOX version 150, one untransformed model.
#![allow(clippy::too_many_lines)] // Chunk validation is kept in one read transaction.

use crate::core::{Error, Result, VoxelGrid};
use std::io::Write;

fn err(message: impl Into<String>) -> Error {
    Error::new("E_VOX", message)
}

fn word(bytes: &[u8]) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes.try_into().map_err(|_| err("truncated VOX word"))?,
    ))
}

/// Source color-index details absent from binary occupancy.
#[derive(Debug)]
pub struct Projection {
    pub grid: VoxelGrid,
    pub colored_voxels: usize,
    pub palette: bool,
}

/// Read a single SIZE/XYZI model. Scene graphs, multiple models and unknown
/// extension chunks fail instead of silently discarding spatial transforms.
pub fn read(bytes: &[u8]) -> Result<Projection> {
    if bytes.len() < 20
        || &bytes[..4] != b"VOX "
        || word(&bytes[4..8])? != 150
        || &bytes[8..12] != b"MAIN"
    {
        return Err(err("expected MagicaVoxel VOX version 150"));
    }
    if word(&bytes[12..16])? != 0
        || usize::try_from(word(&bytes[16..20])?).ok() != Some(bytes.len() - 20)
    {
        return Err(err("invalid MAIN chunk size"));
    }
    let mut offset = 20;
    let mut dims = None;
    let mut voxels = None;
    let mut palette = false;
    while offset < bytes.len() {
        if bytes.len() - offset < 12 {
            return Err(err("truncated VOX chunk"));
        }
        let id = &bytes[offset..offset + 4];
        let size = usize::try_from(word(&bytes[offset + 4..offset + 8])?)
            .map_err(|_| err("chunk too large"))?;
        let children = word(&bytes[offset + 8..offset + 12])?;
        offset += 12;
        let end = offset
            .checked_add(size)
            .ok_or_else(|| err("chunk size overflow"))?;
        if end > bytes.len() || children != 0 {
            return Err(err("invalid or nested VOX chunk"));
        }
        let body = &bytes[offset..end];
        match id {
            b"SIZE" => {
                if dims.is_some() || voxels.is_some() || body.len() != 12 {
                    return Err(err("invalid SIZE chunk order"));
                }
                let d = [word(&body[..4])?, word(&body[4..8])?, word(&body[8..12])?];
                if d.iter().any(|&n| n == 0 || n > 256) {
                    return Err(err("SIZE dimensions must be 1..256"));
                }
                dims = Some(d.map(|n| n as usize));
            }
            b"XYZI" => {
                if voxels.is_some() || dims.is_none() || body.len() < 4 {
                    return Err(err("invalid XYZI chunk order"));
                }
                let n = usize::try_from(word(&body[..4])?).map_err(|_| err("too many voxels"))?;
                if n.checked_mul(4).and_then(|n| n.checked_add(4)) != Some(body.len()) {
                    return Err(err("invalid XYZI length"));
                }
                voxels = Some(body[4..].to_vec());
            }
            b"RGBA" => {
                if palette || body.len() != 1024 {
                    return Err(err("invalid RGBA palette"));
                }
                palette = true;
            }
            _ => {
                return Err(err(
                    "unsupported VOX chunk (multi-model/scene/material data)",
                ));
            }
        }
        offset = end;
    }
    let dims = dims.ok_or_else(|| err("missing SIZE"))?;
    let voxels = voxels.ok_or_else(|| err("missing XYZI"))?;
    let count = dims
        .iter()
        .try_fold(1usize, |a, &b| a.checked_mul(b))
        .filter(|&n| n <= 2_000_000)
        .ok_or_else(|| err("voxel grid exceeds 2,000,000 cells"))?;
    let mut occupied = vec![0; count];
    let mut colored_voxels = 0;
    for item in voxels.chunks_exact(4) {
        let [x, y, z, color] = [
            item[0] as usize,
            item[1] as usize,
            item[2] as usize,
            item[3] as usize,
        ];
        if x >= dims[0] || y >= dims[1] || z >= dims[2] || color == 0 {
            return Err(err("invalid VOX voxel coordinate or color index"));
        }
        let index = (z * dims[1] + y) * dims[0] + x;
        if occupied[index] != 0 {
            return Err(err("duplicate VOX voxel"));
        }
        occupied[index] = 1;
        colored_voxels += usize::from(color != 1);
    }
    let grid = VoxelGrid {
        origin: [0.0; 3],
        spacing: 1.0,
        dims,
        occupied,
    };
    grid.validate()?;
    Ok(Projection {
        grid,
        colored_voxels,
        palette,
    })
}

/// Write one occupancy model with color index 1. Coordinates and scale are
/// absent in VOX; the conversion report must record that loss.
pub fn write(grid: &VoxelGrid, mut writer: impl Write) -> Result<()> {
    grid.validate()?;
    if grid.dims.iter().any(|&n| n > 256) {
        return Err(err("VOX model dimensions must be <= 256 per axis"));
    }
    let count = grid.occupied.iter().filter(|&&v| v != 0).count();
    let xyzi_size = count
        .checked_mul(4)
        .and_then(|n| n.checked_add(4))
        .ok_or_else(|| err("VOX size overflow"))?;
    let children = 24usize
        .checked_add(12)
        .and_then(|n| n.checked_add(xyzi_size))
        .ok_or_else(|| err("VOX size overflow"))?;
    let children = u32::try_from(children).map_err(|_| err("VOX file too large"))?;
    writer.write_all(b"VOX ")?;
    writer.write_all(&150_u32.to_le_bytes())?;
    writer.write_all(b"MAIN")?;
    writer.write_all(&0_u32.to_le_bytes())?;
    writer.write_all(&children.to_le_bytes())?;
    writer.write_all(b"SIZE")?;
    writer.write_all(&12_u32.to_le_bytes())?;
    writer.write_all(&0_u32.to_le_bytes())?;
    for dim in grid.dims {
        writer.write_all(
            &u32::try_from(dim)
                .map_err(|_| err("VOX dimension too large"))?
                .to_le_bytes(),
        )?;
    }
    writer.write_all(b"XYZI")?;
    writer.write_all(
        &u32::try_from(xyzi_size)
            .map_err(|_| err("VOX size overflow"))?
            .to_le_bytes(),
    )?;
    writer.write_all(&0_u32.to_le_bytes())?;
    writer.write_all(
        &u32::try_from(count)
            .map_err(|_| err("too many voxels"))?
            .to_le_bytes(),
    )?;
    for z in 0..grid.dims[2] {
        for y in 0..grid.dims[1] {
            for x in 0..grid.dims[0] {
                if grid.occupied[(z * grid.dims[1] + y) * grid.dims[0] + x] != 0 {
                    writer.write_all(&[
                        u8::try_from(x).map_err(|_| err("VOX coordinate too large"))?,
                        u8::try_from(y).map_err(|_| err("VOX coordinate too large"))?,
                        u8::try_from(z).map_err(|_| err("VOX coordinate too large"))?,
                        1,
                    ])?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occupancy_round_trip_and_rejects_extra_scene_chunk() {
        let grid = VoxelGrid {
            origin: [0.0; 3],
            spacing: 1.0,
            dims: [2, 1, 2],
            occupied: vec![1, 0, 0, 1],
        };
        let mut bytes = Vec::new();
        write(&grid, &mut bytes).unwrap();
        assert_eq!(read(&bytes).unwrap().grid, grid);
        bytes.extend_from_slice(b"nTRN");
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        let children = u32::try_from(bytes.len() - 20).unwrap();
        bytes[16..20].copy_from_slice(&children.to_le_bytes());
        assert_eq!(read(&bytes).unwrap_err().code, "E_VOX");
    }
}
