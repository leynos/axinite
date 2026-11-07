//! Line-oriented report builder shared by the read-only CLI commands.
//!
//! Commands accumulate their output into a [`Report`] so tests can assert on
//! the exact text a command renders from a supplied environment snapshot.

/// Accumulates command output before it is printed.
#[derive(Debug, Default)]
pub(crate) struct Report {
    text: String,
}

impl Report {
    /// Append formatted text without a trailing newline.
    pub(crate) fn part(&mut self, args: std::fmt::Arguments<'_>) {
        use std::fmt::Write as _;
        let _ = write!(self.text, "{args}");
    }

    /// Append a formatted line.
    pub(crate) fn line(&mut self, args: std::fmt::Arguments<'_>) {
        use std::fmt::Write as _;
        let _ = writeln!(self.text, "{args}");
    }

    /// Consume the report and return its accumulated text.
    pub(crate) fn into_string(self) -> String {
        self.text
    }
}

/// Append formatted text to a [`Report`] without a trailing newline.
macro_rules! out {
    ($report:expr, $($arg:tt)*) => {
        $report.part(format_args!($($arg)*))
    };
}

/// Append a formatted line to a [`Report`].
macro_rules! outln {
    ($report:expr, $($arg:tt)*) => {
        $report.line(format_args!($($arg)*))
    };
}

pub(crate) use out;
pub(crate) use outln;
