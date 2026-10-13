//! Convert through the public file API, then present the approved receipt.

use std::io::Write;

use caexfer::{ConversionReport, Error, Omission, PreviewReport, Result, convert, preview};

use super::approval;
use super::args::Args;
use super::common::{conversion_options, emit, path_json};
use super::json::{array, object, quote};
use super::output::{self, PendingOutput};

/// Convert into private paths so the CLI can confirm losses before install.
pub(super) fn run_convert(args: &Args) -> Result<u8> {
    if args.preview {
        let report = preview(&args.paths[0], &args.paths[1], &conversion_options(args)?)?;
        present_preview(args, &report)?;
        return Ok(0);
    }
    if args.mesh_out.as_deref() == Some(args.paths[1].as_path()) {
        return Err(Error::new("E_USAGE", "paired outputs need distinct paths"));
    }
    let mut primary = PendingOutput::new(&args.paths[1], &args.paths[0], args.overwrite)?;
    let mut companion = args
        .mesh_out
        .as_deref()
        .map(|path| PendingOutput::new(path, &args.paths[0], args.overwrite))
        .transpose()?;
    let mut options = conversion_options(args)?;
    // The library may write every proposed projection into private paths.
    // Only the CLI approval step can install those bytes at requested names.
    options.accept_all = true;
    options.mesh_output = companion
        .as_ref()
        .map(|pending| pending.temporary().to_path_buf());
    let mut report = convert(&args.paths[0], primary.temporary(), &options)?;
    if companion.is_some() && report.mesh_output.is_none() {
        return Err(Error::new(
            "E_OUTPUT",
            "conversion produced no companion report",
        ));
    }
    approval::confirm(
        args,
        &report,
        report
            .mesh_output
            .as_ref()
            .map(|item| item.omissions.as_slice()),
    )?;
    if let Some(companion) = &mut companion {
        output::install_pair(&mut primary, companion)?;
        if let Some(receipt) = &mut report.mesh_output {
            receipt.path.clone_from(
                args.mesh_out
                    .as_ref()
                    .ok_or_else(|| Error::new("E_OUTPUT", "missing companion path"))?,
            );
        }
    } else {
        primary.install()?;
    }
    present(args, &report)?;
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

/// Render the installed conversion receipt in the selected CLI format.
fn present(args: &Args, report: &ConversionReport) -> Result<()> {
    if args.json {
        let mut fields = vec![
            ("schema_version", "1".into()),
            ("operation", quote("mesh-and-fields-projection")),
            ("output", path_json(&args.paths[1])),
            ("points", report.points.to_string()),
            ("cells", report.cells.to_string()),
            ("fields", report.fields.to_string()),
            ("units", quote("unspecified")),
            ("omissions", omissions_json(&report.omissions)),
        ];
        if let Some((dimensions, occupied)) = report.voxel_grid {
            fields.push((
                "voxel_grid",
                object([
                    (
                        "dimensions",
                        array(dimensions.into_iter().map(|v| v.to_string())),
                    ),
                    ("occupied_cells", occupied.to_string()),
                ]),
            ));
        }
        if let Some(companion) = &report.mesh_output {
            fields.push((
                "mesh_output",
                object([
                    ("path", path_json(&companion.path)),
                    ("format", quote(companion.format.name())),
                    ("omissions", omissions_json(&companion.omissions)),
                ]),
            ));
        }
        emit(&object(fields))?;
    } else {
        if report.voxel_grid.is_some() {
            emit(&format!(
                "Wrote {} from {} source points, {} source cells, {} source field(s). Units unspecified.",
                args.paths[1].display(),
                report.points,
                report.cells,
                report.fields
            ))?;
        } else {
            emit(&format!(
                "Wrote {}: {} points, {} cells, {} source field(s). Units unspecified.",
                args.paths[1].display(),
                report.points,
                report.cells,
                report.fields
            ))?;
        }
        if let Some(([nx, ny, nz], occupied)) = report.voxel_grid {
            emit(&format!(
                "Voxel grid: {nx} × {ny} × {nz} cells, {occupied} occupied."
            ))?;
        }
        let mut stderr = std::io::stderr().lock();
        for omission in &report.omissions {
            writeln!(stderr, "Omission: {}", omission.detail)?;
        }
        if let Some(companion) = &report.mesh_output {
            emit(&format!(
                "Wrote companion mesh {}.",
                companion.path.display()
            ))?;
            for omission in &companion.omissions {
                writeln!(stderr, "Companion omission: {}", omission.detail)?;
            }
        }
    }
    Ok(())
}

/// Encode omission stages and details for the versioned CLI JSON schema.
fn omissions_json(omissions: &[Omission]) -> String {
    array(omissions.iter().map(|omission| {
        object([
            ("stage", quote(omission.stage.name())),
            ("acceptance_flag", quote(approval::flag_for(omission))),
            ("detail", quote(&omission.detail)),
        ])
    }))
}
