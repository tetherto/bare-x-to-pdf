use crate::error::Error;
use std::io::Read;

pub const MAX_XML_DEPTH: usize = 512;
pub const MAX_FORMULA_DEPTH: usize = 256;
pub const MAX_ZIP_ENTRY_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_TABLE_SPAN: usize = 1000;
pub const MAX_TABLE_DEPTH: u32 = 12;
pub const MAX_TEXT_COLUMNS: usize = 64;
pub const MAX_REPEATED_SPACES: usize = 1000;
pub const MAX_CHART_POINTS: usize = 100_000;
pub const MAX_LIST_VALUE: i64 = 32_767;
pub const MAX_IMAGE_PIXELS: u64 = 50_000_000;
pub const MAX_IMAGE_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_RANGE_CELLS: u64 = 1_000_000;
pub const MAX_SHEET_ROWS: u32 = 1_048_576;
pub const MAX_SHEET_COLUMNS: u32 = 16_384;
pub const MAX_SHEET_CELLS: u64 = 30_000;
pub const MIN_PAGE_SIDE: f64 = 72.0;
pub const MAX_PAGE_SIDE: f64 = 14_400.0;
pub const MAX_LENGTH: f64 = 1_000_000.0;
pub const MAX_PNG_SCALE: f64 = 8.0;
pub const MAX_PNG_PIXELS: f64 = 64_000_000.0;

pub fn span(value: usize) -> usize {
    value.clamp(1, MAX_TABLE_SPAN)
}

pub fn finite(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(-MAX_LENGTH, MAX_LENGTH)
    } else {
        0.0
    }
}

pub fn read_entry(reader: impl Read) -> Result<Vec<u8>, Error> {
    let mut data = Vec::new();
    reader.take(MAX_ZIP_ENTRY_BYTES + 1).read_to_end(&mut data)?;
    if data.len() as u64 > MAX_ZIP_ENTRY_BYTES {
        return Err(Error::new(format!("a part of the document is larger than {} MB uncompressed", MAX_ZIP_ENTRY_BYTES / (1024 * 1024))));
    }
    Ok(data)
}
