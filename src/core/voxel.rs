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

// A bounded occupancy array can still expand into millions of heap-backed
// points, cells, and map entries when projected as an explicit mesh.
const MAX_MESH_VOXELS: usize = 250_000;
const MAX_EXPOSED_FACES: usize = 250_000;

/// A cell-centered, axis-aligned occupancy grid. Coordinates have the source's
/// units; `origin` is the minimum corner of cell `[0, 0, 0]`.
#[derive(Debug, Clone, PartialEq)]
pub struct VoxelGrid {
    pub origin: [f64; 3],
    pub spacing: f64,
    pub dims: [usize; 3],
    pub occupied: Vec<u8>,
}

/// Planned grid geometry and the conservative sampling work bound.
#[derive(Debug, Clone, PartialEq)]
pub struct VoxelEstimate {
    /// Physical lower corner of cell zero.
    pub origin: [f64; 3],
    /// Cubic cell edge length.
    pub spacing: f64,
    /// Number of cells along each axis.
    pub dims: [usize; 3],
    /// Total planned cells before the occupancy allocation.
    pub cells: u128,
    /// Conservative upper bound for ray-triangle tests.
    pub triangle_checks: u128,
}

impl VoxelEstimate {
    /// Apply the current allocation and sampler limits after displaying a plan.
    ///
    /// # Errors
    /// Returns `E_VOXEL` when the grid exceeds the cell or sampler work limit.
    pub fn check_limits(&self) -> Result<()> {
        if self.cells > 2_000_000 {
            return Err(Error::new("E_VOXEL", "voxel grid exceeds 2,000,000 cells"));
        }
        if self.triangle_checks > 100_000_000 {
            return Err(Error::new(
                "E_VOXEL",
                "voxel sampling exceeds 100 million triangle checks; increase --infill-unit-size or simplify the surface",
            ));
        }
        Ok(())
    }
}

/// Smooth a shared-vertex triangle surface with paired Laplacian steps.
/// The negative pass offsets some of the shrinkage of ordinary Laplacian
/// smoothing. Connectivity is unchanged, but geometric fidelity to the grid
/// is deliberately reduced.
pub(crate) fn smooth_surface(mesh: &mut Mesh, iterations: usize) -> Result<()> {
    mesh.validate()?;
    if iterations > 50
        || mesh
            .cells
            .iter()
            .any(|cell| cell.kind != CellKind::Triangle3)
    {
        return Err(Error::new(
            "E_VOXEL",
            "smoothing requires a triangle surface and at most 50 cycles",
        ));
    }
    let mut neighbors = vec![Vec::new(); mesh.points.len()];
    for cell in &mesh.cells {
        let [a, b, c] = [
            cell.connectivity[0],
            cell.connectivity[1],
            cell.connectivity[2],
        ];
        for (u, v) in [(a, b), (b, c), (c, a)] {
            neighbors[u].push(v);
            neighbors[v].push(u);
        }
    }
    for adjacent in &mut neighbors {
        adjacent.sort_unstable();
        adjacent.dedup();
    }
    let mut positions: Vec<[f64; 3]> = mesh.points.iter().map(|point| point.position).collect();
    for _ in 0..iterations {
        // Taubin-style two-pass smoothing: the second pass expands slightly
        // to counter the first pass's tendency to shrink closed objects.
        for weight in [0.5_f64, -0.53_f64] {
            let mut next = positions.clone();
            for (index, adjacent) in neighbors.iter().enumerate() {
                if adjacent.is_empty() {
                    continue;
                }
                for axis in 0..3 {
                    let mean = adjacent.iter().map(|&v| positions[v][axis]).sum::<f64>()
                        / adjacent.len() as f64;
                    next[index][axis] =
                        weight.mul_add(mean - positions[index][axis], positions[index][axis]);
                }
            }
            positions = next;
        }
    }
    for (point, position) in mesh.points.iter_mut().zip(positions) {
        point.position = position;
    }
    mesh.validate()
}

