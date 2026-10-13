//! Read-only conversion planning and presentation.

use caexfer::{PreviewReport, Result, preview};

use super::args::Args;
use super::common::{conversion_options, emit, path_json};
use super::convert::omissions_json;
use super::json::{array, object, quote};

/// Inspect the requested route without creating an output file.
pub(super) fn run_preview(args: &Args) -> Result<u8> {
    let report = preview(&args.paths[0], &args.paths[1], &conversion_options(args)?)?;
    present_preview(args, &report)?;
    Ok(0)
}

/// Show a read-only plan. Voxel plans stop before sampling; other routes are dry projections.
fn present_preview(args: &Args, report: &PreviewReport) -> Result<()> {
    if args.json {
        let mut fields = vec![
            ("schema_version", "1".into()),
            ("operation", quote("conversion-preview")),
            ("input", path_json(&args.paths[0])),
            ("output", path_json(&args.paths[1])),
            ("source_format", quote(report.source_format.name())),
            ("target_format", quote(report.target_format.name())),
            ("source_bytes", report.source_bytes.to_string()),
            ("points", report.points.to_string()),
            ("cells", report.cells.to_string()),
            ("fields", report.fields.to_string()),
            ("complete", report.complete.to_string()),
            ("omissions", omissions_json(&report.omissions)),
            (
                "limit",
                report
                    .limit
                    .as_ref()
                    .map_or_else(|| "null".into(), |s| quote(s)),
            ),
        ];
        if let Some(grid) = &report.voxel_estimate {
            fields.push((
                "voxel_grid",
                object([
                    (
                        "origin",
                        array(grid.origin.into_iter().map(|n| n.to_string())),
                    ),
                    ("spacing", grid.spacing.to_string()),
                    (
                        "dimensions",
                        array(grid.dims.into_iter().map(|n| n.to_string())),
                    ),
                    ("cells", grid.cells.to_string()),
                    ("max_triangle_checks", grid.triangle_checks.to_string()),
                ]),
            ));
        }
        if let Some(companion) = &report.mesh_output {
            fields.push((
                "mesh_output",
                object([
                    (
                        "path",
                        args.mesh_out
                            .as_ref()
                            .map_or_else(|| "null".into(), |p| path_json(p)),
                    ),
                    ("omissions", omissions_json(&companion.omissions)),
                ]),
            ));
        }
        emit(&object(fields))?;
    } else {
        emit(&format!(
            "Preview {} → {}: {} → {}; {} bytes, {} points, {} cells, {} fields.",
            args.paths[0].display(),
            args.paths[1].display(),
            report.source_format.name(),
            report.target_format.name(),
            report.source_bytes,
            report.points,
            report.cells,
            report.fields
        ))?;
        if let Some(grid) = &report.voxel_estimate {
            emit(&format!(
                "Grid: {} × {} × {} = {} cells; origin [{}, {}, {}], spacing {}; up to {} triangle checks.",
                grid.dims[0],
                grid.dims[1],
                grid.dims[2],
                grid.cells,
                grid.origin[0],
                grid.origin[1],
                grid.origin[2],
                grid.spacing,
                grid.triangle_checks
            ))?;
        }
        if let Some(limit) = &report.limit {
            emit(&format!("Limit: {limit}"))?;
        }
        for omission in &report.omissions {
            emit(&format!("Notice: {}", omission.detail))?;
        }
        if let Some(companion) = &report.mesh_output {
            for omission in &companion.omissions {
                emit(&format!("Companion notice: {}", omission.detail))?;
            }
        }
        if !report.complete {
            emit("Voxel plan only; destination notices require conversion.")?;
        }
        emit("No output written.")?;
    }
    Ok(())
}
