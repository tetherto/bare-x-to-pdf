use crate::error::Error;
use std::io::Write;

struct Parsed<'a> {
    data: &'a [u8],
    spans: Vec<Option<(usize, usize)>>,
    header_end: usize,
    root: usize,
    info: Option<usize>,
    id: Option<&'a [u8]>,
    pages: usize,
    kids: Vec<usize>,
}

pub fn merge(pdfs: &[Vec<u8>]) -> Result<Vec<u8>, Error> {
    match pdfs {
        [] => return Err(Error::new("cannot merge: no pages")),
        [single] => return Ok(single.clone()),
        _ => {}
    }
    let parsed = pdfs.iter().map(|pdf| parse(pdf)).collect::<Result<Vec<_>, _>>()?;
    let objects: usize = parsed.iter().map(|p| p.spans.len() - 1).sum();
    let pages_id = objects + 1;
    let size = pages_id + 1;

    let mut out = Vec::with_capacity(pdfs.iter().map(Vec::len).sum::<usize>() + size * 24);
    out.extend_from_slice(&parsed[0].data[..parsed[0].header_end]);
    let mut offsets: Vec<Option<usize>> = vec![None; size];
    let mut kids = Vec::new();
    let mut base = 0;
    for (index, part) in parsed.iter().enumerate() {
        let renumber = |id: usize| if id == part.pages { pages_id } else { base + id };
        for (id, span) in part.spans.iter().enumerate() {
            let Some((start, end)) = *span else {
                continue;
            };
            let foreign_root = index > 0 && (id == part.root || Some(id) == part.info);
            if id == part.pages || foreign_root {
                continue;
            }
            let object = &part.data[start..end];
            let body_at = find(object, b"obj").ok_or_else(broken)? + 3;
            offsets[base + id] = Some(out.len());
            let _ = write!(out, "{} 0 obj", base + id);
            rewrite(&object[body_at..], &renumber, &mut out);
        }
        kids.extend(part.kids.iter().map(|&kid| base + kid));
        base += part.spans.len() - 1;
    }

    offsets[pages_id] = Some(out.len());
    let _ = write!(out, "{pages_id} 0 obj\n<</Type/Pages/Count {}/Kids[", kids.len());
    for (i, kid) in kids.iter().enumerate() {
        let _ = write!(out, "{}{kid} 0 R", if i > 0 { " " } else { "" });
    }
    out.extend_from_slice(b"]>>\nendobj\n");

    let xref = out.len();
    let _ = write!(out, "xref\n0 {size}\n");
    for offset in &offsets {
        match offset {
            Some(at) => {
                let _ = write!(out, "{at:010} 00000 n \n");
            }
            None => out.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    let first = &parsed[0];
    let _ = write!(out, "trailer\n<</Size {size}/Root {} 0 R", first.root);
    if let Some(info) = first.info {
        let _ = write!(out, "/Info {info} 0 R");
    }
    if let Some(id) = first.id {
        out.extend_from_slice(b"/ID");
        out.extend_from_slice(id);
    }
    let _ = write!(out, ">>\nstartxref\n{xref}\n%%EOF\n");
    Ok(out)
}

#[cfg(test)]
pub fn check(pdf: &[u8]) -> Result<usize, Error> {
    let parsed = parse(pdf)?;
    let referenced = std::cell::RefCell::new(Vec::new());
    for (start, end) in parsed.spans.iter().flatten().copied() {
        let object = &pdf[start..end];
        let body_at = find(object, b"obj").ok_or_else(broken)? + 3;
        rewrite(
            &object[body_at..],
            &|id| {
                referenced.borrow_mut().push(id);
                id
            },
            &mut Vec::new(),
        );
    }
    for id in referenced.into_inner() {
        if parsed.spans.get(id).copied().flatten().is_none() {
            return Err(Error::new(format!("dangling reference to object {id}")));
        }
    }
    let parent = format!("/Parent {} 0 R", parsed.pages);
    for &kid in &parsed.kids {
        let (start, end) = parsed.spans.get(kid).copied().flatten().ok_or_else(broken)?;
        let page = &pdf[start..end];
        if find(page, b"/Type/Page").is_none() || find(page, parent.as_bytes()).is_none() {
            return Err(Error::new(format!("page object {kid} is not a child of the page tree")));
        }
    }
    Ok(parsed.kids.len())
}

fn broken() -> Error {
    Error::new("cannot merge: unexpected PDF structure")
}

fn parse(data: &[u8]) -> Result<Parsed<'_>, Error> {
    let startxref = rfind(data, b"startxref").ok_or_else(broken)?;
    let xref = number_after(data, startxref + 9).ok_or_else(broken)?;
    if !data.get(xref..).is_some_and(|rest| rest.starts_with(b"xref")) {
        return Err(broken());
    }
    let trailer_at = xref + find(&data[xref..], b"trailer").ok_or_else(broken)?;
    let table = std::str::from_utf8(&data[xref + 4..trailer_at]).map_err(|_| broken())?;
    let mut tokens = table.split_ascii_whitespace();
    let mut entries: Vec<(usize, usize)> = Vec::new();
    let mut size = 0;
    while let (Some(first), Some(count)) = (tokens.next(), tokens.next()) {
        let first: usize = first.parse().map_err(|_| broken())?;
        let count: usize = count.parse().map_err(|_| broken())?;
        for id in first..first + count {
            let (Some(offset), Some(_), Some(kind)) = (tokens.next(), tokens.next(), tokens.next()) else {
                return Err(broken());
            };
            if kind == "n" {
                entries.push((offset.parse().map_err(|_| broken())?, id));
            }
            size = size.max(id + 1);
        }
    }
    entries.sort_unstable();
    let mut spans = vec![None; size];
    for (i, &(offset, id)) in entries.iter().enumerate() {
        let end = entries.get(i + 1).map_or(xref, |&(next, _)| next);
        if offset >= end || !data[offset..].starts_with(format!("{id} 0 obj").as_bytes()) {
            return Err(broken());
        }
        spans[id] = Some((offset, end));
    }
    let header_end = entries.first().map(|&(offset, _)| offset).ok_or_else(broken)?;

    let trailer = &data[trailer_at..startxref];
    let root = reference_after(trailer, b"/Root").ok_or_else(broken)?;
    let info = reference_after(trailer, b"/Info");
    let id = find(trailer, b"/ID").and_then(|at| {
        let rest = &trailer[at + 3..];
        let open = rest.iter().position(|&c| c == b'[')?;
        let close = rest.iter().position(|&c| c == b']')?;
        (open < close).then(|| &rest[open..=close])
    });

    let object = |id: usize| spans.get(id).copied().flatten().map(|(start, end)| &data[start..end]);
    let catalog = object(root).ok_or_else(broken)?;
    let pages = reference_after(catalog, b"/Pages").ok_or_else(broken)?;
    let tree = object(pages).ok_or_else(broken)?;
    let kids_at = find(tree, b"/Kids").ok_or_else(broken)?;
    let kids_end = kids_at + find(&tree[kids_at..], b"]").ok_or_else(broken)?;
    let kids: Vec<usize> = std::str::from_utf8(&tree[kids_at + 5..kids_end])
        .map_err(|_| broken())?
        .trim_start_matches(|c: char| c == '[' || c.is_ascii_whitespace())
        .split('R')
        .filter_map(|r| r.split_ascii_whitespace().next().and_then(|n| n.parse().ok()))
        .collect();
    for &kid in &kids {
        let page = object(kid).ok_or_else(broken)?;
        if find(page, b"/Type/Pages").is_some() {
            return Err(broken());
        }
    }

    Ok(Parsed {
        data,
        spans,
        header_end,
        root,
        info,
        id,
        pages,
        kids,
    })
}

fn rewrite(src: &[u8], renumber: &impl Fn(usize) -> usize, out: &mut Vec<u8>) {
    let mut i = 0;
    let mut nesting = 0usize;
    while i < src.len() {
        let c = src[i];
        if nesting > 0 {
            out.push(c);
            match c {
                b'\\' if i + 1 < src.len() => {
                    out.push(src[i + 1]);
                    i += 1;
                }
                b'(' => nesting += 1,
                b')' => nesting -= 1,
                _ => {}
            }
            i += 1;
            continue;
        }
        let boundary = i == 0 || !is_regular(src[i - 1]);
        if c == b'(' {
            nesting = 1;
        } else if boundary && src[i..].starts_with(b"stream") && src.get(i + 6).is_some_and(|&n| n == b'\r' || n == b'\n') {
            out.extend_from_slice(&src[i..]);
            return;
        } else if boundary && c.is_ascii_digit() {
            if let Some((id, len)) = reference(&src[i..]) {
                let _ = write!(out, "{} 0 R", renumber(id));
                i += len;
                continue;
            }
            while i < src.len() && is_regular(src[i]) {
                out.push(src[i]);
                i += 1;
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
}

fn reference(src: &[u8]) -> Option<(usize, usize)> {
    let digits = |from: usize| src[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let spaces = |from: usize| src[from..].iter().take_while(|c| c.is_ascii_whitespace()).count();
    let id_len = digits(0);
    let mut at = id_len;
    let gap = spaces(at);
    if gap == 0 {
        return None;
    }
    at += gap;
    let generation = digits(at);
    if generation == 0 {
        return None;
    }
    at += generation;
    let gap = spaces(at);
    if gap == 0 {
        return None;
    }
    at += gap;
    if src.get(at) != Some(&b'R') || src.get(at + 1).is_some_and(|&c| is_regular(c)) {
        return None;
    }
    let id = std::str::from_utf8(&src[..id_len]).ok()?.parse().ok()?;
    Some((id, at + 1))
}

fn is_regular(c: u8) -> bool {
    !c.is_ascii_whitespace() && !b"()<>[]{}/%".contains(&c)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).rposition(|w| w == needle)
}

fn number_after(data: &[u8], from: usize) -> Option<usize> {
    let rest = data.get(from..)?;
    let start = rest.iter().position(|c| c.is_ascii_digit())?;
    let len = rest[start..].iter().take_while(|c| c.is_ascii_digit()).count();
    std::str::from_utf8(&rest[start..start + len]).ok()?.parse().ok()
}

fn reference_after(data: &[u8], key: &[u8]) -> Option<usize> {
    let at = find(data, key)? + key.len();
    let rest = &data[at..];
    let start = rest.iter().position(|c| !c.is_ascii_whitespace())?;
    reference(&rest[start..]).map(|(id, _)| id)
}
