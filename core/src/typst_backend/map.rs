use super::emit::{Literal, Marks, EMPTY_RUN, LABEL_RUN};
use crate::model::{Ref, RefValue};
use std::collections::HashMap;
use std::fmt::Write;
use typst::foundations::{Content, Value};
use typst::introspection::{Location, MetadataElem, Tag};
use typst::layout::{Abs, Frame, FrameItem, Point, Transform};
use typst::model::{TableCell, TableElem};
use typst::syntax::{Source, SpanKind};
use typst_layout::PagedDocument;

#[derive(Clone, Copy)]
struct Rect {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Rect {
    fn of(ts: Transform, x0: Abs, y0: Abs, x1: Abs, y1: Abs) -> Rect {
        let corners = [
            Point::new(x0, y0).transform(ts),
            Point::new(x1, y0).transform(ts),
            Point::new(x0, y1).transform(ts),
            Point::new(x1, y1).transform(ts),
        ];
        let mut rect = Rect { x0: f64::INFINITY, y0: f64::INFINITY, x1: f64::NEG_INFINITY, y1: f64::NEG_INFINITY };
        for corner in corners {
            rect.x0 = rect.x0.min(corner.x.to_pt());
            rect.y0 = rect.y0.min(corner.y.to_pt());
            rect.x1 = rect.x1.max(corner.x.to_pt());
            rect.y1 = rect.y1.max(corner.y.to_pt());
        }
        rect
    }

    fn union(self, other: Rect) -> Rect {
        Rect { x0: self.x0.min(other.x0), y0: self.y0.min(other.y0), x1: self.x1.max(other.x1), y1: self.y1.max(other.y1) }
    }

    fn intersect(self, other: Rect) -> Rect {
        Rect { x0: self.x0.max(other.x0), y0: self.y0.max(other.y0), x1: self.x1.min(other.x1), y1: self.y1.min(other.y1) }
    }

    fn area(&self) -> f64 {
        (self.x1 - self.x0).max(0.0) * (self.y1 - self.y0).max(0.0)
    }
}

fn grow(slot: &mut Option<Rect>, rect: Rect) {
    *slot = Some(slot.map_or(rect, |r| r.union(rect)));
}

struct Hit {
    page: usize,
    literal: usize,
    start: u32,
    end: u32,
    rect: Rect,
    baseline: f64,
    size: f64,
}

struct CellHit {
    page: usize,
    mark: usize,
    x: f64,
    y: f64,
    x1: Option<f64>,
    y1: Option<f64>,
    grid: usize,
    table: Option<usize>,
}

struct Grid {
    location: Option<Location>,
    opened: usize,
    extent: HashMap<usize, Rect>,
}

struct Item {
    page: usize,
    order: usize,
    kind: &'static str,
    mark: Option<usize>,
    tag: Option<Ref>,
    rect: Rect,
    text: Option<(u32, u32)>,
}

struct Walker<'a> {
    source: &'a Source,
    marks: &'a Marks,
    literals: HashMap<usize, usize>,
    spans: HashMap<u64, Option<usize>>,
    chars: HashMap<usize, Vec<usize>>,
    hits: Vec<Hit>,
    cells: Vec<CellHit>,
    pending_cell: Option<(Point, Transform)>,
    clip: Option<Rect>,
    grids: Vec<Grid>,
    grid_stack: Vec<usize>,
    open: Vec<(usize, usize, Option<Rect>)>,
    tables: Vec<usize>,
    body: (f64, f64),
    items: Vec<Item>,
    sections: HashMap<usize, usize>,
    page: usize,
    page_height: f64,
    section: usize,
}

