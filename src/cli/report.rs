//! Line-oriented report builder shared by the read-only CLI commands.
//!
//! Commands stream their output through a [`Report`] so an operator sees each
//! diagnostic as soon as it is produced rather than after every probe finishes.
//! Tests render into an in-memory buffer and assert on the exact text a command
//! produces from a supplied environment snapshot.

/// Streams command output to an injected writer.
///
/// Every line is flushed immediately, so a slow later check cannot hold back a
/// diagnostic that has already been produced.
#[derive(Debug)]
pub(crate) struct Report<W> {
    writer: W,
}

impl<W: std::io::Write> Report<W> {
    /// Wrap `writer` in a report that flushes after every write.
    pub(crate) fn new(writer: W) -> Self {
        Self { writer }
    }

    /// Append formatted text without a trailing newline.
    pub(crate) fn part(&mut self, args: std::fmt::Arguments<'_>) {
        let _ = write!(self.writer, "{args}");
        let _ = self.writer.flush();
    }

    /// Append a formatted line.
    pub(crate) fn line(&mut self, args: std::fmt::Arguments<'_>) {
        let _ = writeln!(self.writer, "{args}");
        let _ = self.writer.flush();
    }
}

impl Report<Vec<u8>> {
    /// Consume a byte-backed report and return the rendered text.
    #[cfg(test)]
    pub(crate) fn into_string(self) -> String {
        String::from_utf8_lossy(&self.writer).into_owned()
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