impl VoxelGrid {
    /// Estimate a surface grid without allocating occupancy or casting rays.
    pub fn estimate_surface(
        mesh: &Mesh,
        spacing: f64,
        reference: Option<&Self>,
    ) -> Result<VoxelEstimate> {
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
        for cell in &mesh.cells {
            for &index in &cell.connectivity {
                let position = mesh.points[index].position;
                for axis in 0..3 {
                    minimum[axis] = minimum[axis].min(position[axis]);
                    maximum[axis] = maximum[axis].max(position[axis]);
                }
            }
        }
        let (origin, dims) = if let Some(grid) = reference {
            grid.validate()?;
            if grid.spacing.to_bits() != spacing.to_bits() {
                return Err(Error::new(
                    "E_VOXEL",
                    "reference grid spacing differs from requested voxel size",
                ));
            }
            for axis in 0..3 {
                let end = (grid.dims[axis] as f64).mul_add(spacing, grid.origin[axis]);
                if minimum[axis] < grid.origin[axis] || maximum[axis] > end {
                    return Err(Error::new(
                        "E_VOXEL",
                        "surface extends outside reference grid bounds",
                    ));
                }
            }
            (grid.origin, grid.dims)
        } else {
            let mut dims = [0; 3];
            for axis in 0..3 {
                let n = ((maximum[axis] - minimum[axis]) / spacing).ceil();
                if !n.is_finite() || n > 2_000_000.0 {
                    return Err(Error::new("E_VOXEL", "voxel grid exceeds limit"));
                }
                dims[axis] = (n as usize).max(1);
            }
            (minimum, dims)
        };
        let cells = dims.iter().fold(1u128, |a, &b| a.saturating_mul(b as u128));
        let triangles = mesh
            .cells
            .iter()
            .map(|c| if c.kind == CellKind::Quad4 { 2 } else { 1 })
            .sum::<usize>();
        let triangle_checks = cells.saturating_mul(triangles as u128).saturating_mul(3);
        Ok(VoxelEstimate {
            origin,
            spacing,
            dims,
            cells,
            triangle_checks,
        })
    }
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
        if self.occupied.iter().filter(|&&value| value != 0).count() > MAX_MESH_VOXELS {
            return Err(Error::new(
                "E_VOXEL",
                "voxel volume exceeds the 250,000-cell explicit mesh limit",
            ));
        }
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