pub fn build(paged: &PagedDocument, source: &Source, marks: &Marks, pages: &[usize], numbers: &[usize], sizes: &[(f64, f64)]) -> String {
    let mut walker = Walker {
        source,
        marks,
        literals: marks.literals.iter().enumerate().map(|(i, l)| (l.start, i)).collect(),
        spans: HashMap::new(),
        chars: HashMap::new(),
        hits: Vec::new(),
        cells: Vec::new(),
        pending_cell: None,
        clip: None,
        grids: Vec::new(),
        grid_stack: Vec::new(),
        open: Vec::new(),
        tables: Vec::new(),
        body: (0.0, 0.0),
        items: Vec::new(),
        sections: HashMap::new(),
        page: 0,
        page_height: 0.0,
        section: 0,
    };
    for (index, page) in paged.pages().iter().enumerate() {
        walker.page = index;
        walker.page_height = page.frame.size().y.to_pt();
        walker.enter_section(walker.section);
        walker.frame(&page.frame, Transform::identity());
        walker.flush_page();
    }
    walker.paragraphs();
    walker.cell_items();
    walker.regions();

    let mut items: Vec<Item> = walker
        .items
        .into_iter()
        .filter(|i| i.rect.x1 >= i.rect.x0 && i.rect.y1 >= i.rect.y0)
        .map(|mut i| {
            i.page = numbers.get(i.page).copied().unwrap_or(i.page);
            i
        })
        .filter(|i| pages.is_empty() || pages.contains(&i.page))
        .collect();
    items.sort_by_key(|i| (i.page, i.order));

    let mut out = String::from("{\"version\":1,\"pages\":[");
    for (index, (width, height)) in sizes.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(out, "{{\"width\":{},\"height\":{}}}", num(*width), num(*height));
    }
    out.push_str("],\"items\":[");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let tag = item.tag.as_ref().or_else(|| item.mark.and_then(|m| marks.tags.get(m - 1)).map(|m| &m.tag));
        let _ = write!(out, "{{\"page\":{},\"kind\":\"{}\",\"ref\":", item.page, item.kind);
        write_ref(&mut out, tag);
        let r = item.rect;
        let _ = write!(
            out,
            ",\"box\":{{\"x\":{},\"y\":{},\"width\":{},\"height\":{}}}",
            num(r.x0),
            num(r.y0),
            num(r.x1 - r.x0),
            num(r.y1 - r.y0)
        );
        if let Some((start, end)) = item.text {
            let _ = write!(out, ",\"text\":{{\"start\":{start},\"end\":{end}}}");
        }
        out.push('}');
    }
    out.push_str("]}");
    out
}

