//! Bound NetCDF-3 header counts before the upstream parser allocates from them.

use crate::core::Result;

use super::read::err;

struct Header<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Header<'_> {
    fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }

    fn take(&mut self, count: usize) -> Result<&[u8]> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or_else(|| err("NetCDF header length overflow"))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| err("NetCDF header exceeds input size"))?;
        self.offset = end;
        Ok(value)
    }

    fn word(&mut self) -> Result<usize> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| err("truncated NetCDF header word"))?;
        Ok(u32::from_be_bytes(bytes) as usize)
    }

    fn counted(&mut self, minimum_bytes_per_item: usize) -> Result<usize> {
        let count = self.word()?;
        if count > self.remaining() / minimum_bytes_per_item {
            return Err(err("NetCDF header count exceeds input size"));
        }
        Ok(count)
    }

    fn name(&mut self) -> Result<()> {
        let length = self.word()?;
        let padded = length
            .checked_add(3)
            .ok_or_else(|| err("NetCDF name length overflow"))?
            & !3;
        self.take(padded)?;
        Ok(())
    }

    fn list(&mut self, tag: usize, minimum_bytes_per_item: usize) -> Result<usize> {
        match self.word()? {
            0 => {
                if self.word()? != 0 {
                    return Err(err("invalid absent NetCDF header list"));
                }
                Ok(0)
            }
            found if found == tag => self.counted(minimum_bytes_per_item),
            _ => Err(err("invalid NetCDF header list tag")),
        }
    }

    fn attributes(&mut self) -> Result<()> {
        for _ in 0..self.list(12, 12)? {
            self.name()?;
            let width = match self.word()? {
                1 | 2 => 1,
                3 => 2,
                4 | 5 => 4,
                6 => 8,
                _ => return Err(err("invalid NetCDF attribute type")),
            };
            let count = self.word()?;
            let length = count
                .checked_mul(width)
                .and_then(|value| value.checked_add(3))
                .ok_or_else(|| err("NetCDF attribute length overflow"))?
                & !3;
            self.take(length)?;
        }
        Ok(())
    }
}

/// Check every count and byte span used by the NetCDF-3 header parser.
pub(super) fn check(bytes: &[u8]) -> Result<()> {
    let mut header = Header { bytes, offset: 4 };
    let records = header.word()?;
    if records > bytes.len() / 4 {
        return Err(err("NetCDF record count exceeds input size"));
    }
    for _ in 0..header.list(10, 8)? {
        header.name()?;
        header.word()?;
    }
    header.attributes()?;
    for _ in 0..header.list(11, 24)? {
        header.name()?;
        let dimensions = header.counted(4)?;
        header.take(dimensions * 4)?;
        header.attributes()?;
        header.take(12)?; // type, declared size, and classic 32-bit offset
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::formats::exodus::read_projection;

    const SOURCE: &[u8] = include_bytes!("../../../../tests/fixtures/exodus-legacy.exo");

    #[test]
    fn malformed_header_counts_fail_before_netcdf_allocation() {
        check(SOURCE).unwrap();
        // List and variable dimension counts in this independently written
        // classic NetCDF fixture previously reached upstream reservations.
        for offset in [12, 184, 348, 364] {
            let mut huge = SOURCE.to_vec();
            huge[offset..offset + 4].copy_from_slice(&0x4000_0000_u32.to_be_bytes());
            let error = read_projection(&huge, None).unwrap_err();
            assert_eq!(error.code, "E_EXODUS");
            assert!(error.message.contains("count exceeds input size"));
        }

        // A too-large character payload is rejected before its values are read.
        let mut name = SOURCE.to_vec();
        name[16..20].copy_from_slice(&0x4000_0000_u32.to_be_bytes());
        assert_eq!(read_projection(&name, None).unwrap_err().code, "E_EXODUS");
    }
}
