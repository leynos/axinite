//! End-to-end OpenClaw import tests over synthetic directory data.
//!
//! Split into `helpers` (synthetic data builders), `reader` (OpenClaw
//! reader behaviour), and `import_types` (options, stats, and errors).

#[path = "helpers.rs"]
mod helpers;

#[path = "import_types.rs"]
mod import_types;
#[path = "reader.rs"]
mod reader;