impl Walker<'_> {
    fn frame(&mut self, frame: &Frame, ts: Transform) {
        for (pos, item) in frame.items() {
            match item {
                FrameItem::Group(group) => {
                    let inner = ts.pre_concat(Transform::translate(pos.x, pos.y)).pre_concat(group.transform);
                    let size = group.frame.size();
                    let rect = Rect::of(inner, Abs::zero(), Abs::zero(), size.x, size.y);
                    let outer_clip = self.clip;
                    if let Some(curve) = &group.clip {
                        let bbox = curve.bbox(None);
                        let clip = Rect::of(inner, bbox.min.x, bbox.min.y, bbox.max.x, bbox.max.y);
                        self.clip = Some(outer_clip.map_or(clip, |c| c.intersect(clip)));
                    }
                    if rect.area() > 0.0 {
                        self.cover_group(rect);
                    }
                    self.frame(&group.frame, inner);
                    self.clip = outer_clip;
                }
                FrameItem::Text(text) => self.text(text, *pos, ts),
                FrameItem::Shape(shape, _) => {
                    let bbox = shape.bbox(true);
                    let rect = Rect::of(ts, pos.x + bbox.min.x, pos.y + bbox.min.y, pos.x + bbox.max.x, pos.y + bbox.max.y);
                    self.cover(rect);
                }
                FrameItem::Image(_, size, _) => {
                    let rect = Rect::of(ts, pos.x, pos.y, pos.x + size.x, pos.y + size.y);
                    self.cover(rect);
                }
                FrameItem::Tag(Tag::Start(content, _)) => self.tag(content, *pos, ts),
                FrameItem::Tag(Tag::End(location, ..)) => {
                    if let Some(at) = self.grid_stack.iter().rposition(|g| self.grids[*g].location == Some(*location)) {
                        self.grid_stack.truncate(at);
                    }
                }
                _ => {}
            }
        }
    }

    fn tag(&mut self, content: &Content, local: Point, ts: Transform) {
        if content.is::<TableElem>() {
            self.grids.push(Grid { location: content.location(), opened: self.page, extent: HashMap::new() });
            self.grid_stack.push(self.grids.len() - 1);
            return;
        }
        if content.is::<TableCell>() {
            self.pending_cell = Some((local, ts));
            return;
        }
        let Some(meta) = content.to_packed::<MetadataElem>() else { return };
        if let Some(label) = content.label() {
            if let Some(index) = label.resolve().as_str().strip_prefix("section-").and_then(|n| n.parse::<usize>().ok()) {
                self.sections.entry(index).or_insert(self.page);
                self.enter_section(index);
            }
            return;
        }
        let Value::Int(n) = meta.value else { return };
        let Some(mark) = usize::try_from(n.unsigned_abs()).ok().filter(|m| (1..=self.marks.tags.len()).contains(m)) else { return };
        let kind = self.marks.tags[mark - 1].kind;
        if n > 0 {
            if kind == "cell" || kind == "sheetCell" {
                if let (Some((origin, ts)), Some(&grid)) = (self.pending_cell.take(), self.grid_stack.last()) {
                    let at = origin.transform(ts);
                    let cell = &self.marks.tags[mark - 1];
                    let far = Point::new(
                        origin.x + Abs::pt(cell.width.unwrap_or(0.0)),
                        origin.y + Abs::pt(cell.height.unwrap_or(0.0)),
                    )
                    .transform(ts);
                    self.cells.push(CellHit {
                        page: self.page,
                        mark,
                        x: at.x.to_pt(),
                        y: at.y.to_pt(),
                        x1: cell.width.map(|_| far.x.to_pt()),
                        y1: cell.height.map(|_| far.y.to_pt()),
                        grid,
                        table: self.tables.last().copied(),
                    });
                }
            } else if self.is_block_table(mark) {
                self.tables.push(mark);
            } else {
                self.open.push((mark, self.page, None));
            }
        } else if let Some(at) = self.tables.iter().rposition(|m| *m == mark) {
            self.tables.remove(at);
        } else if let Some(at) = self.open.iter().rposition(|(m, _, _)| *m == mark) {
            let (mark, _, rect) = self.open.remove(at);
            if let Some(rect) = rect {
                self.push_item(mark, rect);
            }
        }
    }

    fn enter_section(&mut self, index: usize) {
        self.section = index;
        self.body = match self.marks.bodies.get(index) {
            Some(&(top, bottom)) => (top, self.page_height - bottom),
            None => (0.0, self.page_height),
        };
    }

    fn is_block_table(&self, mark: usize) -> bool {
        self.marks.tags[mark - 1].grid
    }

    fn clipped(&self, rect: Rect) -> Option<Rect> {
        let rect = self.clip.map_or(rect, |c| c.intersect(rect));
        (rect.x1 >= rect.x0 && rect.y1 >= rect.y0).then_some(rect)
    }

    fn cover_group(&mut self, rect: Rect) {
        let Some(rect) = self.clipped(rect) else { return };
        let in_body = rect.y1 > self.body.0 && rect.y0 < self.body.1;
        let page = self.page;
        for (_, opened, slot) in &mut self.open {
            if *opened == page || in_body {
                grow(slot, rect);
            }
        }
    }

    fn cover(&mut self, rect: Rect) {
        let Some(rect) = self.clipped(rect) else { return };
        self.cover_group(rect);
        let in_body = rect.y1 > self.body.0 && rect.y0 < self.body.1;
        let page = self.page;
        for &grid in &self.grid_stack {
            let grid = &mut self.grids[grid];
            if grid.opened == page || in_body {
                let slot = grid.extent.entry(page).or_insert(rect);
                *slot = slot.union(rect);
            }
        }
    }

    fn push_item(&mut self, mark: usize, rect: Rect) {
        self.items.push(Item {
            page: self.page,
            order: mark,
            kind: self.marks.tags[mark - 1].kind,
            mark: Some(mark),
            tag: None,
            rect,
            text: None,
        });
    }

    fn flush_page(&mut self) {
        for index in 0..self.open.len() {
            if let (mark, _, Some(rect)) = self.open[index] {
                self.push_item(mark, rect);
                self.open[index].2 = None;
            }
        }
        self.pending_cell = None;
    }

    fn text(&mut self, text: &typst::text::TextItem, pos: Point, ts: Transform) {
        let metrics = text.font.metrics();
        let top = pos.y - metrics.ascender.at(text.size);
        let bottom = pos.y - metrics.descender.at(text.size);
        let baseline = Point::new(pos.x, pos.y).transform(ts).y.to_pt();
        let mut x = pos.x;
        let mut all: Option<Rect> = None;
        for glyph in &text.glyphs {
            let advance = glyph.x_advance.at(text.size);
            let rect = Rect::of(ts, x, top, x + advance, bottom);
            grow(&mut all, rect);
            let Some(rect) = self.clipped(rect) else {
                x += advance;
                continue;
            };
            if let Some(literal) = self.literal_of(glyph.span.0) {
                let byte = usize::from(glyph.span.1);
                let len = usize::from(glyph.range.end.saturating_sub(glyph.range.start)).max(1);
                if let Some((start, end)) = self.units(literal, byte, len) {
                    self.hits.push(Hit { page: self.page, literal, start, end, rect, baseline, size: text.size.to_pt() });
                }
            }
            x += advance;
        }
        if let Some(rect) = all {
            self.cover(rect);
        }
    }

    fn literal_of(&mut self, span: typst::syntax::Span) -> Option<usize> {
        let key = span.into_raw().get();
        if let Some(found) = self.spans.get(&key) {
            return *found;
        }
        let found = match span.get() {
            SpanKind::Number { id, num } if id == self.source.id() => self.source.range(num, None).and_then(|r| self.literals.get(&r.start).copied()),
            _ => None,
        };
        self.spans.insert(key, found);
        found
    }

    fn units(&mut self, literal: usize, byte: usize, len: usize) -> Option<(u32, u32)> {
        let lit: &Literal = &self.marks.literals[literal];
        let chars = self.chars.entry(literal).or_insert_with(|| {
            let mut index = vec![usize::MAX; lit.value.len() + 1];
            for (i, (b, _)) in lit.value.char_indices().enumerate() {
                index[b] = i;
            }
            index[lit.value.len()] = lit.value.chars().count();
            index
        });
        let first = *chars.get(byte).filter(|c| **c != usize::MAX)?;
        let after = (byte + len).min(lit.value.len());
        let last = (after..=lit.value.len()).find_map(|b| chars.get(b).copied().filter(|c| *c != usize::MAX)).unwrap_or(first + 1);
        let start = lit.units.get(first)?.0;
        let end = lit.units.get(last.saturating_sub(1).max(first)).map_or(start, |u| u.1);
        Some((start, end))
    }

    fn paragraphs(&mut self) {
        let mut by_mark: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut order: Vec<usize> = Vec::new();
        for (index, hit) in self.hits.iter().enumerate() {
            let mark = self.marks.literals[hit.literal].mark;
            by_mark
                .entry(mark)
                .or_insert_with(|| {
                    order.push(mark);
                    Vec::new()
                })
                .push(index);
        }
        for mark in order {
            let hits = &by_mark[&mark];
            let mut lines: Vec<Vec<usize>> = Vec::new();
            for &index in hits {
                let hit = &self.hits[index];
                let new_line = match lines.last().and_then(|l| l.last()) {
                    None => true,
                    Some(&prev) => {
                        let prev = &self.hits[prev];
                        let line = lines.last().unwrap();
                        let size = line.iter().map(|&i| self.hits[i].size).fold(hit.size, f64::max);
                        let dy = (hit.baseline - prev.baseline).abs();
                        prev.page != hit.page || dy > 0.6 * size || (hit.rect.x0 < prev.rect.x0 - 1.0 && dy > 0.5)
                    }
                };
                if new_line {
                    lines.push(Vec::new());
                }
                lines.last_mut().unwrap().push(index);
            }

            let mut fragment: Option<(usize, Rect, f64)> = None;
            for line in &lines {
                let page = self.hits[line[0]].page;
                let rect = line.iter().map(|&i| self.hits[i].rect).reduce(Rect::union).unwrap();
                let text_hits: Vec<&Hit> = line.iter().map(|&i| &self.hits[i]).filter(|h| self.marks.literals[h.literal].run != LABEL_RUN).collect();
                let text = if text_hits.is_empty() {
                    None
                } else if text_hits.iter().all(|h| self.marks.literals[h.literal].run == EMPTY_RUN) {
                    Some((text_hits[0].start, text_hits[0].start))
                } else {
                    let real = text_hits.iter().filter(|h| self.marks.literals[h.literal].run != EMPTY_RUN);
                    let start = real.clone().map(|h| h.start).min().unwrap_or(0);
                    let end = real.map(|h| h.end).max().unwrap_or(start);
                    Some((start, end))
                };
                self.items.push(Item { page, order: mark, kind: "line", mark: Some(mark), tag: None, rect, text });

                let mut run: Option<(u32, Rect, u32, u32)> = None;
                for hit in &text_hits {
                    let id = self.marks.literals[hit.literal].run;
                    if id == EMPTY_RUN {
                        continue;
                    }
                    match &mut run {
                        Some((current, r, start, end)) if *current == id => {
                            *r = r.union(hit.rect);
                            *start = (*start).min(hit.start);
                            *end = (*end).max(hit.end);
                        }
                        _ => {
                            if let Some((_, r, start, end)) = run.take() {
                                self.items.push(Item { page, order: mark, kind: "run", mark: Some(mark), tag: None, rect: r, text: Some((start, end)) });
                            }
                            run = Some((id, hit.rect, hit.start, hit.end));
                        }
                    }
                }
                if let Some((_, r, start, end)) = run {
                    self.items.push(Item { page, order: mark, kind: "run", mark: Some(mark), tag: None, rect: r, text: Some((start, end)) });
                }

                fragment = match fragment {
                    Some((p, r, top)) if p == page && rect.y0 >= top - 1.0 => Some((p, r.union(rect), rect.y0)),
                    Some((p, r, _)) => {
                        self.items.push(Item { page: p, order: mark, kind: "paragraph", mark: Some(mark), tag: None, rect: r, text: None });
                        Some((page, rect, rect.y0))
                    }
                    None => Some((page, rect, rect.y0)),
                };
            }
            if let Some((p, r, _)) = fragment {
                self.items.push(Item { page: p, order: mark, kind: "paragraph", mark: Some(mark), tag: None, rect: r, text: None });
            }
        }
    }

    fn cell_items(&mut self) {
        let mut by_grid: HashMap<(usize, usize), (Vec<f64>, Vec<f64>)> = HashMap::new();
        for cell in &self.cells {
            let entry = by_grid.entry((cell.grid, cell.page)).or_default();
            entry.0.push(cell.x);
            entry.1.push(cell.y);
        }
        let empty = Rect { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 };
        let extent = |grids: &[Grid], key: &(usize, usize)| grids[key.0].extent.get(&key.1).copied().unwrap_or(empty);
        for (key, (xs, ys)) in &mut by_grid {
            let bounds = extent(&self.grids, key);
            xs.push(bounds.x1);
            ys.push(bounds.y1);
            for list in [xs, ys] {
                list.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                list.dedup_by(|a, b| (*a - *b).abs() < 0.05);
            }
        }
        let mut tables: HashMap<(usize, usize), Rect> = HashMap::new();
        let mut table_order: Vec<(usize, usize)> = Vec::new();
        for cell in &self.cells {
            let mark = &self.marks.tags[cell.mark - 1];
            let key = (cell.grid, cell.page);
            let (xs, ys) = &by_grid[&key];
            let bounds = extent(&self.grids, &key);
            let next = |list: &[f64], at: f64, span: usize, fallback: f64| {
                list.iter().position(|v| (v - at).abs() < 0.05).and_then(|i| list.get(i + span.max(1)).copied()).unwrap_or(fallback)
            };
            let rect = Rect {
                x0: cell.x,
                y0: cell.y,
                x1: cell.x1.unwrap_or_else(|| next(xs, cell.x, mark.colspan, bounds.x1)),
                y1: cell.y1.unwrap_or_else(|| next(ys, cell.y, mark.rowspan, bounds.y1)),
            };
            self.items.push(Item { page: cell.page, order: cell.mark, kind: mark.kind, mark: Some(cell.mark), tag: None, rect, text: None });
            if let Some(table) = cell.table {
                let key = (table, cell.page);
                if !tables.contains_key(&key) {
                    table_order.push(key);
                }
                let slot = tables.entry(key).or_insert(rect);
                *slot = slot.union(rect);
            }
        }
        for key in table_order {
            self.items.push(Item { page: key.1, order: key.0, kind: "table", mark: Some(key.0), tag: None, rect: tables[&key], text: None });
        }
    }

    fn regions(&mut self) {
        for (section, region) in &self.marks.regions {
            let Some(&page) = self.sections.get(section) else { continue };
            self.items.push(Item {
                page,
                order: 0,
                kind: region.tag.kind.unwrap_or("shape"),
                mark: None,
                tag: Some(region.tag.clone()),
                rect: Rect { x0: region.x, y0: region.y, x1: region.x + region.width, y1: region.y + region.height },
                text: None,
            });
        }
    }
}

fn write_ref(out: &mut String, tag: Option<&Ref>) {
    out.push('{');
    for (index, (key, value)) in tag.map(|t| t.fields.as_slice()).unwrap_or(&[]).iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(out, "\"{key}\":");
        match value {
            RefValue::Int(n) => {
                let _ = write!(out, "{n}");
            }
            RefValue::Bool(b) => {
                let _ = write!(out, "{b}");
            }
            RefValue::Str(s) => json_str(out, s),
        }
    }
    out.push('}');
}

fn json_str(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn num(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    let rounded = (value * 100.0).round() / 100.0;
    if rounded == 0.0 {
        "0".to_string()
    } else {
        format!("{rounded}")
    }
}
