use crate::error::Error;
use crate::limits::MAX_PNG_PIXELS;
use typst_layout::Page;
use typst_render::RenderOptions;

pub fn png(page: &Page, scale: f64) -> Result<Vec<u8>, Error> {
    let size = page.frame.size();
    let pixels = size.x.to_pt() * scale * size.y.to_pt() * scale;
    if !(pixels <= MAX_PNG_PIXELS) {
        return Err(Error::new(format!("the page image would have more than {MAX_PNG_PIXELS} pixels, use a smaller scale")));
    }
    let pixmap = typst_render::render(page, &RenderOptions { pixel_per_pt: scale.into(), render_bleed: false });
    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect();

    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, pixmap.width(), pixmap.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| Error::new(format!("png encoding failed: {e}")))?;
    writer.write_image_data(&rgba).map_err(|e| Error::new(format!("png encoding failed: {e}")))?;
    writer.finish().map_err(|e| Error::new(format!("png encoding failed: {e}")))?;
    Ok(out)
}
