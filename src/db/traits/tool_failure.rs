//! Tool failure persistence traits.
//!
//! Defines the dyn-safe [`ToolFailureStore`] and its native-async sibling
//! [`NativeToolFailureStore`] for tool failure tracking and analysis.

use core::future::Future;

use crate::agent::BrokenTool;
use crate::db::params::DbFuture;
use crate::error::DatabaseError;

/// Object-safe persistence surface for tool failure tracking and analysis.
///
/// This trait provides the dyn-safe boundary for tool failure storage
/// operations, enabling trait-object usage (e.g.,
/// `Arc<dyn ToolFailureStore>`).  It uses boxed futures ([`DbFuture`]) to
/// maintain object safety.
///
/// Companion trait: [`NativeToolFailureStore`] provides the same API using
/// native async traits (RPITIT).  A blanket adapter automatically bridges
/// implementations of `NativeToolFailureStore` to satisfy this trait.
///
/// Thread-safety: All implementations must be `Send + Sync` to support
/// concurrent access.
pub trait ToolFailureStore: Send + Sync {
    fn record_tool_failure<'a>(
        &'a self,
        tool_name: &'a str,
        error_message: &'a str,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn get_broken_tools<'a>(
        &'a self,
        threshold: i32,
    ) -> DbFuture<'a, Result<Vec<BrokenTool>, DatabaseError>>;
    fn mark_tool_repaired<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn increment_repair_attempts<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
}

/// Native async sibling trait for concrete tool-failure-store implementations.
pub trait NativeToolFailureStore: Send + Sync {
    fn record_tool_failure<'a>(
        &'a self,
        tool_name: &'a str,
        error_message: &'a str,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn get_broken_tools<'a>(
        &'a self,
        threshold: i32,
    ) -> impl Future<Output = Result<Vec<BrokenTool>, DatabaseError>> + Send + 'a;
    fn mark_tool_repaired<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn increment_repair_attempts<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
}
