//! Bounded occupancy grids and deliberately simple mesh projections.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::items_after_statements,
    clippy::too_many_lines
)]
// Grid dimensions are bounded to two million cells before coordinate casts.

use std::collections::BTreeMap;

use super::{Cell, CellKind, Error, Mesh, Point, Result};

/// A cell-centered, axis-aligned occupancy grid. Coordinates have the source's
/// units; `origin` is the minimum corner of cell `[0, 0, 0]`.
#[derive(Debug, Clone, PartialEq)]
pub struct VoxelGrid {
    pub origin: [f64; 3],
    pub spacing: f64,
    pub dims: [usize; 3],
    pub occupied: Vec<u8>,
}

impl VoxelGrid {
    /// Check finite geometry, dimensions, and binary occupancy.
    pub fn validate(&self) -> Result<()> {
        if !self.spacing.is_finite()
            || self.spacing <= 0.0
            || !self.origin.iter().all(|x| x.is_finite())
            || self.dims.contains(&0)
        {
            return Err(Error::new(
                "E_VOXEL",
                "invalid voxel origin, spacing, or dimensions",
            ));
        }
        let count = self
            .dims
            .iter()
            .try_fold(1usize, |a, &b| a.checked_mul(b))
            .ok_or_else(|| Error::new("E_VOXEL", "voxel dimensions overflow"))?;
        if count != self.occupied.len() || count > 2_000_000 || self.occupied.iter().any(|&v| v > 1)
        {
            return Err(Error::new(
                "E_VOXEL",
                "invalid or oversized occupancy array",
            ));
        }
        for axis in 0..3 {
            if !((self.dims[axis] as f64).mul_add(self.spacing, self.origin[axis])).is_finite() {
                return Err(Error::new("E_VOXEL", "voxel bounds are not finite"));
            }
        }
        Ok(())
    }

    fn index(&self, xyz: [usize; 3]) -> usize {
        (xyz[2] * self.dims[1] + xyz[1]) * self.dims[0] + xyz[0]
    }

    fn corner(&self, xyz: [usize; 3]) -> [f64; 3] {
        std::array::from_fn(|axis| (xyz[axis] as f64).mul_add(self.spacing, self.origin[axis]))
    }

    /// Emit one conforming Hex8 per occupied voxel. Internal faces remain
    /// shared through common point indices.
    pub fn volume_mesh(&self) -> Result<Mesh> {
        self.validate()?;
        let mut mesh = Mesh::default();
        let mut corners = BTreeMap::new();
        const OFFSETS: [[usize; 3]; 8] = [
            [0, 0, 0],
            [1, 0, 0],
            [1, 1, 0],
            [0, 1, 0],
            [0, 0, 1],
            [1, 0, 1],
            [1, 1, 1],
            [0, 1, 1],
        ];
        for z in 0..self.dims[2] {
            for y in 0..self.dims[1] {
                for x in 0..self.dims[0] {
                    if self.occupied[self.index([x, y, z])] == 0 {
                        continue;
                    }
                    let mut connectivity = Vec::with_capacity(8);
                    for offset in OFFSETS {
                        let key = [x + offset[0], y + offset[1], z + offset[2]];
                        let index = *corners.entry(key).or_insert_with(|| {
                            let index = mesh.points.len();
                            mesh.points.push(Point {
                                id: (index + 1) as u64,
                                position: self.corner(key),
                            });
                            index
                        });
                        connectivity.push(index);
                    }
                    mesh.cells.push(Cell {
                        id: (mesh.cells.len() + 1) as u64,
                        kind: CellKind::Hex8,
                        connectivity,
                        property_id: None,
                    });
                }
            }
        }
        mesh.validate()?;
        Ok(mesh)
    }

