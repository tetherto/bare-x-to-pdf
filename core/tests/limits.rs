use simple_converter_core::{limits, xml};
use std::io::Read;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn hostile(name: &str) -> Vec<u8> {
    std::fs::read(root().join("test/fixtures/hostile").join(name)).unwrap()
}

fn convert(name: &str, from: &str) -> Result<Vec<u8>, simple_converter_core::error::Error> {
    let fonts = root().join("fonts");
    let options = simple_converter_core::Options { fonts_dir: &fonts };
    simple_converter_core::convert(&hostile(name), from, "pdf", &options)
}

#[test]
fn read_entry_stops_at_the_size_limit() {
    let small = limits::read_entry(std::io::repeat(b'x').take(1024)).unwrap();
    assert_eq!(small.len(), 1024);
    let error = limits::read_entry(std::io::repeat(b'x').take(limits::MAX_ZIP_ENTRY_BYTES + 1)).unwrap_err();
    assert!(error.message().contains("larger than"));
}

#[test]
fn finite_and_span_clamp_hostile_numbers() {
    assert_eq!(limits::finite(f64::NAN), 0.0);
    assert_eq!(limits::finite(f64::INFINITY), 0.0);
    assert_eq!(limits::finite(1e300), limits::MAX_LENGTH);
    assert_eq!(limits::finite(-12.5), -12.5);
    assert_eq!(limits::span(0), 1);
    assert_eq!(limits::span(usize::MAX), limits::MAX_TABLE_SPAN);
}

#[test]
fn xml_parse_rejects_excessive_nesting() {
    let depth = limits::MAX_XML_DEPTH + 10;
    let deep = format!("{}{}", "<a>".repeat(depth), "</a>".repeat(depth));
    assert!(xml::parse(deep.as_bytes()).unwrap_err().message().contains("nested"));
    let shallow = format!("{}{}", "<a>".repeat(100), "</a>".repeat(100));
    assert!(xml::parse(shallow.as_bytes()).is_ok());
}

#[test]
fn deeply_nested_documents_fail_cleanly() {
    assert!(convert("deep-unknown-elements.docx", "docx").is_err());
    assert!(convert("deep-sdt.docx", "docx").is_err());
}

#[test]
fn deeply_nested_formulas_do_not_overflow_the_stack() {
    assert!(convert("cf-nested-parens.xlsx", "xlsx").is_ok());
    assert!(convert("cf-nested-minus.xlsx", "xlsx").is_ok());
    assert!(convert("geometry-nested-parens.odp", "odp").is_ok());
}

#[test]
fn huge_declared_values_are_clamped() {
    for (name, from) in [
        ("gridspan-huge.docx", "docx"),
        ("colspan-huge.odt", "odt"),
        ("numbering-letters-huge.docx", "docx"),
        ("chart-point-index-huge.pptx", "pptx"),
        ("text-space-count-huge.odt", "odt"),
        ("arcto-infinite.pptx", "pptx"),
        ("xlsx-far-cell.xlsx", "xlsx"),
    ] {
        assert!(convert(name, from).is_ok(), "{name}");
    }
}
