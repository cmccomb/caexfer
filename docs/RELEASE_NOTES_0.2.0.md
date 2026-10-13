# caexfer 0.2.0 release notes (draft)

Version 0.2.0 adds Rust-native voxel geometry exchange. It reads and writes a
bounded ASCII VTK ImageData (`.vti`) occupancy subset and single-model
MagicaVoxel (`.vox`) files. Closed triangle or quad surfaces and the external
boundary of supported linear volume cells can be sampled at an explicit voxel
size. Occupied voxels can be exported as shared-corner Hex8 cells or as an
exposed triangle surface. An optional smoothing pass softens the STL surface.
Conversion reports identify lost fields, IDs, scale, and palette details.
Mesh voxelization can match an existing VTI grid's origin, spacing, and
dimensions, with a bounds check that rejects clipping. A read-only conversion
preview covers every writable route: ordinary conversions produce exact dry
projection notices, while voxelization estimates grid dimensions and sampler
work before occupancy allocation.

The update also includes the paired OP2/companion-output recovery work and the
Exodus classic-header allocation guard from the stabilization branch. An MSC
Nastran plate fixture is checked against independent mesh and result readers.

Voxel exchange uses binary occupancy rather than solver-result interpolation.
The supported VTI and VOX subsets, mesh expansion limits, and output omissions
are detailed in the [support contract](SUPPORT.md). No physical units are
inferred. Requires Rust 1.86 or newer.
