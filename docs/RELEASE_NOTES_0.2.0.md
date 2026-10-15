# caexfer 0.2.0 release notes (draft)

Version 0.2.0 adds Rust-native voxel geometry exchange. It reads and writes a
bounded ASCII VTK ImageData (`.vti`) occupancy subset and single-model
MagicaVoxel (`.vox`) files. Closed triangle or quad surfaces and the external
boundary of supported linear volume cells can be sampled at an explicit voxel
size. Occupied voxels can be exported as shared-corner Hex8 cells or as an
exposed triangle surface. An optional smoothing pass softens the STL surface.
Conversion reports identify lost fields, IDs, scale, and palette details.
Mesh voxelization can match an existing VTI grid's origin, spacing, and
dimensions, with a bounds check that rejects clipping. `caexfer inspect INPUT`
reports source counts and notices. The read-only
`caexfer convert INPUT OUTPUT --preview` covers every writable route:
ordinary conversions produce exact dry
projection notices, while voxelization estimates grid dimensions and sampler
work before occupancy allocation.

Closed surfaces can also become surface-conforming Tet4 volumes through Gmsh,
or voxelized Hex8, Wedge6, and Pyramid5 volumes. The CLI and library use one
paired size interface, `--infill-shape` and `--infill-unit-size`, for these
routes and for mesh-to-VTI/VOX sampling. `--surface` extracts the external
triangle shell from linear volume meshes. Measured donut and cube round trips
are documented in the README. The older voxel and tetrahedralization size
flags are removed.

The CLI now uses `caexfer formats` for the capability listing. The former
`-f` and `--formats` switches are removed.

This branch builds on the published v0.1.2 maintenance release, including its
stricter INP parsing, paired-output recovery, and expanded regression suite.
The Exodus classic-header allocation guard and MSC Nastran plate fixture remain
covered by independent reader checks.

Voxel exchange uses binary occupancy rather than solver-result interpolation.
The supported VTI and VOX subsets, mesh expansion limits, and output omissions
are detailed in the [support contract](SUPPORT.md). No physical units are
inferred. Requires Rust 1.86 or newer.
