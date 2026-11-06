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
    /// Record one failure for `tool_name`, inserting it or raising its failure count.
    ///
    /// Recording a failure also clears any earlier repaired marker for the tool.
    fn record_tool_failure<'a>(
        &'a self,
        tool_name: &'a str,
        error_message: &'a str,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// List unrepaired tools whose failure count is at least `threshold`, most failures first.
    fn get_broken_tools<'a>(
        &'a self,
        threshold: i32,
    ) -> DbFuture<'a, Result<Vec<BrokenTool>, DatabaseError>>;
    /// Fetch one unrepaired broken tool by name, or `None` if it is absent or repaired.
    fn get_broken_tool_by_name<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> DbFuture<'a, Result<Option<BrokenTool>, DatabaseError>>;
    /// Mark `tool_name` as repaired and reset its failure count to zero.
    fn mark_tool_repaired<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Count one more repair attempt against `tool_name`.
    fn increment_repair_attempts<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
}

/// Native async sibling trait for concrete tool-failure-store implementations.
pub trait NativeToolFailureStore: Send + Sync {
    /// Record one failure for `tool_name`, inserting it or raising its failure count.
    ///
    /// Recording a failure also clears any earlier repaired marker for the tool.
    fn record_tool_failure<'a>(
        &'a self,
        tool_name: &'a str,
        error_message: &'a str,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// List unrepaired tools whose failure count is at least `threshold`, most failures first.
    fn get_broken_tools<'a>(
        &'a self,
        threshold: i32,
    ) -> impl Future<Output = Result<Vec<BrokenTool>, DatabaseError>> + Send + 'a;
    /// Fetch one unrepaired broken tool by name, or `None` if it is absent or repaired.
    fn get_broken_tool_by_name<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> impl Future<Output = Result<Option<BrokenTool>, DatabaseError>> + Send + 'a;
    /// Mark `tool_name` as repaired and reset its failure count to zero.
    fn mark_tool_repaired<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// Count one more repair attempt against `tool_name`.
    fn increment_repair_attempts<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
}