    /// Emit conforming Hex8, Wedge6, or Pyramid5 cells from occupied grid cells.
    /// Wedges split each cube along the same vertical diagonal; pyramids meet
    /// at one new cube-center point. The outside boundary remains voxelized.
    pub(crate) fn volume_mesh_as(&self, kind: CellKind) -> Result<Mesh> {
        let factor = match kind {
            CellKind::Hex8 => 1,
            CellKind::Wedge6 => 2,
            CellKind::Pyramid5 => 6,
            _ => return Err(Error::new("E_VOXEL", "unsupported grid infill cell shape")),
        };
        self.validate()?;
        if self.occupied.iter().filter(|&&value| value != 0).count() > MAX_MESH_VOXELS / factor {
            return Err(Error::new(
                "E_VOXEL",
                "infill exceeds the 250,000-cell explicit mesh limit",
            ));
        }
        let mut mesh = self.volume_mesh()?;
        if kind == CellKind::Hex8 {
            return Ok(mesh);
        }
        let hexes = std::mem::take(&mut mesh.cells);
        for hex in hexes {
            let c = hex.connectivity;
            if kind == CellKind::Wedge6 {
                for local in [[0, 1, 2, 4, 5, 6], [0, 2, 3, 4, 6, 7]] {
                    mesh.cells.push(Cell {
                        id: (mesh.cells.len() + 1) as u64,
                        kind,
                        connectivity: local.map(|index| c[index]).to_vec(),
                        property_id: None,
                    });
                }
            } else {
                let center = std::array::from_fn(|axis| {
                    c.iter()
                        .map(|&index| mesh.points[index].position[axis])
                        .sum::<f64>()
                        / 8.0
                });
                let apex = mesh.points.len();
                mesh.points.push(Point {
                    id: (apex + 1) as u64,
                    position: center,
                });
                for base in [
                    [0, 1, 2, 3],
                    [4, 7, 6, 5],
                    [0, 4, 5, 1],
                    [1, 5, 6, 2],
                    [2, 6, 7, 3],
                    [3, 7, 4, 0],
                ] {
                    let mut connectivity: Vec<usize> = base.map(|index| c[index]).to_vec();
                    connectivity.push(apex);
                    mesh.cells.push(Cell {
                        id: (mesh.cells.len() + 1) as u64,
                        kind,
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
        let mut exposed = 0usize;
        for z in 0..self.dims[2] {
            for y in 0..self.dims[1] {
                for x in 0..self.dims[0] {
                    if self.occupied[self.index([x, y, z])] == 0 {
                        continue;
                    }
                    for direction in [
                        [-1, 0, 0],
                        [1, 0, 0],
                        [0, -1, 0],
                        [0, 1, 0],
                        [0, 0, -1],
                        [0, 0, 1],
                    ] {
                        let neighbor = [
                            x as isize + direction[0],
                            y as isize + direction[1],
                            z as isize + direction[2],
                        ];
                        if !neighbor
                            .iter()
                            .enumerate()
                            .all(|(axis, &v)| v >= 0 && v < self.dims[axis] as isize)
                            || self.occupied[self.index(neighbor.map(|v| v as usize))] == 0
                        {
                            exposed += 1;
                            if exposed > MAX_EXPOSED_FACES {
                                return Err(Error::new(
                                    "E_VOXEL",
                                    "voxel surface exceeds the 250,000-face explicit mesh limit",
                                ));
                            }
                        }
                    }
                }
            }
        }
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
        Self::from_surface_with_reference(mesh, spacing, None)
    }

    /// Sample a closed surface on its own grid or an existing grid's geometry.
    pub fn from_surface_with_reference(
        mesh: &Mesh,
        spacing: f64,
        reference: Option<&Self>,
    ) -> Result<Self> {
        let estimate = Self::estimate_surface(mesh, spacing, reference)?;
        estimate.check_limits()?;
        let mut edges = BTreeMap::<([u64; 3], [u64; 3]), usize>::new();
        let mut triangles = Vec::new();
        for cell in &mesh.cells {
            let vertices: Vec<[f64; 3]> = cell
                .connectivity
                .iter()
                .map(|&i| mesh.points[i].position)
                .collect();
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
        if triangles
            .iter()
            .any(|triangle| triangle_is_degenerate(*triangle))
        {
            return Err(Error::new(
                "E_VOXEL",
                "surface contains a zero-area or nonfinite triangle",
            ));
        }
        let mut grid = Self {
            origin: estimate.origin,
            spacing,
            dims: estimate.dims,
            occupied: vec![0; estimate.cells as usize],
        };
        let dims = estimate.dims;
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in 0..dims[0] {
                    let center = std::array::from_fn(|axis| {
                        (0.5_f64).mul_add(spacing, grid.corner([x, y, z])[axis])
                    });
                    let mut inside = None;
                    // Compare three deterministic directions; discard edge hits,
                    // and reject any conflicting unambiguous parity results.
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
                            let result = hits % 2 == 1;
                            if inside.is_some_and(|previous| previous != result) {
                                return Err(Error::new(
                                    "E_VOXEL",
                                    "surface rays disagree about inside/outside at a voxel center",
                                ));
                            }
                            inside = Some(result);
                        }
                    }
                    let Some(inside) = inside else {
                        return Err(Error::new(
                            "E_VOXEL",
                            "ray intersects a surface edge for every sampling direction",
                        ));
                    };
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
                "no voxel centers fall inside the surface; reduce --infill-unit-size",
            ));
        }
        Ok(grid)
    }
}

fn coordinate_bits(value: f64) -> u64 {
    if value == 0.0 { 0 } else { value.to_bits() }
}

fn triangle_is_degenerate(triangle: [[f64; 3]; 3]) -> bool {
    let u = std::array::from_fn::<_, 3, _>(|axis| triangle[1][axis] - triangle[0][axis]);
    let v = std::array::from_fn::<_, 3, _>(|axis| triangle[2][axis] - triangle[0][axis]);
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    cross.iter().any(|value| !value.is_finite()) || cross.iter().all(|&value| value == 0.0)
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
    let dot_product = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let e1 = subtract(triangle[1], triangle[0]);
    let e2 = subtract(triangle[2], triangle[0]);
    let p = cross(direction, e2);
    let det = dot_product(e1, p);
    if det.abs() <= 1e-12 {
        return Hit::None;
    }
    let t = subtract(origin, triangle[0]);
    let u = dot_product(t, p) / det;
    let v = dot_product(direction, cross(t, e1)) / det;
    if u < -1e-10 || v < -1e-10 || u + v > 1.0 + 1e-10 {
        return Hit::None;
    }
    let distance = dot_product(e2, cross(t, e1)) / det;
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
            VoxelGrid::from_surface(&boundary_surface(&volume).unwrap(), 1.0).unwrap(),
            grid
        );
        assert_eq!(
            VoxelGrid::from_surface(&grid.surface_mesh().unwrap(), 1.0).unwrap(),
            grid
        );
    }

