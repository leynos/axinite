//! WIT version compatibility checking between extensions and the host.

use super::WasmLoadError;

/// Check that a declared WIT version is compatible with the host WIT version.
///
/// Compatibility rules (semver):
/// - Same major version required (0.x is special: same minor required)
/// - Extension WIT version must not be greater than host version
///
/// If `declared` is `None`, the check is skipped (pre-versioning extension).
pub fn check_wit_version_compat(
    name: &str,
    declared: Option<&str>,
    host_version: &str,
) -> Result<(), WasmLoadError> {
    let Some(declared_str) = declared else {
        return Ok(());
    };

    let declared = semver::Version::parse(declared_str).map_err(|e| {
        WasmLoadError::WitVersionMismatch(format!(
            "Extension '{name}' has invalid wit_version '{declared_str}': {e}"
        ))
    })?;

    let host = semver::Version::parse(host_version).map_err(|e| {
        WasmLoadError::WitVersionMismatch(format!(
            "Host WIT version '{host_version}' is invalid: {e}"
        ))
    })?;

    // Major version must match
    if declared.major != host.major {
        return Err(WasmLoadError::WitVersionMismatch(format!(
            "Extension '{name}' compiled against WIT {declared}, but host supports WIT {host}. \
             Major version mismatch — rebuild the extension."
        )));
    }

    // For 0.x versions, minor must also match (semver: 0.x.y has no compatibility guarantees)
    if declared.major == 0 && declared.minor != host.minor {
        return Err(WasmLoadError::WitVersionMismatch(format!(
            "Extension '{name}' compiled against WIT {declared}, but host supports WIT {host}. \
             Rebuild the extension against the current WIT."
        )));
    }

    // Extension cannot be newer than host
    if declared > host {
        return Err(WasmLoadError::WitVersionMismatch(format!(
            "Extension '{name}' compiled against WIT {declared}, but host only supports WIT {host}. \
             Update the host or rebuild with an older WIT."
        )));
    }

    Ok(())
}
