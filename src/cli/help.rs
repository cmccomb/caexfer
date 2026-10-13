//! Human-facing command help. Keep option descriptions beside the CLI parser.

/// Show the command hierarchy and the shortest useful next step.
pub(super) const OVERVIEW: &str = "caexfer: transfer meshes, voxel occupancy, and results

USAGE
  caexfer --help
  caexfer --version
  caexfer --formats
  caexfer COMMAND [OPTIONS]

COMMANDS
  validate  Check supported data, counts, and omissions
  convert   Write a supported projection to an output file

Run 'caexfer COMMAND --help' for options and examples.

GLOBAL OPTIONS
  -h, --help     Show this help
  -V, --version  Show the version
  -f, --formats  List supported formats and read/write capabilities
";

/// Explain validation and when source omissions make it fail.
const VALIDATE: &str = "caexfer validate: check supported data, counts, and omissions

USAGE
  caexfer validate INPUT [OPTIONS]

INPUT OPTIONS
  --from FORMAT         Override the input extension; see 'caexfer --formats'
  --max-bytes N         Limit input size in bytes (default: 268435456 / 256 MiB)
  --mesh FILE           Companion mesh for OP2/PCH results
  --subcase N           Select an OP2/PCH displacement subcase
  --step N              Select OP2/PCH/Exodus step by zero-based index, or FRD step number

GENERAL OPTIONS
  -s, --strict          Fail when the source has omissions or assumptions
  -j, --json            Return the report as JSON (schema_version=1)
  -h, --help            Show this command's help

EXAMPLES
  caexfer validate model.bdf
  caexfer validate model.bdf --strict --json
  caexfer validate results.op2 --mesh model.bdf --subcase 1
  caexfer validate results.op2 --mesh model.vtu
";

/// Explain conversion in the order users choose source, destination, and receipt.
const CONVERT: &str = "caexfer convert: write a supported projection to an output file

USAGE
  caexfer convert INPUT OUTPUT [OPTIONS]

INPUT OPTIONS
  --from FORMAT              Override the input extension; see 'caexfer --formats'
  --max-bytes N              Limit input size in bytes (default: 268435456 / 256 MiB)
  --mesh FILE                Companion mesh for OP2/PCH results
  --subcase N                Select an OP2/PCH displacement subcase
  --step N                   Select OP2/PCH/Exodus step by zero-based index, or FRD step number

DESTINATION OPTIONS
  --msh-version 2.2|4.1      MSH output dialect (default: 4.1)
  --infill-shape SHAPE       Closed-surface volume cells: tet4|hex8|wedge6|pyramid5
  --infill-unit-size N       Positive target edge/grid size; required with --infill-shape
  --surface                  Extract external triangles from a volume mesh
  --gmsh FILE                Gmsh executable for tet4 infill (default: gmsh on PATH)
  --smooth-iterations N      Smooth VTI/VOX to STL surface (1-50 cycles; default: off)
  --mesh-out FILE            Write a companion mesh alongside OP2 output
  --overwrite                Replace existing regular output files

ACCEPTANCE OPTIONS
  --accept-omissions        Accept reported source and destination losses
  --accept-basic-frame      Assert basic frame with a non-BDF companion mesh
  --accept-zero-rotations   Fill missing OP2 rotations with zero
  --accept-synthetic-zero   Create synthetic zero OP2 from BDF/INP; no solver runs
  --accept-all              Accept all reported changes

GENERAL OPTIONS
  -j, --json                 Return the report as JSON (schema_version=1)
  -h, --help                 Show this command's help

OUTPUT is selected by its extension. Existing files require --overwrite.
Replacement occurs after conversion and acceptance; it does not imply acceptance.
With --mesh-out, --overwrite applies to both output paths. The input file
cannot be replaced. A failure installing the second file can leave the first
output installed.
Conversions with reported losses or assumptions list them and ask for
confirmation. In a noninteractive session, pass the listed acceptance flags.

EXAMPLES
  caexfer convert model.bdf model.vtu
  caexfer convert results.op2 results.vtu --mesh model.bdf
  caexfer convert model.msh model-2.2.msh --msh-version 2.2
  caexfer convert volume.vtu shell.vtu --surface --accept-omissions
  caexfer convert shell.vtu volume.vtu --infill-shape hex8 --infill-unit-size 0.5 --accept-omissions
  caexfer convert shell.vtu tets.vtu --infill-shape tet4 --infill-unit-size 0.5 --accept-omissions
  caexfer convert shell.vtu wedges.vtu --infill-shape wedge6 --infill-unit-size 0.5 --accept-omissions
";

/// Return the help for the selected command; parsing ensures it is recognized.
pub(super) fn for_command(command: &str) -> &'static str {
    match command {
        "validate" => VALIDATE,
        "convert" => CONVERT,
        _ => OVERVIEW,
    }
}
