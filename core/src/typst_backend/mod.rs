pub mod emit;
pub mod fonts;
mod images;
pub mod map;
mod notes;
mod raster;
pub mod world;

use crate::error::Error;
use crate::{Output, Request};
use crate::model::Document;
use fonts::FontSet;
use std::path::Path;
use typst::diag::SourceDiagnostic;
use typst_layout::PagedDocument;
use typst_pdf::PdfOptions;
use world::ConverterWorld;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Pdf,
    Png,
}

pub fn render_pdf(document: Document, fonts_dir: &Path) -> Result<Vec<u8>, Error> {
    let mut output = render(document, fonts_dir, &Request::default(), Format::Pdf)?;
    Ok(output.files.pop().unwrap_or_default())
}

pub fn render(mut document: Document, fonts_dir: &Path, request: &Request, format: Format) -> Result<Output, Error> {
    sanitize_pages(&mut document);
    let sizes: Vec<(f64, f64)> = document.sections.iter().map(|s| (s.page.width, s.page.height)).collect();
    let mut kept: Option<Vec<usize>> = None;
    if document.page_per_section && !request.pages.is_empty() {
        if let Some(page) = request.pages.iter().find(|p| **p >= sizes.len()) {
            return Err(Error::new(format!("page {page} is out of range, the document has {} pages", sizes.len())));
        }
        let mut wanted = request.pages.clone();
        wanted.sort_unstable();
        wanted.dedup();
        let mut index = 0;
        document.sections.retain(|_| {
            index += 1;
            wanted.binary_search(&(index - 1)).is_ok()
        });
        kept = Some(wanted);
    }
    let doc = &document;
    let fonts = FontSet::load(fonts_dir)?;
    let mut emitted = emit::emit(doc, &fonts, request.map);

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

    let count = paged.pages().len();
    let numbers: Vec<usize> = match &kept {
        Some(wanted) if wanted.len() == count => wanted.clone(),
        _ => (0..count).collect(),
    };
    if let Some(page) = request.pages.iter().find(|p| !numbers.contains(p)) {
        return Err(Error::new(format!("page {page} is out of range, the document has {count} pages")));
    }
    let laid = |page: usize| numbers.iter().position(|n| *n == page).unwrap_or(0);
    let selected: Vec<usize> = if request.pages.is_empty() { (0..count).collect() } else { request.pages.iter().map(|p| laid(*p)).collect() };

    let map = request.map.then(|| {
        let sizes: Vec<(f64, f64)> = match &kept {
            Some(_) => sizes,
            None => paged.pages().iter().map(|p| (p.frame.width().to_pt(), p.frame.height().to_pt())).collect(),
        };
        let wanted: Vec<usize> = if kept.is_some() { Vec::new() } else { request.pages.clone() };
        map::build(&paged, world.main_source(), &emitted.marks, &wanted, &numbers, &sizes)
    });

    let files = match format {
        Format::Pdf => {
            let mut options = PdfOptions::default();
            if !request.pages.is_empty() && kept.is_none() {
                options.page_ranges = Some(page_ranges(&selected));
                options.tagged = false;
            }
            let mut pdf = typst_pdf::pdf(&paged, &options)
                .map_err(|errors| Error::new(format!("pdf export failed: {}", diagnostics(&errors))))?;
            let has_notes = doc.sections.iter().any(|s| !s.notes.is_empty());
            if has_notes && request.pages.is_empty() && count == doc.sections.len() {
                let pages: Vec<(f64, &[crate::model::PageNote])> =
                    doc.sections.iter().map(|s| (s.page.height, s.notes.as_slice())).collect();
                pdf = notes::append_notes(pdf, &pages)?;
            }
            vec![pdf]
        }
        Format::Png => {
            let scale = request.scale.unwrap_or(1.0);
            selected.iter().map(|&index| raster::png(&paged.pages()[index], scale)).collect::<Result<_, _>>()?
        }
    };

    typst::comemo::evict(0);
    Ok(Output { files, map })
}

fn page_ranges(pages: &[usize]) -> typst::layout::PageRanges {
    let ranges = pages
        .iter()
        .map(|&p| {
            let number = std::num::NonZeroUsize::new(p + 1);
            number..=number
        })
        .collect();
    typst::layout::PageRanges::new(ranges)
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
