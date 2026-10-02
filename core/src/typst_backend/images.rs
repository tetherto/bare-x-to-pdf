use crate::limits::MAX_IMAGE_PIXELS;
use typst::foundations::Bytes;
use typst::visualize::{ExchangeFormat, RasterImage};

const PLACEHOLDER: [u8; 68] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0x00,
    0x02, 0x00, 0x00, 0x05, 0x00, 0x01, 0x7a, 0x5e, 0xab, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

pub fn sanitize(path: &str, data: Vec<u8>) -> Vec<u8> {
    let format = match path.rsplit('.').next() {
        Some("png") => ExchangeFormat::Png,
        Some("jpg") => ExchangeFormat::Jpg,
        Some("gif") => ExchangeFormat::Gif,
        _ => return data,
    };
    let within_limit = dimensions(format, &data).is_some_and(|(w, h)| w > 0 && h > 0 && u64::from(w) * u64::from(h) <= MAX_IMAGE_PIXELS);
    if !within_limit {
        return PLACEHOLDER.to_vec();
    }
    let bytes = Bytes::new(data);
    match RasterImage::plain(bytes.clone(), format) {
        Ok(_) => bytes.to_vec(),
        Err(_) => PLACEHOLDER.to_vec(),
    }
}

fn dimensions(format: ExchangeFormat, data: &[u8]) -> Option<(u32, u32)> {
    match format {
        ExchangeFormat::Png => {
            let ihdr = data.get(12..24)?;
            if &ihdr[..4] != b"IHDR" {
                return None;
            }
            Some((u32::from_be_bytes(ihdr[4..8].try_into().ok()?), u32::from_be_bytes(ihdr[8..12].try_into().ok()?)))
        }
        ExchangeFormat::Gif => {
            let screen = data.get(6..10)?;
            Some((u32::from(u16::from_le_bytes([screen[0], screen[1]])), u32::from(u16::from_le_bytes([screen[2], screen[3]]))))
        }
        ExchangeFormat::Jpg => jpeg_dimensions(data),
        _ => None,
    }
}

fn jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let mut pos = 2;
    while pos + 4 <= data.len() {
        if data[pos] != 0xff {
            return None;
        }
        let marker = data[pos + 1];
        if marker == 0xff {
            pos += 1;
            continue;
        }
        if marker == 0xd8 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            pos += 2;
            continue;
        }
        let length = usize::from(u16::from_be_bytes([data[pos + 2], data[pos + 3]]));
        let is_frame = (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc);
        if is_frame {
            let frame = data.get(pos + 5..pos + 9)?;
            let height = u32::from(u16::from_be_bytes([frame[0], frame[1]]));
            let width = u32::from(u16::from_be_bytes([frame[2], frame[3]]));
            return Some((width, height));
        }
        if length < 2 {
            return None;
        }
        pos += 2 + length;
    }
    None
}
