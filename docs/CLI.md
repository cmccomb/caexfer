# CLI contract

Install the published CLI with `cargo install caexfer`
(Rust 2024 edition, toolchain 1.86+). For a development build, use
`cargo install --git https://github.com/cmccomb/caexfer.git`. In a checkout,
`cargo run -- COMMAND ...` runs the same CLI without installation. All routes
run without Python.

```sh
caexfer --formats
caexfer validate model.bdf --json
caexfer validate results.op2 --mesh model.vtu -j
caexfer convert model.bdf model.vtu
caexfer convert model.bdf model.msh --msh-version 2.2
caexfer convert results.op2 results.vtu --mesh model.bdf
caexfer convert results.pch results.vtu --mesh model.bdf --subcase 1
caexfer convert results.op2 results.vtu --mesh model.msh \
  --accept-basic-frame
caexfer convert results.frd results.op2 --accept-zero-rotations \
  --mesh-out results-mesh.bdf
```

Use fresh output filenames by default. Pass `--overwrite` to replace existing
regular output files after conversion and acceptance. With `--mesh-out`, it
applies to both destinations. For example:

```sh
caexfer convert model.bdf model.vtu --overwrite --accept-omissions
caexfer convert model.bdf results.op2 --mesh-out results-mesh.bdf \
  --overwrite --accept-all
```

STL reads triangle surfaces and writes binary STL. SU2 carries named boundary
markers when the input contains oriented boundary cells and corresponding cell
sets. UNV carries geometry with original node and element labels. Classic
Exodus II carries supported mesh blocks and complete scalar fields over time.
For example:

```sh
caexfer convert surface.stl surface.vtu
caexfer convert named-boundaries.msh named-boundaries.su2
caexfer convert mesh.unv mesh.vtu
caexfer convert results.exo results.vtu --step 0
caexfer convert examples/donut.stl donut.vti --voxel-size 0.25 --accept-omissions
caexfer convert donut.vti donut-roundtrip.stl --accept-omissions
caexfer preview examples/donut.stl donut.vti --voxel-size 0.25
caexfer convert other.stl aligned.vti --match-grid donut.vti --accept-omissions
```

These commands use example input names. The [format limits](SUPPORT.md) describe
which cells, sets, fields, and time data each route can retain.

`caexfer --help` lists the commands and global options. `caexfer -f` and
`caexfer --formats` list format capabilities. Run `caexfer validate` or
`caexfer convert` without paths to see that command's options and examples.
`--help` works after either command too. Paths can occur
before or after options; use `--` for paths beginning with a hyphen. Filenames
use OS-native strings internally; JSON/human display of non-UTF-8 paths is
lossy, not an exact path serialization.

`-j` abbreviates `--json` for both commands; `-s` abbreviates `--strict` for
validation.

| Command or option | Result |
| --- | --- |
| `-f`, `--formats` | Actual read/write capabilities, not a roadmap |
| `validate INPUT` | Supported subset checks, projected counts, and source omissions |
| `convert INPUT OUTPUT` | Project to a writable format; confirm each reported change before installation |
| `preview INPUT OUTPUT` | Read-only conversion plan with counts and notices; voxel routes show grid dimensions and sampling limits |

Input extensions are `.bdf`, `.nas`, `.dat`, `.pch`, `.vtu`, `.vti`, `.vox`, `.vtk`, `.msh`, `.inp`,
`.frd`, `.op2`, `.stl`, `.su2`, `.unv`, `.exo`, `.e`, and `.exodus`
(case-insensitive). `--from FORMAT` overrides the extension;
this is not content autodetection. `.pch` selects the read-only PCH results
adapter. Output extension selects the writer.
MSH input version is detected from `$MeshFormat`; `--msh-version 2.2` selects
2.2 output, while 4.1 remains the default. The option also applies to an MSH
companion written through `--mesh-out`.
`--voxel-size N` supplies a positive cubic cell size when converting a closed
mesh or STL surface to VTI/VOX occupancy, or STL to a Hex8 volume mesh. VTI
and VOX input can be converted to an STL surface or an explicit volume mesh
without this option. `--smooth-iterations N` (1–50) applies only to VTI/VOX
to STL and changes the surface away from exact voxel boundaries. Both routes
report geometry, field, ID, and scale losses that require acceptance. VTI
retains origin and spacing; VOX does not. See [format limits](SUPPORT.md).
`--match-grid FILE` uses an existing supported VTI occupancy grid's origin,
spacing, and dimensions for mesh voxelization. It supplies the voxel size;
omit `--voxel-size`. The surface must fit completely inside the reference
grid. Reference occupancy values are ignored. `caexfer preview` creates no output.
For ordinary conversions it performs the full projection into a discard
stream, giving exact destination notices. For mesh voxelization it stops
before occupancy allocation and sampling; it reports estimated dimensions,
cell count, a conservative maximum triangle-check count, and any applicable
cell, work, or VOX-axis limit. That estimate does not establish watertightness
or exact destination losses. A nonzero `limit` in JSON means the requested
voxelization cannot run at that resolution.

