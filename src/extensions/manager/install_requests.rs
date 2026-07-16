//! Request types for WASM extension installation.

use crate::extensions::ExtensionKind;

/// A build-from-source WASM install request.
pub(super) struct BuildableInstall<'a> {
    /// Extension name to install under.
    pub name: &'a str,
    /// Build directory relative to the manifest dir, or absolute.
    pub build_dir: Option<&'a str>,
    /// Crate name when it differs from the extension name.
    pub crate_name: Option<&'a str>,
    /// Whether the artifact is a tool or a channel.
    pub kind: ExtensionKind,
}

/// Inputs for downloading and installing a WASM extension bundle.
///
/// Groups the extension name, download URLs, and install directory so the
/// installer need not thread four positional arguments.
pub(super) struct WasmDownloadRequest<'a> {
    /// Logical extension name; determines the installed file basenames.
    pub name: &'a str,
    /// HTTPS URL of the `.wasm` file or tar.gz bundle to download.
    pub url: &'a str,
    /// Optional separate capabilities-file URL for bare `.wasm` downloads.
    pub capabilities_url: Option<&'a str>,
    /// Extension kind, selecting the install directory and result message.
    pub kind: ExtensionKind,
}