    /// Emit only exposed voxel faces as outward-wound triangles for STL.
    pub fn surface_mesh(&self) -> Result<Mesh> {
        self.validate()?;
        let mut mesh = Mesh::default();
        let mut corners = BTreeMap::new();
        // Face corner offsets are wound outward for each of the six sides.
        const FACES: [([[usize; 3]; 4], [isize; 3]); 6] = [
            ([[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]], [-1, 0, 0]),
            ([[1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]], [1, 0, 0]),
            ([[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]], [0, -1, 0]),
            ([[0, 1, 0], [0, 1, 1], [1, 1, 1], [1, 1, 0]], [0, 1, 0]),
            ([[0, 0, 0], [0, 1, 0], [1, 1, 0], [1, 0, 0]], [0, 0, -1]),
            ([[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]], [0, 0, 1]),
        ];
        for z in 0..self.dims[2] {
            for y in 0..self.dims[1] {
                for x in 0..self.dims[0] {
                    if self.occupied[self.index([x, y, z])] == 0 {
                        continue;
                    }
                    for (offsets, direction) in FACES {
                        let neighbor = [
                            x as isize + direction[0],
                            y as isize + direction[1],
                            z as isize + direction[2],
                        ];
                        if neighbor
                            .iter()
                            .enumerate()
                            .all(|(axis, &v)| v >= 0 && v < self.dims[axis] as isize)
                            && self.occupied[self.index(neighbor.map(|v| v as usize))] != 0
                        {
                            continue;
                        }
                        let mut face = [0; 4];
                        for (slot, offset) in face.iter_mut().zip(offsets) {
                            let key = [x + offset[0], y + offset[1], z + offset[2]];
                            *slot = *corners.entry(key).or_insert_with(|| {
                                let index = mesh.points.len();
                                mesh.points.push(Point {
                                    id: (index + 1) as u64,
                                    position: self.corner(key),
                                });
                                index
                            });
                        }
                        for connectivity in
                            [[face[0], face[1], face[2]], [face[0], face[2], face[3]]]
                        {
                            mesh.cells.push(Cell {
                                id: (mesh.cells.len() + 1) as u64,
                                kind: CellKind::Triangle3,
                                connectivity: connectivity.to_vec(),
                                property_id: None,
                            });
                        }
                    }
                }
            }
        }
        mesh.validate()?;
        Ok(mesh)
    }

    /// Sample a closed triangle/quad surface at voxel centers. Exact coordinate
    /// welding is used for STL facet-local points; open surfaces are rejected.
    pub fn from_surface(mesh: &Mesh, spacing: f64) -> Result<Self> {
        mesh.validate()?;
        if !spacing.is_finite()
            || spacing <= 0.0
            || mesh.cells.is_empty()
            || mesh
                .cells
                .iter()
                .any(|c| !matches!(c.kind, CellKind::Triangle3 | CellKind::Quad4))
        {
            return Err(Error::new(
                "E_VOXEL",
                "voxelization needs positive spacing and a triangle/quad surface",
            ));
        }
        let mut minimum = [f64::INFINITY; 3];
        let mut maximum = [f64::NEG_INFINITY; 3];
        let mut edges = BTreeMap::<([u64; 3], [u64; 3]), usize>::new();
        let mut triangles = Vec::new();
        for cell in &mesh.cells {
            let vertices: Vec<[f64; 3]> = cell
                .connectivity
                .iter()
                .map(|&i| mesh.points[i].position)
                .collect();
            for vertex in &vertices {
                for axis in 0..3 {
                    minimum[axis] = minimum[axis].min(vertex[axis]);
                    maximum[axis] = maximum[axis].max(vertex[axis]);
                }
            }
            for i in 0..vertices.len() {
                let a = vertices[i].map(coordinate_bits);
                let b = vertices[(i + 1) % vertices.len()].map(coordinate_bits);
                let pair = if a < b { (a, b) } else { (b, a) };
                *edges.entry(pair).or_default() += 1;
            }
            triangles.push([vertices[0], vertices[1], vertices[2]]);
            if vertices.len() == 4 {
                triangles.push([vertices[0], vertices[2], vertices[3]]);
            }
        }
        if edges.values().any(|&count| count != 2) {
            return Err(Error::new(
                "E_VOXEL",
                "surface is open or nonmanifold after exact-coordinate welding",
            ));
        }
        let mut dims = [0; 3];
        for axis in 0..3 {
            let n = ((maximum[axis] - minimum[axis]) / spacing).ceil();
            if !n.is_finite() || n > 2_000_000.0 {
                return Err(Error::new("E_VOXEL", "voxel grid exceeds limit"));
            }
            dims[axis] = (n as usize).max(1);
        }
        let count = dims
            .iter()
            .try_fold(1usize, |a, &b| a.checked_mul(b))
            .filter(|&n| n <= 2_000_000)
            .ok_or_else(|| Error::new("E_VOXEL", "voxel grid exceeds 2,000,000 cells"))?;
        if count
            .checked_mul(triangles.len())
            .is_none_or(|work| work > 100_000_000)
        {
            return Err(Error::new(
                "E_VOXEL",
                "voxel sampling exceeds 100 million triangle checks; increase --voxel-size or simplify the surface",
            ));
        }
        let mut grid = Self {
            origin: minimum,
            spacing,
            dims,
            occupied: vec![0; count],
        };
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in 0..dims[0] {
                    let center = std::array::from_fn(|axis| {
                        (0.5_f64).mul_add(spacing, grid.corner([x, y, z])[axis])
                    });
                    let mut inside = false;
                    // Odd intersection count along a generic, fixed direction. Edge
                    // coincidences are retried with two other deterministic directions.
                    let mut resolved = false;
                    for direction in [
                        [1.0, 0.371, 0.529],
                        [0.431, 1.0, 0.613],
                        [0.719, 0.283, 1.0],
                    ] {
                        let mut hits = 0;
                        let mut ambiguous = false;
                        for triangle in &triangles {
                            match ray_triangle(center, direction, *triangle) {
                                Hit::Interior => hits += 1,
                                Hit::Edge => {
                                    ambiguous = true;
                                    break;
                                }
                                Hit::None => {}
                            }
                        }
                        if !ambiguous {
                            inside = hits % 2 == 1;
                            resolved = true;
                            break;
                        }
                    }
                    if !resolved {
                        return Err(Error::new(
                            "E_VOXEL",
                            "ray intersects a surface edge for every sampling direction",
                        ));
                    }
                    if inside {
                        let index = grid.index([x, y, z]);
                        grid.occupied[index] = 1;
                    }
                }
            }
        }
        grid.validate()?;
        if grid.occupied.iter().all(|&value| value == 0) {
            return Err(Error::new(
                "E_VOXEL",
                "no voxel centers fall inside the surface; reduce --voxel-size",
            ));
        }
        Ok(grid)
    }
}

