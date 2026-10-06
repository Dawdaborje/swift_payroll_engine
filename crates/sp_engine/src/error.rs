use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// The structure is wrong: found before any payroll runs.
    Structure(String),
    /// A person's input is wrong or incomplete.
    Input { employee: String, message: String },
    /// A formula failed for a person.
    Component { employee: String, component: String, message: String },
    /// The pay is negative and the structure says that is an error.
    NegativeNet { employee: String, net: String },
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Structure(m) => write!(f, "the salary structure is not valid: {m}"),
            EngineError::Input { employee, message } => write!(f, "{employee}: {message}"),
            EngineError::Component { employee, component, message } => write!(f, "{employee}: {component}: {message}"),
            EngineError::NegativeNet { employee, net } => write!(f, "{employee}: net pay would be {net}, below zero"),
        }
    }
}

impl std::error::Error for EngineError {}
