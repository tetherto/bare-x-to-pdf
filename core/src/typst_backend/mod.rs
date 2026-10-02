pub mod emit;
pub mod fonts;
mod images;
mod notes;
pub mod world;

use crate::error::Error;
use crate::model::Document;
use fonts::FontSet;
use std::path::Path;
use typst::diag::SourceDiagnostic;
use typst_layout::PagedDocument;
use typst_pdf::PdfOptions;
use world::ConverterWorld;

pub fn render_pdf(mut document: Document, fonts_dir: &Path) -> Result<Vec<u8>, Error> {
    sanitize_pages(&mut document);
    let doc = &document;
    let fonts = FontSet::load(fonts_dir)?;
    let mut emitted = emit::emit(doc, &fonts);

    if let Ok(path) = std::env::var("SIMPLE_CONVERTER_DUMP_TYPST") {
        let _ = std::fs::write(path, &emitted.source);
    }
    if let Ok(path) = std::env::var("SIMPLE_CONVERTER_LOAD_TYPST") {
        emitted.source = std::fs::read_to_string(path).map_err(|e| Error::new(format!("cannot read typst source: {e}")))?;
    }

    let mut world = ConverterWorld::new(emitted.source, fonts);
    for (path, data) in emitted.files {
        let data = images::sanitize(&path, data);
        world.add_file(&path, data)?;
    }
    let compiled = typst::compile::<PagedDocument>(&world);
    let paged = compiled
        .output
        .map_err(|errors| Error::new(format!("layout failed: {}", diagnostics(&errors))))?;

    let mut pdf = typst_pdf::pdf(&paged, &PdfOptions::default())
        .map_err(|errors| Error::new(format!("pdf export failed: {}", diagnostics(&errors))))?;

    let has_notes = doc.sections.iter().any(|s| !s.notes.is_empty());
    if has_notes && paged.pages().len() == doc.sections.len() {
        let pages: Vec<(f64, &[crate::model::PageNote])> =
            doc.sections.iter().map(|s| (s.page.height, s.notes.as_slice())).collect();
        pdf = notes::append_notes(pdf, &pages)?;
    }

    typst::comemo::evict(0);
    Ok(pdf)
}

fn diagnostics(errors: &[SourceDiagnostic]) -> String {
    let mut messages: Vec<String> = Vec::new();
    for error in errors {
        let message = error.message.to_string();
        if !messages.contains(&message) {
            messages.push(message);
        }
        if messages.len() == 5 {
            break;
        }
    }
    messages.join("; ")
}

fn sanitize_pages(document: &mut Document) {
    for section in &mut document.sections {
        let page = &mut section.page;
        page.width = side(page.width);
        page.height = side(page.height);
        let margin = &mut page.margin;
        for value in [&mut margin.top, &mut margin.right, &mut margin.bottom, &mut margin.left, &mut margin.header, &mut margin.footer] {
            *value = crate::limits::finite(*value);
        }
        fit_margins(&mut margin.left, &mut margin.right, page.width);
        fit_margins(&mut margin.top, &mut margin.bottom, page.height);
    }
}

fn side(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(crate::limits::MIN_PAGE_SIDE, crate::limits::MAX_PAGE_SIDE)
    } else {
        crate::limits::MIN_PAGE_SIDE
    }
}

fn fit_margins(start: &mut f64, end: &mut f64, size: f64) {
    let available = size - crate::limits::MIN_PAGE_SIDE / 2.0;
    let used = start.abs() + end.abs();
    if used > available && used > 0.0 {
        let scale = available / used;
        *start *= scale;
        *end *= scale;
    }
}
