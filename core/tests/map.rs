use simple_converter_core::{convert_with, Options, Output, Request};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn run(path: &str, to: &str, request: Request) -> Output {
    let path = root().join(path);
    let from = path.extension().unwrap().to_str().unwrap().to_string();
    let fonts = root().join("fonts");
    let options = Options { fonts_dir: &fonts };
    convert_with(&std::fs::read(&path).unwrap(), &from, to, &options, &request).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn mapped(path: &str) -> (Vec<u8>, String) {
    let mut output = run(path, "pdf", Request { map: true, ..Request::default() });
    (output.files.remove(0), output.map.expect("map requested"))
}

fn items(map: &str) -> Vec<&str> {
    map.split("{\"page\":").skip(1).collect()
}

fn pdf_pages(pdf: &[u8]) -> usize {
    String::from_utf8_lossy(pdf).matches("/Type/Page/").count()
}

#[test]
fn map_leaves_the_pdf_unchanged() {
    for path in ["test/fixtures/sdk-sample.docx", "test/fixtures/office-features.pptx", "test/fixtures/office-features.xlsx"] {
        let plain = run(path, "pdf", Request::default());
        let (pdf, _) = mapped(path);
        assert!(plain.map.is_none());
        assert_eq!(plain.files[0], pdf, "{path}");
    }
}

#[test]
fn word_items_use_body_block_indexes_and_paragraph_ids() {
    let (_, map) = mapped("test/fixtures/office-features.docx");
    assert!(map.starts_with("{\"version\":1,\"pages\":[{\"width\":792,\"height\":612}]"));
    let all = items(&map);
    let paragraph = all.iter().find(|i| i.contains("\"kind\":\"paragraph\",\"ref\":{\"part\":\"body\",\"block\":0,\"id\":\"")).expect("first body paragraph");
    let id = paragraph.split("\"id\":\"").nth(1).unwrap().split('"').next().unwrap();
    assert_eq!(id.len(), 8);
    assert!(id.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase()));
    assert!(all.iter().any(|i| i.contains("\"ref\":{\"part\":\"footnote\",\"id\":\"")));
    for line in all.iter().filter(|i| i.contains("\"kind\":\"line\"") || i.contains("\"kind\":\"run\"")) {
        let text = line.split("\"text\":{\"start\":").nth(1).expect("lines and runs carry text offsets");
        let start: u32 = text.split(',').next().unwrap().parse().unwrap();
        let end: u32 = text.split("\"end\":").nth(1).unwrap().trim_end_matches(['}', ',', ']']).parse().unwrap();
        assert!(start <= end);
    }
}

#[test]
fn word_table_cells_cover_their_grid_slots() {
    let (_, map) = mapped("test/fixtures/sdk-sample.docx");
    let all = items(&map);
    let cells: Vec<&&str> = all.iter().filter(|i| i.contains("\"kind\":\"cell\"")).collect();
    assert_eq!(cells.len(), 36);
    assert!(cells.iter().any(|c| c.contains("\"ref\":{\"part\":\"body\",\"block\":1,\"row\":5,\"column\":5,\"id\":\"")));
    assert!(all.iter().any(|i| i.contains("\"kind\":\"table\",\"ref\":{\"part\":\"body\",\"block\":1,")));
}

#[test]
fn slide_items_name_slides_shapes_groups_and_empty_placeholders() {
    let (_, map) = mapped("test/fixtures/office-features.pptx");
    let all = items(&map);
    let has = |kind: &str, reference: &str| all.iter().any(|i| i.contains(&format!("\"kind\":\"{kind}\",\"ref\":{reference}")));
    assert!(has("slide", "{\"slide\":0,\"slideId\":256}"));
    assert!(has("shape", "{\"slide\":0,\"slideId\":256,\"shape\":2}"));
    assert!(has("shape", "{\"slide\":0,\"slideId\":256,\"shape\":6}"));
    assert!(has("shape", "{\"slide\":0,\"slideId\":256,\"shape\":5}"));
    assert!(has("run", "{\"slide\":0,\"slideId\":256,\"shape\":4,\"paragraph\":0}"));
}

