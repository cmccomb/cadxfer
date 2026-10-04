# Voxel geometry scope (draft for 0.2.0)

This branch covers two workflows: exchange an existing regular voxel grid, and
voxelize supported mesh geometry. Neither workflow infers physical units.

## Existing grids

Read and write VTK XML ImageData (`.vti`) within a documented subset. Retain
the grid extent, origin, spacing, and point-versus-cell association when a
conversion can represent them. Converting a grid to an explicit mesh must use
the correct VTK voxel-to-hexahedron corner order. Reject or report metadata
that the destination cannot retain; do not silently reinterpret a grid as an
unrelated collection of hexahedra.

## Mesh voxelization

Convert supported closed surface or volume geometry to a regular grid using
an explicit voxel size. The output records geometry occupancy on cells, not
interpolated solver results. Report source result fields that the voxelization
does not carry, and reject geometry whose inside and outside cannot be
determined reliably. Bounds, indexing, and boundary treatment must be
deterministic and documented. Check dimensions and allocation size before
building the grid.

## Completion checks

- Verify tiny grids, nonzero origins, nonunit spacing, boundary voxels, and
  invalid or excessive dimensions.
- Verify grid files and voxelized output with an independent VTK reader.
- Check that mesh result values never appear as interpolated voxel values and
  that omissions are visible in conversion reports.
- Update the CLI, library, and support guides only as each route is implemented.
