//! Format-aware conversion with explicit source and destination omissions.
//!
//! [`read_path`] loads a supported source; [`convert`] writes its supported
//! projection to a caller-owned stream. The returned [`ConversionReport`]
//! describes what was carried and what was omitted.

mod format;
mod read;
mod report;
mod tetra;
mod write;

#[cfg(test)]
mod tests;

pub use format::{Format, InfillShape, Options};
pub use read::read_path;
pub use report::{AssumptionKind, CompanionReport, ConversionReport, Omission, ReadResult, Stage};
pub use write::convert;

/// Read a VTI reference without expanding occupancy into an explicit mesh.
pub(crate) fn load_match_grid(
    options: &Options,
) -> crate::core::Result<Option<crate::core::VoxelGrid>> {
    use crate::core::Error;
    use crate::formats::vti;
    use std::io::Read;
    let Some(path) = &options.match_grid else {
        return Ok(None);
    };
    if Format::from_input_path(path)? != Format::Vti {
        return Err(Error::new("E_USAGE", "match_grid requires a VTI file"));
    }
    let size = std::fs::metadata(path)?.len();
    if size > options.max_bytes as u64 {
        return Err(Error::new("E_LIMIT", "reference grid exceeds max_bytes"));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(options.max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > options.max_bytes {
        return Err(Error::new("E_LIMIT", "reference grid exceeds max_bytes"));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| Error::new("E_VTI", "reference grid must be UTF-8 XML"))?;
    vti::read(&text).map(Some)
}
