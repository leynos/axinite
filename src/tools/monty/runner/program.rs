//! Resolution of the helper executable the supervisor launches.

use std::path::{Path, PathBuf};

use crate::config::EnvContext;

use super::RunError;

/// Name of the executable the parent looks for.
pub const HELPER_NAME: &str = "axinite-monty-helper";

/// Environment variable that overrides helper resolution.
pub const HELPER_ENV: &str = "AXINITE_MONTY_HELPER";

/// A resolved helper executable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelperProgram(PathBuf);

impl HelperProgram {
    /// Uses an explicit executable path.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// Resolves the helper from the ambient environment, then beside the
    /// parent executable.
    ///
    /// # Errors
    ///
    /// Returns [`RunError::HelperUnavailable`] when no executable can be
    /// resolved.
    pub fn resolve() -> Result<Self, RunError> {
        Self::resolve_from(&EnvContext::capture_ambient())
    }

    /// Resolves the helper from an environment snapshot, then beside the
    /// parent executable.
    ///
    /// The snapshot supplies [`HELPER_ENV`]; process environment reads stay
    /// behind [`EnvContext`] so a test can supply its own view.
    ///
    /// # Errors
    ///
    /// Returns [`RunError::HelperUnavailable`] when no executable can be
    /// resolved.
    pub fn resolve_from(env: &EnvContext) -> Result<Self, RunError> {
        if let Some(path) = env.get(HELPER_ENV) {
            // The plan requires a trusted absolute path: a relative override
            // would resolve against whatever directory the parent happens to
            // be in, which is not an authority the caller supplied.
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(RunError::HelperUnavailable);
            }
            return Ok(Self(path));
        }
        let parent = std::env::current_exe().map_err(|_| RunError::HelperUnavailable)?;
        let directory = parent.parent().ok_or(RunError::HelperUnavailable)?;
        let candidate = directory.join(HELPER_NAME);
        if candidate.is_file() {
            Ok(Self(candidate))
        } else {
            Err(RunError::HelperUnavailable)
        }
    }

    /// The executable path.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for helper resolution.

    use std::collections::HashMap;

    use super::{HELPER_ENV, HelperProgram};
    use crate::config::EnvContext;

    #[test]
    fn the_environment_override_wins_over_the_sibling_search() {
        let env = EnvContext::for_testing(
            HashMap::from([(
                HELPER_ENV.to_string(),
                "/nonexistent/axinite-monty-helper".to_string(),
            )]),
            HashMap::new(),
        );
        let program = HelperProgram::resolve_from(&env).expect("the override resolves");
        assert_eq!(
            program.path(),
            std::path::Path::new("/nonexistent/axinite-monty-helper")
        );
    }
}
