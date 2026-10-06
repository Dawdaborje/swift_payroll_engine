use std::fmt;

/// What went wrong reading or running an expression, and where (a byte offset in the source when known).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub message: String,
    pub at: Option<usize>,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), at: None }
    }

    pub fn at(message: impl Into<String>, at: usize) -> Self {
        Self { message: message.into(), at: Some(at) }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.at {
            Some(at) => write!(f, "{} (at {at})", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for Error {}