#[test]
fn sheet_cells_map_to_a1_refs_and_hidden_columns_do_not_print() {
    let (_, map) = mapped("test/fixtures/office-features.xlsx");
    let all = items(&map);
    let cell = |name: &str| all.iter().any(|i| i.contains(&format!("\"kind\":\"sheetCell\",\"ref\":{{\"sheet\":\"Sheet1\",\"cell\":\"{name}\"}}")));
    assert!(cell("A1"));
    assert!(cell("E4"));
    assert!(!cell("D2"));
    assert!(!cell("A5"));
}

#[test]
fn formulas_without_cached_values_print_a_value_or_the_formula() {
    let (_, map) = mapped("test/fixtures/office-uncached-formulas.xlsx");
    let all = items(&map);
    let text_in = |name: &str| all.iter().any(|i| i.contains("\"kind\":\"line\"") && i.contains(&format!("\"cell\":\"{name}\"")));
    assert!(text_in("D3"));
    assert!(text_in("D6"));
    let (_, map) = mapped("test/fixtures/office-features.xlsx");
    let all = items(&map);
    let fallback = all.iter().find(|i| i.contains("\"kind\":\"line\"") && i.contains("\"cell\":\"B6\"")).expect("formula text is printed");
    assert!(fallback.contains("\"text\":{\"start\":0,\"end\":13}"));
}

#[test]
fn pages_select_slides_and_keep_document_page_numbers() {
    let all = run("bench/corpus/lo-group.pptx", "pdf", Request { map: true, ..Request::default() });
    let one = run("bench/corpus/lo-group.pptx", "pdf", Request { map: true, pages: vec![1], ..Request::default() });
    let map = one.map.unwrap();
    let pages = |map: &str| map.split("],\"items\"").next().unwrap().matches("\"width\":").count();
    assert_eq!(pages(&map), 2);
    assert_eq!(pages(&all.map.unwrap()), 2);
    assert!(items(&map).iter().all(|i| i.starts_with('1')));
    assert_eq!(pdf_pages(&one.files[0]), 1);
}

#[test]
fn pages_select_word_pages_from_the_full_layout() {
    let full = run("test/fixtures/office-features.docx", "pdf", Request::default());
    let first = run("test/fixtures/office-features.docx", "pdf", Request { pages: vec![0], ..Request::default() });
    assert_eq!(pdf_pages(&full.files[0]), 1);
    assert_eq!(pdf_pages(&first.files[0]), 1);
    let path = root().join("test/fixtures/office-features.docx");
    let fonts = root().join("fonts");
    let error = convert_with(&std::fs::read(path).unwrap(), "docx", "pdf", &Options { fonts_dir: &fonts }, &Request { pages: vec![3], ..Request::default() }).err().unwrap();
    assert!(error.message().contains("out of range"));
}

#[test]
fn png_output_renders_each_requested_page() {
    let output = run("test/fixtures/office-features.pptx", "png", Request { scale: Some(0.5), ..Request::default() });
    assert_eq!(output.files.len(), 1);
    let png = &output.files[0];
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
    assert_eq!((width, height), (480, 270));
}

#[test]
fn request_parses_the_binding_options() {
    let request = Request::parse("map=1;pages=2,0;scale=1.5").unwrap();
    assert!(request.map);
    assert_eq!(request.pages, vec![2, 0]);
    assert_eq!(request.scale, Some(1.5));
    assert!(Request::parse("scale=0").is_err());
    assert!(Request::parse("pages=x").is_err());
    assert!(Request::parse("color=1").is_err());
}

#[test]
fn template_and_macro_presentations_are_read() {
    let bytes = std::fs::read(root().join("test/fixtures/office-features.pptx")).unwrap();
    let fonts = root().join("fonts");
    for from in ["pptm", "potx", "potm", "ppsx", "ppsm"] {
        let pdf = simple_converter_core::convert(&bytes, from, "pdf", &Options { fonts_dir: &fonts }).unwrap();
        assert!(pdf.starts_with(b"%PDF-"), "{from}");
    }
}