fn coordinate_bits(value: f64) -> u64 {
    if value == 0.0 { 0 } else { value.to_bits() }
}

/// Extract the external faces of linear volume cells, or triangulate a pure
/// triangle/quad surface. Internal faces cancel only when all their corner
/// coordinates match exactly.
pub fn boundary_surface(mesh: &Mesh) -> Result<Mesh> {
    mesh.validate()?;
    let has_volume = mesh.cells.iter().any(|cell| cell.kind.dimension() == 3);
    if !has_volume {
        if mesh
            .cells
            .iter()
            .any(|cell| !matches!(cell.kind, CellKind::Triangle3 | CellKind::Quad4))
        {
            return Err(Error::new(
                "E_MESH",
                "surface extraction requires 2D or 3D cells",
            ));
        }
        let mut output = Mesh {
            points: mesh.points.clone(),
            ..Mesh::default()
        };
        for cell in &mesh.cells {
            let c = &cell.connectivity;
            let faces: Vec<Vec<usize>> = if c.len() == 3 {
                vec![c.clone()]
            } else {
                vec![vec![c[0], c[1], c[2]], vec![c[0], c[2], c[3]]]
            };
            for connectivity in faces {
                output.cells.push(Cell {
                    id: (output.cells.len() + 1) as u64,
                    kind: CellKind::Triangle3,
                    connectivity,
                    property_id: None,
                });
            }
        }
        output.validate()?;
        return Ok(output);
    }
    let mut faces = BTreeMap::<Vec<[u64; 3]>, (Vec<usize>, usize)>::new();
    for cell in &mesh.cells {
        if cell.kind.dimension() != 3 {
            continue;
        }
        let c = &cell.connectivity;
        let local: &[&[usize]] = match cell.kind {
            CellKind::Tet4 => &[&[0, 2, 1], &[0, 1, 3], &[1, 2, 3], &[2, 0, 3]],
            CellKind::Hex8 => &[
                &[0, 3, 2, 1],
                &[4, 5, 6, 7],
                &[0, 1, 5, 4],
                &[1, 2, 6, 5],
                &[2, 3, 7, 6],
                &[3, 0, 4, 7],
            ],
            CellKind::Wedge6 => &[
                &[0, 2, 1],
                &[3, 4, 5],
                &[0, 1, 4, 3],
                &[1, 2, 5, 4],
                &[2, 0, 3, 5],
            ],
            CellKind::Pyramid5 => &[
                &[0, 3, 2, 1],
                &[0, 1, 4],
                &[1, 2, 4],
                &[2, 3, 4],
                &[3, 0, 4],
            ],
            _ => unreachable!(),
        };
        let center: [f64; 3] = std::array::from_fn(|axis| {
            c.iter()
                .map(|&i| mesh.points[i].position[axis])
                .sum::<f64>()
                / c.len() as f64
        });
        for face in local {
            let mut indices: Vec<usize> = face.iter().map(|&i| c[i]).collect();
            // Orient relative to this cell centroid, even when the input cell
            // numbering is inverted. Parity itself does not depend on winding.
            let p = mesh.points[indices[0]].position;
            let q = mesh.points[indices[1]].position;
            let r = mesh.points[indices[2]].position;
            let cross = [
                (q[1] - p[1]) * (r[2] - p[2]) - (q[2] - p[2]) * (r[1] - p[1]),
                (q[2] - p[2]) * (r[0] - p[0]) - (q[0] - p[0]) * (r[2] - p[2]),
                (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]),
            ];
            if cross
                .iter()
                .enumerate()
                .map(|(axis, x)| x * (p[axis] - center[axis]))
                .sum::<f64>()
                < 0.0
            {
                indices.reverse();
            }
            let mut key: Vec<[u64; 3]> = indices
                .iter()
                .map(|&i| mesh.points[i].position.map(coordinate_bits))
                .collect();
            key.sort_unstable();
            let item = faces.entry(key).or_insert_with(|| (indices, 0));
            item.1 += 1;
            if item.1 > 2 {
                return Err(Error::new("E_MESH", "more than two cells share a face"));
            }
        }
    }
    let mut output = Mesh {
        points: mesh.points.clone(),
        ..Mesh::default()
    };
    for (_, (face, count)) in faces {
        if count != 1 {
            continue;
        }
        let triangles = if face.len() == 3 {
            vec![vec![face[0], face[1], face[2]]]
        } else {
            vec![
                vec![face[0], face[1], face[2]],
                vec![face[0], face[2], face[3]],
            ]
        };
        for connectivity in triangles {
            output.cells.push(Cell {
                id: (output.cells.len() + 1) as u64,
                kind: CellKind::Triangle3,
                connectivity,
                property_id: None,
            });
        }
    }
    output.validate()?;
    Ok(output)
}