    #[test]
    fn reference_grid_aligns_sampling_and_rejects_clipping() {
        let cube = VoxelGrid {
            origin: [0.0; 3],
            spacing: 1.0,
            dims: [1, 1, 1],
            occupied: vec![1],
        };
        let surface = cube.surface_mesh().unwrap();
        let reference = VoxelGrid {
            origin: [-1.0; 3],
            spacing: 1.0,
            dims: [3, 3, 3],
            occupied: vec![0; 27],
        };
        let estimate = VoxelGrid::estimate_surface(&surface, 1.0, Some(&reference)).unwrap();
        assert_eq!(estimate.dims, [3, 3, 3]);
        assert_eq!(estimate.cells, 27);
        let sampled =
            VoxelGrid::from_surface_with_reference(&surface, 1.0, Some(&reference)).unwrap();
        assert_eq!(
            sampled.origin.map(f64::to_bits),
            reference.origin.map(f64::to_bits)
        );
        assert_eq!(
            sampled
                .occupied
                .iter()
                .map(|&v| usize::from(v))
                .sum::<usize>(),
            1
        );
        assert_eq!(sampled.occupied[13], 1);

        let clipped = VoxelGrid {
            origin: [0.25, -1.0, -1.0],
            ..reference
        };
        assert!(
            VoxelGrid::estimate_surface(&surface, 1.0, Some(&clipped))
                .unwrap_err()
                .message
                .contains("outside reference grid")
        );
    }

    #[test]
    fn oversized_estimate_is_visible_before_sampling() {
        let cube = VoxelGrid {
            origin: [0.0; 3],
            spacing: 1.0,
            dims: [1, 1, 1],
            occupied: vec![1],
        };
        let estimate =
            VoxelGrid::estimate_surface(&cube.surface_mesh().unwrap(), 0.005, None).unwrap();
        assert_eq!(estimate.dims, [200, 200, 200]);
        assert_eq!(estimate.cells, 8_000_000);
        assert!(estimate.check_limits().is_err());
    }

