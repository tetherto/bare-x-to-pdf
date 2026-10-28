pub mod doc;
pub mod docx;
pub mod error;
pub mod limits;
pub mod markdown;
pub mod model;
pub mod odf_chart;
pub mod odt;
pub mod pptx;
pub mod typst_backend;
pub mod xlsx;
pub mod xml;

use error::Error;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;

pub struct Options<'a> {
    pub fonts_dir: &'a Path,
}

#[derive(Debug, Clone, Default)]
pub struct Request {
    pub map: bool,
    pub pages: Vec<usize>,
    pub scale: Option<f64>,
}

impl Request {
    pub fn parse(spec: &str) -> Result<Request, Error> {
        let mut request = Request::default();
        for pair in spec.split(';').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            match key {
                "map" => request.map = value == "1",
                "pages" => {
                    request.pages = value
                        .split(',')
                        .filter(|v| !v.is_empty())
                        .map(|v| v.parse().map_err(|_| Error::new(format!("pages must list page indexes, got {v}"))))
                        .collect::<Result<_, _>>()?
                }
                "scale" => {
                    let scale: f64 = value.parse().map_err(|_| Error::new(format!("scale must be a number, got {value}")))?;
                    if !(scale.is_finite() && scale > 0.0 && scale <= limits::MAX_PNG_SCALE) {
                        return Err(Error::new(format!("scale must be above 0 and at most {}", limits::MAX_PNG_SCALE)));
                    }
                    request.scale = Some(scale);
                }
                _ => return Err(Error::new(format!("unknown option {key}"))),
            }
        }
        Ok(request)
    }
}

pub struct Output {
    pub files: Vec<Vec<u8>>,
    pub map: Option<String>,
}

pub fn convert(input: &[u8], from: &str, to: &str, options: &Options) -> Result<Vec<u8>, Error> {
    let mut output = convert_with(input, from, to, options, &Request::default())?;
    Ok(output.files.pop().unwrap_or_default())
}

pub fn convert_with(input: &[u8], from: &str, to: &str, options: &Options, request: &Request) -> Result<Output, Error> {
    let format = match to {
        "pdf" => typst_backend::Format::Pdf,
        "png" => typst_backend::Format::Png,
        _ => return Err(Error::new(format!("conversion from {from} to {to} is not supported"))),
    };
    let document = read(input, from).map_err(|e| match e {
        Some(e) => e,
        None => Error::new(format!("conversion from {from} to {to} is not supported")),
    })?;
    typst_backend::render(document, options.fonts_dir, request, format)
}

fn read(input: &[u8], from: &str) -> Result<model::Document, Option<Error>> {
    let compound = input.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]).then(|| compound_kind(input)).flatten();
    let from = match from {
        "doc" | "dot" if input.starts_with(b"PK") => "docx",
        "xls" | "xlt" if input.starts_with(b"PK") => "xlsx",
        "doc" | "dot" | "docx" | "docm" | "dotx" | "dotm" if compound == Some("xls") => "xls",
        "xls" | "xlt" | "xlsx" | "xlsm" | "xltx" | "xltm" if compound == Some("doc") => "doc",
        "docx" | "docm" | "dotx" | "dotm" if input.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) => "doc",
        "odt" | "ott" if input.starts_with(b"<") || input.starts_with(&[0xEF, 0xBB, 0xBF]) => "fodt",
        other => other,
    };
    let document = match from {
        "docx" | "docm" | "dotx" | "dotm" => docx::read(input),
        "doc" | "dot" => doc::read(input),
        "odt" | "ott" | "fodt" => odt::read(input),
        "xlsx" | "xlsm" | "xltx" | "xltm" => xlsx::read(input),
        "odp" | "otp" | "fodp" => odt::read_odp(input),
        "ods" | "ots" | "fods" => xlsx::read_ods(input),
        "xls" | "xlt" => xlsx::read_xls(input),
        "md" | "markdown" => markdown::read(input),
        "pptx" | "pptm" | "potx" | "potm" | "ppsx" | "ppsm" => pptx::read(input),
        _ => return Err(None),
    };
    document.map_err(Some)
}

