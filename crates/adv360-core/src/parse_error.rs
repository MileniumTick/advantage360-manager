//! Syntax errors found while reading a configuration line.
//!
//! A [`ParseError`] never means "discard the line". It means "this line starts like documented
//! syntax but is malformed", which is exactly the situation the Direct Programming Guide warns
//! about. The line is preserved byte for byte and the error blocks writing until it is resolved.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Machine-readable classification, so both interfaces can explain the failure consistently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyntaxCode {
    /// A bracket or brace was opened and never closed.
    UnterminatedGroup,
    /// An unexpected character outside any bracket group.
    StrayCharacter,
    /// The line mixed square and curly brackets, which the guide forbids.
    MixedBrackets,
    /// The `>` separator is missing between the position/trigger and the action.
    MissingSeparator,
    /// More than one `>` separator on a line.
    RepeatedSeparator,
    /// A position, trigger or action group is empty.
    EmptyGroup,
    /// A group is not a valid action token, position token or number where one is required.
    ExpectedToken,
    /// A number (delay, colour channel, tap-and-hold timing) is out of range or malformed.
    BadNumber,
    /// The Tap-and-Hold token `t&hNNN` is malformed.
    BadTapHold,
    /// A macro speed prefix `sN` is outside 1-9.
    BadSpeed,
    /// A macro multiplay prefix `xN` is outside 1-9.
    BadMultiplay,
    /// The LED colour list is not a whole number of RRR/GGG/BBB triples.
    BadColorTriple,
}

/// A malformed line, with the column where the problem starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseError {
    pub code: SyntaxCode,
    /// 0-based column inside the line content, when known.
    pub column: Option<usize>,
    /// What was expected.
    pub expected: String,
    /// What was found.
    pub found: String,
}

impl ParseError {
    pub fn new(
        code: SyntaxCode,
        column: Option<usize>,
        expected: impl Into<String>,
        found: impl Into<String>,
    ) -> Self {
        ParseError {
            code,
            column,
            expected: expected.into(),
            found: found.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "expected {}, found {}", self.expected, self.found)?;
        if let Some(c) = self.column {
            write!(f, " at column {}", c + 1)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_reports_column_one_based() {
        let e = ParseError::new(
            SyntaxCode::UnterminatedGroup,
            Some(0),
            "a closing ']'",
            "end of line",
        );
        assert_eq!(
            e.to_string(),
            "expected a closing ']', found end of line at column 1"
        );
    }

    #[test]
    fn column_is_optional() {
        let e = ParseError::new(SyntaxCode::MissingSeparator, None, "'>'", "end of line");
        assert_eq!(e.to_string(), "expected '>', found end of line");
    }
}
