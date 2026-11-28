use std::fmt;

use rust_decimal::Decimal;

/// Why a component could not be calculated for a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ComponentErrorKind {
    /// Its formula or condition failed; the expression error's kind says how.
    Expression(swift_payroll_expr::ErrorKind),
    /// Its formula gave something other than a number.
    NotANumber,
    /// Its condition gave something other than true or false.
    NotACondition,
    /// The input it reads is not a number.
    InputNotANumber,
    /// It came to a negative amount and `allow_negative` is not set.
    Negative,
    /// Its amount is too large to keep the decimal places its rounding asks for.
    TooLarge,
}

/// Which total did not fit the number type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Total {
    /// The sum of the earnings.
    Gross,
    /// The sum of the deductions.
    Deductions,
    /// The sum of the employer contributions.
    EmployerCost,
    /// Gross less deductions.
    Net,
}

impl Total {
    fn describe(self) -> &'static str {
        match self {
            Total::Gross => "gross pay",
            Total::Deductions => "deductions",
            Total::EmployerCost => "employer cost",
            Total::Net => "net pay",
        }
    }
}

/// What went wrong compiling a structure or calculating a person.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EngineError {
    /// The structure is wrong: found before any payroll runs.
    Structure {
        /// The component at fault, when there is one.
        component: Option<String>,
        /// What is wrong.
        message: String,
    },
    /// A person's input is wrong or incomplete.
    Input {
        /// Who.
        employee: String,
        /// The input at fault, when there is one.
        input: Option<String>,
        /// What is wrong.
        message: String,
    },
    /// A component failed for a person.
    Component {
        /// Who.
        employee: String,
        /// Which component.
        component: String,
        /// How it failed.
        kind: ComponentErrorKind,
        /// What is wrong.
        message: String,
    },
    /// The pay is negative and the structure says that is an error.
    NegativeNet {
        /// Who.
        employee: String,
        /// What net pay would have been.
        net: Decimal,
    },
    /// A total does not fit the number type.
    TooLarge {
        /// Who.
        employee: String,
        /// Which total.
        total: Total,
    },
}

impl EngineError {
    /// The person the error is about, when it is about one.
    pub fn employee(&self) -> Option<&str> {
        match self {
            EngineError::Structure { .. } => None,
            EngineError::Input { employee, .. }
            | EngineError::Component { employee, .. }
            | EngineError::NegativeNet { employee, .. }
            | EngineError::TooLarge { employee, .. } => Some(employee),
        }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Structure { message, .. } => write!(f, "the salary structure is not valid: {message}"),
            EngineError::Input { employee, message, .. } if employee.trim().is_empty() => f.write_str(message),
            EngineError::Input { employee, message, .. } => write!(f, "{employee}: {message}"),
            EngineError::Component { employee, component, message, .. } => write!(f, "{employee}: {component}: {message}"),
            EngineError::NegativeNet { employee, net } => write!(f, "{employee}: net pay would be {net}, below zero"),
            EngineError::TooLarge { employee, total } => write!(f, "{employee}: the {} is too large to hold", total.describe()),
        }
    }
}

impl std::error::Error for EngineError {}