`--mesh FILE` supplies geometry when reading OP2 or PCH. The companion must
carry original node IDs that match the displacement table; generated one-based
IDs are rejected. BDF input verifies `GRID CD=0`. Other formats do not carry
that check. Validation reports the basic-frame assumption without accepting
it; `--strict` fails on that notice. Conversion requires confirmation or
`--accept-basic-frame` to assert that mesh coordinates and result displacements
use the basic frame. Any fields in the companion file are ignored.
OP2 output carries one displacement table and needs a separately retained
matching mesh. `--mesh-out FILE` optionally writes a geometry-only companion
in BDF, VTU, VTK, MSH, INP, or FRD format. `--subcase N` and
zero-based `--step N` select an OP2 or PCH displacement result. For FRD, `--step N` selects the
step number. These selection options also work with `validate`.
For Exodus input, `--step N` selects a zero-based time step. Without it, the
reader carries all complete scalar fields and times that the destination can
represent.
When writing OP2 from three-component displacement, the CLI proposes zero for
the absent R1/R2/R3 values. Confirm the proposal or pass
`--accept-zero-rotations` only when they are known zero. When writing OP2 from
a BDF or INP mesh without results, the CLI proposes a synthetic static table
with six float zeros per node. Confirm it or pass `--accept-synthetic-zero`.
The OP2 title labels the result as synthetic; no solver runs.
For INP, retain the source mesh or use `--mesh-out` to export a companion.

For each reported change, the prompt names its acceptance flag.
`--accept-omissions` covers source and destination losses. Specific assumption
flags cover the basic frame, zero rotations, and synthetic zero results.
`--accept-all` covers every listed change. If stdin
is not a terminal, or `--json` is selected, an unaccepted change fails before
output installation and reports the needed flags. A conversion with no reported
changes proceeds without confirmation.

Approval applies only after the source can be projected into valid geometry.
For example, `examples/nonbasic-frame.bdf` has a GRID in `CP=42` and cannot
be converted: caexfer does not resolve its `CORD2R` frame. Resolve the GRID
coordinates into the basic frame with a trusted BDF preprocessor first.
`--accept-all` does not override source errors or unsupported topology. To see
the approval prompt, run `caexfer convert examples/plate.bdf model.vtu` from
the repository root in a terminal, using a new output filename.

For `validate` and `convert`, `--json` returns the report on stdout as JSON with
schema_version=1. Human-readable conversion omission reports go to stderr;
JSON conversion reports place them in an `omissions` array with each item's
`stage` (`source`, `destination`, or `assumption`), `acceptance_flag`, and detail. A paired export
adds `mesh_output` with its path, format, and separate omissions. VTI/VOX output
adds `voxel_grid` with dimensions and occupied cell count. JSON error
output also goes to stdout. Schema version 1 is intentionally small; consumers
should tolerate new keys.

Exit 0 means the requested scoped operation succeeded. `validate` reports
projected counts, omissions, and assumptions; without `--strict`, notices do
not fail it. Exit 1 means I/O, parsing, projection, or validation failed.
`validate --strict` also exits 1 on an omission or assumption. Exit 2 means
invalid CLI usage, including an unattended conversion that needs acceptance.
Use stable diagnostic `code` values rather than parsing human descriptions.
The version's code list can grow.

The CLI refuses an existing output path unless `convert --overwrite` is given.
With `--mesh-out`, the flag applies to both output paths. It does not accept
omissions or assumptions. The CLI stages and checks both files, obtains any
required acceptance, then replaces destinations with same-filesystem renames.
It refuses symbolic links, nonregular destinations, and output paths that
refer to the input file (including hard links on Unix). The public
`caexfer::convert` operation retains
its staged, no-clobber installation policy. CLI staging requires a filesystem
supporting hard links, including when `--overwrite` is used.
For paired output, the CLI stages both files and rechecks both destinations
before installing either. Two different filenames cannot be committed
atomically: if installing the second fails after the OP2 is installed, the
error names the OP2 file that remains or was replaced and any retained recovery
files. The CLI keeps the staged companion and hard links to previous outputs in
private sibling directories. Inspect the current destinations before restoring
or installing those files; concurrent in-place writes can change a hard-linked
previous file.
