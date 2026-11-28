use std::fmt;

/// What kind of thing went wrong, so a caller can react to it without reading the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The text is not an expression (an unexpected character, a missing bracket, an unclosed string).
    Syntax,
    /// The expression is nested more deeply than the language allows.
    TooDeep,
    /// A name the environment does not have, or a map key or list item that is not there.
    Undefined,
    /// A value of the wrong type (a condition that is not true or false, text where a number goes).
    Type,
    /// A number does not fit the 96-bit decimal type.
    Overflow,
    /// A division or remainder by zero.
    DivisionByZero,
    /// With [`Precision::Exact`](crate::Precision::Exact): a result needs more than 28 decimal places (or more
    /// significant digits than the number type holds), so it cannot be kept exactly.
    Precision,
    /// A function was called with the wrong number of arguments.
    Arity,
    /// A function was given a value it cannot work with (a bracket table whose limits go down, digits above 10).
    InvalidArgument,
    /// There is no function with that name.
    UnknownFunction,
    /// The host's own function (see [`Env::call`](crate::Env::call)) failed.
    Host,
}

/// What went wrong reading or running an expression, and where (a byte offset in the source when known).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Error {
    /// The kind of failure, for code that reacts to it.
    pub kind: ErrorKind,
    /// A description for people.
    pub message: String,
    /// The byte offset in the source the failure belongs to, when known.
    pub at: Option<usize>,
}

/// The result of reading or running an expression.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// An error with no position.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), at: None }
    }

    /// An error at a byte offset of the source.
    pub fn at(kind: ErrorKind, message: impl Into<String>, at: usize) -> Self {
        Self { kind, message: message.into(), at: Some(at) }
    }

    pub(crate) fn syntax(message: impl Into<String>, at: usize) -> Self {
        Self::at(ErrorKind::Syntax, message, at)
    }

    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidArgument, message)
    }

    pub(crate) fn overflow() -> Self {
        Self::new(ErrorKind::Overflow, "the number is too large")
    }

    pub(crate) fn kind_of_type(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Type, message)
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
