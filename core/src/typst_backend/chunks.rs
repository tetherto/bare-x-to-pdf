use super::emit::{self, Chunk};
use super::fonts::FontSet;
use super::{export, layout, merge};
use crate::error::Error;
use crate::model::Document;
use std::num::NonZeroUsize;
use std::ops::Range;
use typst::foundations::{Label, NativeElement, Selector, Value};
use typst::introspection::{Introspector, MetadataElem};
use typst::layout::PageRanges;
use typst::model::FootnoteElem;
use typst::utils::PicoStr;
use typst_layout::PagedDocument;
use typst_pdf::PdfOptions;

pub const CHUNK_SOURCE_BYTES: usize = 2 * 1024 * 1024;
const TOP_TOLERANCE: f64 = 0.5;

type Cursor = (usize, usize);

type Sink<'a> = dyn FnMut(&PagedDocument, usize) -> Result<(), Error> + 'a;

pub fn render(doc: &Document, fonts: &FontSet, sizes: &[Vec<usize>], uses_total_pages: bool) -> Result<Vec<u8>, Error> {
    render_in_chunks(doc, fonts, sizes, uses_total_pages, CHUNK_SOURCE_BYTES)
}

fn render_in_chunks(doc: &Document, fonts: &FontSet, sizes: &[Vec<usize>], uses_total_pages: bool, budget: usize) -> Result<Vec<u8>, Error> {
    let mut pdfs = Vec::new();
    paginate(doc, fonts, sizes, uses_total_pages, budget, &mut |paged, keep| {
        let options = PdfOptions {
            tagged: false,
            page_ranges: Some(PageRanges::new(vec![NonZeroUsize::new(1)..=NonZeroUsize::new(keep)])),
            ..PdfOptions::default()
        };
        pdfs.push(export(paged, &options)?);
        Ok(())
    })?;
    merge::merge(&pdfs)
}

fn paginate(doc: &Document, fonts: &FontSet, sizes: &[Vec<usize>], uses_total_pages: bool, budget: usize, sink: &mut Sink) -> Result<(), Error> {
    let total_pages = if uses_total_pages {
        Some(run(doc, fonts, sizes, None, budget, &mut |_, _| Ok(()))?)
    } else {
        None
    };
    run(doc, fonts, sizes, total_pages, budget, sink)?;
    Ok(())
}

fn run(doc: &Document, fonts: &FontSet, sizes: &[Vec<usize>], total_pages: Option<u64>, budget: usize, sink: &mut Sink) -> Result<u64, Error> {
    let mut last_number = 0;
    let mut chunk = Chunk {
        total_pages,
        ..Chunk::default()
    };
    let mut cursor: Cursor = (0, 0);
    while cursor.0 < sizes.len() {
        let mut span = budget;
        let (paged, keep, resume) = loop {
            let (parts, next) = take(sizes, cursor, span);
            chunk.parts = parts;
            let paged = layout(emit::emit_chunk(doc, fonts, &chunk), fonts.clone())?;
            let count = paged.pages().len();
            if next.0 >= sizes.len() {
                break (paged, count, next);
            }
            if let Some((keep, resume)) = find_cut(&paged, doc, cursor) {
                break (paged, keep, resume);
            }
            drop(paged);
            typst::comemo::evict(0);
            span = span.saturating_mul(2);
        };
        if keep > 0 {
            sink(&paged, keep)?;
            last_number = paged.pages()[keep - 1].number;
            chunk.page_number = Some(last_number + 1);
        }
        chunk.footnotes += footnotes_on(&paged, keep);
        chunk.page_offset += keep;
        chunk.resumed = true;
        drop(paged);
        typst::comemo::evict(0);
        cursor = resume;
    }
    Ok(last_number)
}

fn take(sizes: &[Vec<usize>], cursor: Cursor, budget: usize) -> (Vec<(usize, Range<usize>)>, Cursor) {
    let (mut section, mut block) = cursor;
    let mut parts = Vec::new();
    let mut used = 0;
    while section < sizes.len() {
        let start = block;
        let blocks = &sizes[section];
        while block < blocks.len() && (used < budget || (parts.is_empty() && block == start)) {
            used += blocks[block];
            block += 1;
        }
        parts.push((section, start..block));
        if block < blocks.len() {
            return (parts, (section, block));
        }
        section += 1;
        block = 0;
        if used >= budget {
            break;
        }
    }
    (parts, (section, 0))
}

