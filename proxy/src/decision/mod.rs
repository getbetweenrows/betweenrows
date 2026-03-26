//! Decision function runtime for policy evaluation.
//!
//! A decision function is an optional JS function attached to a policy that gates
//! whether the policy's effect fires. The JS is compiled to WASM at save time via Javy,
//! and evaluated at query time via wasmtime.

pub mod context;
pub mod wasm;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Result of evaluating a decision function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionResult {
    /// Whether the policy should fire (apply its effect).
    pub fire: bool,
    /// Captured console.log output (only when debug=true on the policy).
    pub logs: Vec<String>,
    /// Number of WASM fuel instructions consumed.
    pub fuel_consumed: u64,
    /// Execution time in microseconds.
    pub time_us: u64,
    /// Error message if the function failed (on_error determines behavior).
    pub error: Option<String>,
}

/// Errors from the decision runtime.
#[derive(Debug)]
pub enum RuntimeError {
    /// JS compilation to WASM failed.
    CompilationError(String),
    /// WASM execution failed (e.g., fuel exhaustion, runtime trap).
    ExecutionError(String),
    /// The function returned an invalid result (not `{ fire: boolean }`).
    InvalidResult(String),
    /// Javy CLI not found or failed to execute.
    JavyError(String),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeError::CompilationError(msg) => write!(f, "Compilation error: {msg}"),
            RuntimeError::ExecutionError(msg) => write!(f, "Execution error: {msg}"),
            RuntimeError::InvalidResult(msg) => write!(f, "Invalid result: {msg}"),
            RuntimeError::JavyError(msg) => write!(f, "Javy error: {msg}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

/// Validation result from testing a decision function.
#[derive(Debug, Serialize)]
pub struct ValidationResult {
    pub success: bool,
    pub result: Option<DecisionResult>,
    pub error: Option<String>,
}

/// Trait for decision function runtimes (WASM, mock, etc.).
#[async_trait]
pub trait DecisionRuntime: Send + Sync {
    /// Compile JS source to WASM binary.
    async fn compile(&self, policy_id: &str, js_source: &str) -> Result<Vec<u8>, RuntimeError>;

    /// Evaluate a compiled WASM decision function.
    ///
    /// `log_level` controls stderr capture:
    /// - `"off"` — no log capture
    /// - `"error"` — capture stderr (exception messages)
    /// - `"info"` — capture stderr (all console.log output; Javy routes console.log to stderr)
    ///
    /// Note: `"error"` and `"info"` are functionally equivalent for now since Javy sends
    /// all console.log output to stderr. The distinction is semantic intent.
    async fn evaluate(
        &self,
        policy_id: &str,
        wasm_bytes: &[u8],
        context: &serde_json::Value,
        config: &serde_json::Value,
        fuel_limit: u64,
        log_level: &str,
    ) -> Result<DecisionResult, RuntimeError>;

    /// Validate a JS source by compiling and running it with a test context.
    async fn validate(
        &self,
        js_source: &str,
        test_context: &serde_json::Value,
        test_config: &serde_json::Value,
    ) -> Result<ValidationResult, RuntimeError>;
}
