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

    fn fixture(name: &str) -> (Document, FontSet) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let bytes = std::fs::read(root.join("test/fixtures").join(name)).unwrap();
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

    fn chunked_pages(name: &str) -> (Vec<String>, Vec<Vec<String>>) {
        let (doc, fonts) = fixture(name);
        let full = emit::emit(&doc, &fonts);
        let sizes = full.block_sizes.clone();
        let uses_total_pages = full.uses_total_pages;
        let single = layout(full, fonts.clone()).unwrap().pages().iter().map(page_text).collect();
        let mut chunks = Vec::new();
        paginate(&doc, &fonts, &sizes, uses_total_pages, BUDGET, &mut |paged, keep| {
            chunks.push(paged.pages()[..keep].iter().map(page_text).collect());
            Ok(())
        })
        .unwrap();
        (single, chunks)
    }

    fn assert_same_pages(single: &[String], chunks: &[Vec<String>]) {
        let chunked: Vec<&String> = chunks.iter().flatten().collect();
        assert_eq!(chunked.len(), single.len());
        for (index, (a, b)) in chunked.iter().zip(single).enumerate() {
            assert_eq!(*a, b, "page {}", index + 1);
        }
    }

    #[test]
    fn chunked_layout_matches_a_single_pass() {
        let (doc, fonts) = fixture("chunked-sections.docx");
        assert!(emit::emit(&doc, &fonts).uses_total_pages);
        let (single, chunks) = chunked_pages("chunked-sections.docx");
        assert!(chunks.len() >= 8, "only {} chunks", chunks.len());
        assert_same_pages(&single, &chunks);
    }

    #[test]
    fn chunks_never_end_inside_columns_or_a_table() {
        let (single, chunks) = chunked_pages("chunked-no-cut.docx");
        assert!(chunks.len() >= 3, "only {} chunks", chunks.len());
        assert_same_pages(&single, &chunks);
        let block_starts = ["Opening paragraph", "End of", "Columns paragraph 0:", "row 0 cell 0", "Closing paragraph"];
        for chunk in &chunks[1..] {
            let first = chunk[0].split_once('|').map_or("", |(text, _)| text);
            assert!(block_starts.iter().any(|start| first.contains(start)), "a chunk starts inside a block: {first}");
        }
        let longest = chunks.iter().map(Vec::len).max().unwrap();
        let table_pages = single.iter().filter(|page| page.contains("row ")).count();
        assert!(longest >= table_pages, "the table was split across chunks");
    }

    #[test]
    fn merged_pdf_is_well_formed() {
        for name in ["chunked-sections.docx", "chunked-no-cut.docx"] {
            let (doc, fonts) = fixture(name);
            let full = emit::emit(&doc, &fonts);
            let pages = layout(emit::emit(&doc, &fonts), fonts.clone()).unwrap().pages().len();
            let pdf = render_in_chunks(&doc, &fonts, &full.block_sizes, full.uses_total_pages, BUDGET).unwrap();
            assert_eq!(merge::check(&pdf).unwrap(), pages, "{name}");
        }
    }

    #[test]
    fn typst_pdfs_have_the_structure_merge_expects() {
        let (doc, fonts) = fixture("chunked-sections.docx");
        let paged = layout(emit::emit(&doc, &fonts), fonts).unwrap();
        for tagged in [true, false] {
            let options = PdfOptions {
                tagged,
                ..PdfOptions::default()
            };
            let pdf = export(&paged, &options).unwrap();
            assert_eq!(merge::check(&pdf).unwrap(), paged.pages().len());
            let doubled = merge::merge(&[pdf.clone(), pdf]).unwrap();
            assert_eq!(merge::check(&doubled).unwrap(), 2 * paged.pages().len());
        }
    }
}