enum Hit {
    None,
    Interior,
    Edge,
}

fn ray_triangle(origin: [f64; 3], direction: [f64; 3], triangle: [[f64; 3]; 3]) -> Hit {
    let subtract = |a: [f64; 3], b: [f64; 3]| std::array::from_fn(|i| a[i] - b[i]);
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let e1 = subtract(triangle[1], triangle[0]);
    let e2 = subtract(triangle[2], triangle[0]);
    let p = cross(direction, e2);
    let det = dot(e1, p);
    if det.abs() <= 1e-12 {
        return Hit::None;
    }
    let t = subtract(origin, triangle[0]);
    let u = dot(t, p) / det;
    let v = dot(direction, cross(t, e1)) / det;
    if u < -1e-10 || v < -1e-10 || u + v > 1.0 + 1e-10 {
        return Hit::None;
    }
    let distance = dot(e2, cross(t, e1)) / det;
    if distance <= 1e-10 {
        return Hit::None;
    }
    if u <= 1e-10 || v <= 1e-10 || u + v >= 1.0 - 1e-10 {
        Hit::Edge
    } else {
        Hit::Interior
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_cells_share_corners_and_hide_internal_face() {
        let grid = VoxelGrid {
            origin: [0.0; 3],
            spacing: 1.0,
            dims: [2, 1, 1],
            occupied: vec![1, 1],
        };
        let volume = grid.volume_mesh().unwrap();
        assert_eq!((volume.points.len(), volume.cells.len()), (12, 2));
        assert_eq!(grid.surface_mesh().unwrap().cells.len(), 20);
        assert_eq!(boundary_surface(&volume).unwrap().cells.len(), 20);
        assert_eq!(
            VoxelGrid::from_surface(&grid.surface_mesh().unwrap(), 1.0).unwrap(),
            grid
        );
    }

    #[test]
    fn open_surface_and_oversized_grid_fail() {
        let grid = VoxelGrid {
            origin: [0.0; 3],
            spacing: 1.0,
            dims: [1, 1, 1],
            occupied: vec![1],
        };
        let mut surface = grid.surface_mesh().unwrap();
        surface.cells.pop();
        assert_eq!(
            VoxelGrid::from_surface(&surface, 0.5).unwrap_err().code,
            "E_VOXEL"
        );
        assert_eq!(
            VoxelGrid::from_surface(&grid.surface_mesh().unwrap(), 0.001)
                .unwrap_err()
                .code,
            "E_VOXEL"
        );
    }
}
