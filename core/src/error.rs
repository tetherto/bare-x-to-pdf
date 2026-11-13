use std::fmt;

#[derive(Debug, Clone)]
pub struct Error {
    message: String,
    limit: bool,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Error {
            message: message.into(),
            limit: false,
        }
    }

    pub fn limit(message: impl Into<String>) -> Self {
        Error {
            message: message.into(),
            limit: true,
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn is_limit(&self) -> bool {
        self.limit
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::new(format!("io: {e}"))
    }
}

impl From<zip::result::ZipError> for Error {
    fn from(e: zip::result::ZipError) -> Self {
        Error::new(format!("zip: {e}"))
    }
}

impl From<quick_xml::Error> for Error {
    fn from(e: quick_xml::Error) -> Self {
        Error::new(format!("xml: {e}"))
    }
}