impl Output {
    fn meta(&self) -> String {
        let sizes: Vec<String> = self.files.iter().map(|f| f.len().to_string()).collect();
        format!("{{\"sizes\":[{}],\"map\":{}}}", sizes.join(","), self.map.as_deref().unwrap_or("null"))
    }
}

fn compound_kind(input: &[u8]) -> Option<&'static str> {
    let file = cfb::CompoundFile::open(std::io::Cursor::new(input)).ok()?;
    if file.exists("/WordDocument") {
        Some("doc")
    } else if file.exists("/Workbook") || file.exists("/Book") {
        Some("xls")
    } else {
        None
    }
}

#[repr(C)]
pub struct Buffer {
    pub data: *mut u8,
    pub len: usize,
}

impl Buffer {
    fn from_vec(bytes: Vec<u8>) -> Buffer {
        let boxed = bytes.into_boxed_slice();
        let len = boxed.len();
        let data = Box::into_raw(boxed) as *mut u8;
        Buffer { data, len }
    }

    fn empty() -> Buffer {
        Buffer {
            data: std::ptr::null_mut(),
            len: 0,
        }
    }
}

unsafe fn c_string(ptr: *const c_char, name: &str) -> Result<String, Error> {
    if ptr.is_null() {
        return Err(Error::new(format!("{name} must not be null")));
    }
    CStr::from_ptr(ptr)
        .to_str()
        .map(str::to_owned)
        .map_err(|_| Error::new(format!("{name} must be valid UTF-8")))
}

#[no_mangle]
pub unsafe extern "C" fn sc_convert(
    input: *const u8,
    input_len: usize,
    from: *const c_char,
    to: *const c_char,
    fonts_dir: *const c_char,
    out: *mut Buffer,
    error: *mut Buffer,
) -> i32 {
    sc_convert_with(input, input_len, from, to, fonts_dir, std::ptr::null(), out, std::ptr::null_mut(), error)
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sc_convert_with(
    input: *const u8,
    input_len: usize,
    from: *const c_char,
    to: *const c_char,
    fonts_dir: *const c_char,
    request: *const c_char,
    out: *mut Buffer,
    meta: *mut Buffer,
    error: *mut Buffer,
) -> i32 {
    if out.is_null() || error.is_null() {
        return 3;
    }
    *out = Buffer::empty();
    *error = Buffer::empty();
    if !meta.is_null() {
        *meta = Buffer::empty();
    }

    static QUIET_PANICS: std::sync::Once = std::sync::Once::new();
    QUIET_PANICS.call_once(|| panic::set_hook(Box::new(|_| {})));

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let input: &[u8] = if input.is_null() {
            &[]
        } else {
            std::slice::from_raw_parts(input, input_len)
        };
        let from = c_string(from, "from")?;
        let to = c_string(to, "to")?;
        let fonts_dir = c_string(fonts_dir, "fontsDir")?;
        let options = Options {
            fonts_dir: Path::new(&fonts_dir),
        };
        let request = if request.is_null() { Request::default() } else { Request::parse(&c_string(request, "options")?)? };
        convert_with(input, &from, &to, &options, &request)
    }));

    match result {
        Ok(Ok(output)) => {
            if !meta.is_null() {
                *meta = Buffer::from_vec(output.meta().into_bytes());
            }
            *out = Buffer::from_vec(output.files.concat());
            0
        }
        Ok(Err(e)) => {
            *error = Buffer::from_vec(e.message().as_bytes().to_vec());
            1
        }
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            *error = Buffer::from_vec(format!("internal error: {message}").into_bytes());
            2
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn sc_free(buffer: *mut Buffer) {
    if buffer.is_null() {
        return;
    }
    let b = &mut *buffer;
    if !b.data.is_null() {
        drop(Box::from_raw(std::slice::from_raw_parts_mut(b.data, b.len)));
    }
    b.data = std::ptr::null_mut();
    b.len = 0;
}
