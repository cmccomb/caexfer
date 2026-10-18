//! Human-facing command help. Keep option descriptions beside the CLI parser.

/// Show the command hierarchy and the shortest useful next step.
pub(super) const OVERVIEW: &str = "caexfer: transfer meshes, voxel occupancy, and results

USAGE
  caexfer --help
  caexfer --version
  caexfer formats
  caexfer COMMAND [OPTIONS]

COMMANDS
  formats  List supported formats and read/write capabilities
  inspect  Check source data and report counts and notices
  convert  Write a supported projection, or preview it without writing

Run 'caexfer COMMAND --help' for options and examples.

GLOBAL OPTIONS
  -h, --help     Show this help
  -V, --version  Show the version
";

/// Explain the zero-argument format capability listing.
const FORMATS: &str = "caexfer formats: list supported formats and read/write capabilities

USAGE
  caexfer formats
";

/// Explain validation and when source omissions make it fail.
const INSPECT: &str = "caexfer inspect: check one source file

USAGE
  caexfer inspect INPUT [OPTIONS]

INPUT OPTIONS
  --from FORMAT         Override the input extension; see 'caexfer formats'
  --max-bytes N         Limit input size in bytes (default: 268435456 / 256 MiB)
  --mesh FILE           Companion mesh for OP2/PCH results
  --subcase N           Select an OP2/PCH displacement subcase
  --step N              Select OP2/PCH/Exodus step by zero-based index, or FRD step number

GENERAL OPTIONS
  -s, --strict          Fail when the source has omissions or assumptions
  -j, --json            Return the report as JSON (schema_version=1)
  -h, --help            Show this command's help

Reads the supported source subset and reports its counts and notices. It does
not check whether any particular output format can represent that data. Use
'caexfer convert INPUT OUTPUT --preview' to check a proposed conversion.

EXAMPLES
  caexfer inspect model.bdf
  caexfer inspect model.bdf --strict --json
  caexfer inspect results.op2 --mesh model.bdf --subcase 1
  caexfer inspect results.op2 --mesh model.vtu
";

/// Explain conversion in the order users choose source, destination, and receipt.
const CONVERT: &str = "caexfer convert: write or preview a supported projection

USAGE
  caexfer convert INPUT OUTPUT [OPTIONS]

INPUT OPTIONS
  --from FORMAT              Override the input extension; see 'caexfer formats'
  --max-bytes N              Limit input size in bytes (default: 268435456 / 256 MiB)
  --mesh FILE                Companion mesh for OP2/PCH results
  --subcase N                Select an OP2/PCH displacement subcase
  --step N                   Select OP2/PCH/Exodus step by zero-based index, or FRD step number

DESTINATION OPTIONS
  --msh-version 2.2|4.1      MSH output dialect (default: 4.1)
  --infill-shape SHAPE       Closed-surface volume cells: tet4|hex8|wedge6|pyramid5
  --infill-unit-size N       Positive target edge/grid size; required with --infill-shape
  --match-grid FILE          Sample on an existing VTI grid using hex8 infill
  --surface                  Extract external triangles from a volume mesh
  --gmsh FILE                Gmsh executable for tet4 infill (default: gmsh on PATH)
  --smooth-iterations N      Smooth VTI/VOX to STL surface (1-50 cycles; default: off)
  --mesh-out FILE            Write or preview an OP2 companion mesh projection
  --overwrite                Replace existing regular output files when writing

ACCEPTANCE OPTIONS
  --accept-omissions        Accept reported source and destination losses
  --accept-basic-frame      Assert basic frame with a non-BDF companion mesh
  --accept-zero-rotations   Fill missing OP2 rotations with zero
  --accept-synthetic-zero   Create synthetic zero OP2 from BDF/INP; no solver runs
  --accept-all              Accept all reported changes

GENERAL OPTIONS
  --preview                  Report the proposed conversion without writing output
  -j, --json                 Return the report as JSON (schema_version=1)
  -h, --help                 Show this command's help

OUTPUT is selected by its extension. --preview uses the same route options and
never creates or replaces output files. Most routes run the destination writer
into a discard stream, so destination notices are exact and preview may take as
long as conversion. Grid infill reports size and sampler work before sampling;
complete=false means destination effects remain unchecked. Acceptance flags and
--overwrite cannot be combined with --preview.

Existing files require --overwrite when writing.
Replacement occurs after conversion and acceptance; it does not imply acceptance.
With --mesh-out, --overwrite applies to both output paths. The input file
cannot be replaced. A failure installing the second file can leave the first
output installed.
Conversions with reported losses or assumptions list them and ask for
confirmation. In a noninteractive session, pass the listed acceptance flags.

EXAMPLES
  caexfer convert model.bdf model.vtu
  caexfer convert model.bdf model.vtu --preview
  caexfer convert examples/donut.stl donut.vti --infill-shape hex8 --infill-unit-size 0.25 --preview
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
        "formats" => FORMATS,
        "inspect" => INSPECT,
        "convert" => CONVERT,
        _ => OVERVIEW,
    }
}