fn find_cut(paged: &PagedDocument, doc: &Document, start: Cursor) -> Option<(usize, Cursor)> {
    let label = Label::new(PicoStr::intern(emit::CUT_LABEL))?;
    let introspector = paged.introspector();
    let mut best: Option<(usize, Cursor)> = None;
    for content in introspector.query(&Selector::Label(label)) {
        let Some(meta) = content.to_packed::<MetadataElem>() else {
            continue;
        };
        let Value::Int(id) = meta.value else {
            continue;
        };
        let at = emit::cut_position(id);
        if at <= start {
            continue;
        }
        let Some(position) = content.location().and_then(|l| introspector.position(l)) else {
            continue;
        };
        let page = position.page.get();
        let Some(section) = doc.sections.get(at.0) else {
            continue;
        };
        let scaled = section.content_scale > 0.0 && (section.content_scale - 1.0).abs() > 0.001;
        if page < 2 || section.columns > 1 || scaled {
            continue;
        }
        if (position.point.y.to_pt() - section.page.margin.top).abs() > TOP_TOLERANCE {
            continue;
        }
        let better = match best {
            None => true,
            Some((best_page, best_at)) => page > best_page || (page == best_page && at < best_at),
        };
        if better {
            best = Some((page, at));
        }
    }
    best.map(|(page, at)| (page - 1, at))
}

fn footnotes_on(paged: &PagedDocument, pages: usize) -> usize {
    let introspector = paged.introspector();
    introspector
        .query(&Selector::Elem(FootnoteElem::ELEM, None))
        .iter()
        .filter(|content| {
            content
                .location()
                .and_then(|l| introspector.position(l))
                .is_some_and(|p| p.page.get() <= pages)
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write;
    use std::path::Path;
    use typst::layout::{Frame, FrameItem};

    const BUDGET: usize = 16 * 1024;

    fn fixture() -> (Document, FontSet) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let bytes = std::fs::read(root.join("test/fixtures/chunked-sections.docx")).unwrap();
        let mut doc = crate::docx::read(&bytes).unwrap();
        super::super::sanitize_pages(&mut doc);
        (doc, FontSet::load(&root.join("fonts")).unwrap())
    }

    fn page_text(page: &typst_layout::Page) -> String {
        fn walk(frame: &Frame, out: &mut String) {
            for (point, item) in frame.items() {
                match item {
                    FrameItem::Group(group) => walk(&group.frame, out),
                    FrameItem::Text(text) => {
                        let _ = write!(out, "{:.2},{:.2} {}|", point.x.to_pt(), point.y.to_pt(), text.text);
                    }
                    _ => {}
                }
            }
        }
        let mut out = format!("#{} ", page.number);
        walk(&page.frame, &mut out);
        out
    }

    #[test]
    fn chunked_layout_matches_a_single_pass() {
        let (doc, fonts) = fixture();
        let full = emit::emit(&doc, &fonts);
        let sizes = full.block_sizes.clone();
        let uses_total_pages = full.uses_total_pages;
        assert!(uses_total_pages);
        let single: Vec<String> = layout(full, fonts.clone()).unwrap().pages().iter().map(page_text).collect();

        let mut chunked = Vec::new();
        let mut chunks = 0;
        paginate(&doc, &fonts, &sizes, uses_total_pages, BUDGET, &mut |paged, keep| {
            chunks += 1;
            chunked.extend(paged.pages()[..keep].iter().map(page_text));
            Ok(())
        })
        .unwrap();

        assert!(chunks >= 8, "only {chunks} chunks");
        assert_eq!(chunked.len(), single.len());
        for (index, (a, b)) in chunked.iter().zip(&single).enumerate() {
            assert_eq!(a, b, "page {}", index + 1);
        }
    }

    #[test]
    fn merged_pdf_keeps_every_page() {
        let (doc, fonts) = fixture();
        let full = emit::emit(&doc, &fonts);
        let pages = layout(emit::emit(&doc, &fonts), fonts.clone()).unwrap().pages().len();
        let pdf = render_in_chunks(&doc, &fonts, &full.block_sizes, full.uses_total_pages, BUDGET).unwrap();
        assert_eq!(merge::page_count(&pdf).unwrap(), pages);
    }
}
