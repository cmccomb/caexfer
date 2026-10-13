//! Read-only conversion preview with a fast voxel work estimate.

use std::io::sink;
use std::path::Path;

use crate::conversion::{self, ConversionReport, Format, InfillShape, Omission, Options};
use crate::core::{Error, Result, VoxelEstimate, VoxelGrid, boundary_surface};

use super::convert::propose_synthetic_zero;

/// Source counts and either a full dry projection or a pre-sampling voxel plan.
/// A voxel plan includes source notices; destination notices require conversion.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewReport {
    /// Source file format.
    pub source_format: Format,
    /// Requested destination format.
    pub target_format: Format,
    /// Source file size in bytes.
    pub source_bytes: u64,
    /// Projected source point count.
    pub points: usize,
    /// Projected source cell count.
    pub cells: usize,
    /// Projected source numeric field count.
    pub fields: usize,
    /// Source notices, plus destination notices when `complete` is true.
    pub omissions: Vec<Omission>,
    /// Planned voxel geometry and conservative ray-triangle test count.
    pub voxel_estimate: Option<VoxelEstimate>,
    /// Whether the destination writer completed a full dry projection.
    /// False means only grid size and sampling work were estimated; output
    /// representability and destination omissions remain unchecked.
    pub complete: bool,
    /// A limit or output-format reason that prevents the planned voxelization.
    pub limit: Option<String>,
    /// Optional companion output preview for OP2 conversions.
    pub mesh_output: Option<ConversionReport>,
}

/// Preview a conversion without creating or replacing an output file.
///
/// Mesh voxelization stops after grid and work estimation, before allocating
/// occupancy or sampling rays. Other routes run their writer into a discard
/// stream, so their destination omissions and representability checks are exact.
/// Acceptance flags are not needed for preview.
///
/// # Errors
/// Returns source, option, or destination projection errors. Voxel allocation
/// and sampler limits are reported in `PreviewReport.limit` instead.
#[allow(clippy::too_many_lines)] // Source preparation and both preview modes share one report.
pub fn preview(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    options: &Options,
) -> Result<PreviewReport> {
    let input = input.as_ref();
    let output = output.as_ref();
    let target = Format::from_output_path(output)?;
    if options.mesh_output.is_some() && target != Format::Op2 {
        return Err(Error::new(
            "E_USAGE",
            "mesh_output applies only to OP2 output",
        ));
    }
    if options.match_grid.is_some() && options.infill_shape.is_some() {
        return Err(Error::new(
            "E_USAGE",
            "match_grid supplies hex8 infill size; omit infill flags",
        ));
    }
    let source_format = options
        .input_format
        .map_or_else(|| Format::from_input_path(input), Ok)?;
    let mut engine_options = options.clone();
    if matches!(source_format, Format::Op2 | Format::Pch)
        && options
            .mesh
            .as_ref()
            .is_some_and(|path| Format::from_input_path(path).is_ok_and(|f| f != Format::Bdf))
    {
        engine_options.assume_basic_frame = true;
    }
    if target == Format::Op2 {
        engine_options.zero_missing_rotations = true;
    }
    let mut source = conversion::read_path(input, &engine_options)?;
    if target == Format::Op2
        && matches!(source.format, Format::Bdf | Format::Inp)
        && source.dataset.fields.is_empty()
    {
        propose_synthetic_zero(input, &mut source, options.max_bytes)?;
    }
    let mut result = PreviewReport {
        source_format: source.format,
        target_format: target,
        source_bytes: std::fs::metadata(input)?.len(),
        points: source.dataset.mesh.points.len(),
        cells: source.dataset.mesh.cells.len(),
        fields: source.dataset.fields.len(),
        omissions: source.omissions.clone(),
        voxel_estimate: None,
        complete: false,
        limit: None,
        mesh_output: None,
    };
    let mesh_to_grid = matches!(target, Format::Vti | Format::Vox) && source.voxel_grid.is_none();
    let surface_to_grid_volume = source
        .dataset
        .mesh
        .cells
        .iter()
        .all(|cell| cell.kind.dimension() == 2)
        && !source.dataset.mesh.cells.is_empty()
        && !matches!(
            target,
            Format::Stl | Format::Vti | Format::Vox | Format::Op2 | Format::Pch
        )
        && (matches!(
            options.infill_shape,
            Some(InfillShape::Hex8 | InfillShape::Wedge6 | InfillShape::Pyramid5)
        ) || options.match_grid.is_some());
    if options.match_grid.is_some() && !(mesh_to_grid || surface_to_grid_volume) {
        return Err(Error::new(
            "E_USAGE",
            "--match-grid applies to mesh voxelization only",
        ));
    }
    if (mesh_to_grid || surface_to_grid_volume)
        && (options.infill_shape == Some(InfillShape::Hex8)
            || surface_to_grid_volume
            || options.match_grid.is_some())
    {
        let reference = conversion::load_match_grid(options)?;
        let spacing = options
            .infill_unit_size
            .or_else(|| reference.as_ref().map(|grid| grid.spacing))
            .ok_or_else(|| Error::new("E_USAGE", "missing infill unit size"))?;
        let mesh = &source.dataset.mesh;
        let surface = if mesh.cells.iter().any(|cell| cell.kind.dimension() == 3) {
            boundary_surface(mesh)?
        } else {
            mesh.clone()
        };
        let estimate = VoxelGrid::estimate_surface(&surface, spacing, reference.as_ref())?;
        result.limit = estimate.check_limits().err().map(|error| error.message);
        if target == Format::Vox && estimate.dims.iter().any(|&n| n > 256) {
            result.limit = Some("VOX dimensions exceed 256 cells on an axis".into());
        }
        result.voxel_estimate = Some(estimate);
        return Ok(result);
    }
    let mut companion_source = options.mesh_output.as_ref().map(|_| source.clone());
    let report = conversion::convert(source, target, &engine_options, sink())?;
    result.omissions = report.omissions;
    result.complete = true;
    if let (Some(companion_path), Some(mut companion_source)) =
        (&options.mesh_output, companion_source.take())
    {
        let format = Format::from_output_path(companion_path)?;
        if format == Format::Op2 {
            return Err(Error::new(
                "E_USAGE",
                "mesh_output needs a mesh-bearing format",
            ));
        }
        let excluded = companion_source.dataset.fields.len();
        companion_source.dataset.fields.clear();
        if excluded > 0 {
            companion_source.omissions.push(Omission {
                stage: conversion::Stage::Destination,
                assumption: None,
                detail: format!("{excluded} result field(s) excluded from companion mesh"),
            });
        }
        result.mesh_output = Some(conversion::convert(
            companion_source,
            format,
            &engine_options,
            sink(),
        )?);
    }
    Ok(result)
}