    #[test]
    fn grid_infill_shapes_keep_the_same_closed_outer_boundary() {
        let grid = VoxelGrid {
            origin: [0.0; 3],
            spacing: 1.0,
            dims: [2, 1, 1],
            occupied: vec![1, 1],
        };
        for (kind, cells) in [
            (CellKind::Hex8, 2),
            (CellKind::Wedge6, 4),
            (CellKind::Pyramid5, 12),
        ] {
            let mesh = grid.volume_mesh_as(kind).unwrap();
            assert_eq!(mesh.cells.len(), cells);
            assert!(mesh.cells.iter().all(|cell| cell.kind == kind));
            let shell = boundary_surface(&mesh).unwrap();
            assert_eq!(shell.cells.len(), 20);
            assert_eq!(VoxelGrid::from_surface(&shell, 1.0).unwrap(), grid);
        }
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

    #[test]
    fn rejects_invalid_grid_bounds_and_keeps_empty_meshes_empty() {
        let empty = VoxelGrid {
            origin: [0.0; 3],
            spacing: 1.0,
            dims: [2, 1, 1],
            occupied: vec![0, 0],
        };
        assert_eq!(empty.volume_mesh().unwrap().cells.len(), 0);
        assert_eq!(empty.surface_mesh().unwrap().cells.len(), 0);
        let mut invalid = empty.clone();
        invalid.spacing = 0.0;
        assert_eq!(invalid.validate().unwrap_err().code, "E_VOXEL");
        invalid = empty.clone();
        invalid.origin[0] = f64::NAN;
        assert_eq!(invalid.validate().unwrap_err().code, "E_VOXEL");
        invalid = empty.clone();
        invalid.dims[0] = 0;
        assert_eq!(invalid.validate().unwrap_err().code, "E_VOXEL");
        invalid = empty.clone();
        invalid.occupied[0] = 2;
        assert_eq!(invalid.validate().unwrap_err().code, "E_VOXEL");
        invalid = empty;
        invalid.origin[0] = f64::MAX;
        invalid.spacing = f64::MAX;
        assert_eq!(invalid.validate().unwrap_err().code, "E_VOXEL");
    }

    #[test]
    fn explicit_mesh_limits_reject_large_expansions_before_allocating() {
        let filled = VoxelGrid {
            origin: [0.0; 3],
            spacing: 1.0,
            dims: [MAX_MESH_VOXELS + 1, 1, 1],
            occupied: vec![1; MAX_MESH_VOXELS + 1],
        };
        assert!(
            filled
                .volume_mesh()
                .unwrap_err()
                .message
                .contains("explicit mesh limit")
        );

        let mut alternating = filled;
        for (index, value) in alternating.occupied.iter_mut().enumerate() {
            *value = u8::from(index % 2 == 0);
        }
        assert!(
            alternating
                .surface_mesh()
                .unwrap_err()
                .message
                .contains("explicit mesh limit")
        );
        assert!(triangle_is_degenerate([
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0]
        ]));
    }

    #[test]
    fn closed_quad_surface_round_trips_through_voxel_sampling() {
        let grid = VoxelGrid {
            origin: [0.0; 3],
            spacing: 1.0,
            dims: [1, 1, 1],
            occupied: vec![1],
        };
        let mut quad_mesh = grid.volume_mesh().unwrap();
        quad_mesh.cells = [
            [0, 3, 2, 1],
            [4, 5, 6, 7],
            [0, 1, 5, 4],
            [1, 2, 6, 5],
            [2, 3, 7, 6],
            [3, 0, 4, 7],
        ]
        .into_iter()
        .enumerate()
        .map(|(index, face)| Cell {
            id: (index + 1) as u64,
            kind: CellKind::Quad4,
            connectivity: face.to_vec(),
            property_id: None,
        })
        .collect();
        assert_eq!(boundary_surface(&quad_mesh).unwrap().cells.len(), 12);
        assert_eq!(VoxelGrid::from_surface(&quad_mesh, 1.0).unwrap(), grid);

        let mut inverted = grid.volume_mesh().unwrap();
        inverted.cells[0].connectivity = vec![3, 2, 1, 0, 7, 6, 5, 4];
        assert_eq!(boundary_surface(&inverted).unwrap().cells.len(), 12);
        let mut nonmanifold = grid.volume_mesh().unwrap();
        for id in 2..=3 {
            let mut extra = nonmanifold.cells[0].clone();
            extra.id = id;
            nonmanifold.cells.push(extra);
        }
        assert!(
            boundary_surface(&nonmanifold)
                .unwrap_err()
                .message
                .contains("more than two")
        );
    }
}
