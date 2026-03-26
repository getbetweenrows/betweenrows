//! WASM-based decision function runtime using wasmtime + Javy (dynamic mode).
//!
//! - JS source → bytecode WASM: compiled via `javy build -C dynamic` (1-16 KB output).
//! - QuickJS engine plugin (~869 KB) compiled once at startup from `PLUGIN_WASM`.
//! - WASM evaluation: two-module linking (plugin + bytecode) with fuel-based limits.
//! - Console.log → stdout (fd 1) in Javy 8.x; `parse_stdout_result()` extracts JSON result from last line.
//! - Fresh WASM instance per evaluation (no cross-policy state leakage).

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::OnceLock;
use wasmtime::{Config, Engine, Linker, Module, Store};

use super::{DecisionResult, DecisionRuntime, RuntimeError, ValidationResult};

/// Default fuel limit: 1,000,000 WASM instructions.
pub const DEFAULT_FUEL_LIMIT: u64 = 1_000_000;

/// Embedded QuickJS engine plugin WASM, emitted by `javy emit-plugin` at build time.
pub const PLUGIN_WASM: &[u8] = include_bytes!(env!("JAVY_PLUGIN_PATH"));

/// Write the plugin WASM to a temp file for the Javy CLI `-C plugin=<path>` flag.
pub fn plugin_file_path() -> &'static std::path::Path {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let path = std::env::temp_dir().join("br_javy_engine.wasm");
        if !path.exists() {
            std::fs::write(&path, PLUGIN_WASM).expect("write plugin to temp");
        }
        path
    })
}

/// WASM decision runtime backed by wasmtime + Javy (dynamic mode).
///
/// The QuickJS engine plugin is compiled once at construction. Per-function
/// bytecode modules (1-16 KB) compile in ~1ms and don't need caching.
pub struct WasmDecisionRuntime {
    engine: Engine,
    /// Pre-compiled QuickJS engine plugin module.
    plugin_module: Module,
}

impl WasmDecisionRuntime {
    /// Create a new WASM runtime. Call once at server startup.
    pub fn new() -> Result<Self, RuntimeError> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.wasm_memory64(false);

        let engine = Engine::new(&config).map_err(|e| {
            RuntimeError::ExecutionError(format!("Failed to create wasmtime engine: {e}"))
        })?;

        let plugin_module = Module::new(&engine, PLUGIN_WASM).map_err(|e| {
            RuntimeError::ExecutionError(format!("Failed to compile QuickJS plugin: {e}"))
        })?;

        Ok(Self {
            engine,
            plugin_module,
        })
    }
}

/// Wrapper for Javy WASM module evaluation.
/// The Javy-compiled module expects:
/// - stdin (fd 0): JSON input `{ "ctx": ..., "config": ... }`
/// - stdout (fd 1): JSON output `{ "fire": true/false }`
/// - stderr (fd 2): console.log output (line-delimited)
struct WasiState {
    stdin: Vec<u8>,
    stdin_pos: usize,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Resolve the Javy CLI path: compile-time path from build.rs, then PATH fallback.
pub fn javy_cli_path() -> &'static str {
    // Set by build.rs via cargo:rustc-env
    const BUILD_TIME_PATH: &str = env!("JAVY_CLI_PATH");

    // At runtime, prefer BR_JAVY_CLI_PATH env var (for Docker), then build-time path, then PATH
    static RESOLVED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    RESOLVED.get_or_init(|| {
        if let Ok(p) = std::env::var("BR_JAVY_CLI_PATH")
            && std::path::Path::new(&p).exists()
        {
            return p;
        }
        if std::path::Path::new(BUILD_TIME_PATH).exists() {
            return BUILD_TIME_PATH.to_string();
        }
        // Fallback to PATH lookup
        "javy".to_string()
    })
}

/// Compile JS source to bytecode WASM via Javy CLI in dynamic mode.
///
/// Dynamic mode produces a small bytecode module (1-16 KB) that imports
/// the QuickJS engine from a separate plugin module, rather than embedding
/// the full engine (~869 KB) in every compiled function.
pub async fn compile_with_javy(js_source: &str) -> Result<Vec<u8>, RuntimeError> {
    // Write JS to a temp file
    let tmp_dir = std::env::temp_dir();
    let unique = uuid::Uuid::now_v7();
    let input_path = tmp_dir.join(format!("br_decision_{unique}.js"));
    let output_path = tmp_dir.join(format!("br_decision_{unique}.wasm"));

    // Wrap user code in a strict-mode IIFE to prevent global variable leaks,
    // then add the Javy harness that reads stdin/writes stdout.
    // Javy v8 IO API: readSync(fd, buffer) returns bytesRead; writeSync(fd, buffer).
    // NOTE: Javy 8.x routes console.log to stdout (fd 1) by default.
    // The runtime's parse_stdout_result() handles this by extracting the
    // JSON result from the last line, treating prior lines as log output.
    // If we ever want to redirect console.log to stderr at the JS level,
    // we can prepend a console override IIFE here before the user code:
    //   console.log = (...args) => Javy.IO.writeSync(2, new TextEncoder().encode(args.join(' ')+'\n'));
    // This would require recompilation of existing decision functions.
    let wrapped_js = format!(
        r#"var evaluate = (function() {{
    "use strict";
    {js_source}
    if (typeof evaluate !== 'function') {{
        throw new Error('Decision function must define an evaluate(ctx, config) function');
    }}
    return evaluate;
}})();

// Javy harness: read stdin JSON, call evaluate(), write result to stdout
function __br_readStdin() {{
    const chunks = [];
    let total = 0;
    while (true) {{
        const buf = new Uint8Array(4096);
        const n = Javy.IO.readSync(0, buf);
        if (n === 0) break;
        chunks.push(buf.subarray(0, n));
        total += n;
    }}
    const all = new Uint8Array(total);
    let off = 0;
    for (const c of chunks) {{ all.set(c, off); off += c.length; }}
    return all;
}}

const input = JSON.parse(new TextDecoder().decode(__br_readStdin()));
const result = evaluate(input.ctx, input.config);

// Validate result shape
if (typeof result !== 'object' || result === null || typeof result.fire !== 'boolean') {{
    throw new Error('Decision function must return {{ fire: boolean }}, got: ' + JSON.stringify(result));
}}

const output = JSON.stringify(result);
Javy.IO.writeSync(1, new TextEncoder().encode(output));
"#
    );

    tokio::fs::write(&input_path, &wrapped_js)
        .await
        .map_err(|e| RuntimeError::JavyError(format!("Failed to write temp JS file: {e}")))?;

    let javy = javy_cli_path();
    let plugin_path = plugin_file_path();

    // Run javy build in dynamic mode — produces bytecode-only WASM (1-16 KB)
    let output = tokio::process::Command::new(javy)
        .arg("build")
        .arg("-C")
        .arg("dynamic")
        .arg("-C")
        .arg(format!("plugin={}", plugin_path.display()))
        .arg("-o")
        .arg(&output_path)
        .arg(&input_path)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                RuntimeError::JavyError(format!(
                    "Javy CLI not found at '{javy}'. Rebuild the project to trigger build.rs download."
                ))
            } else {
                RuntimeError::JavyError(format!("Failed to run Javy CLI: {e}"))
            }
        })?;

    // Clean up input file
    let _ = tokio::fs::remove_file(&input_path).await;

    if !output.status.success() {
        let _ = tokio::fs::remove_file(&output_path).await;
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RuntimeError::CompilationError(format!(
            "Javy compilation failed: {stderr}"
        )));
    }

    // Read compiled WASM
    let wasm_bytes = tokio::fs::read(&output_path)
        .await
        .map_err(|e| RuntimeError::JavyError(format!("Failed to read compiled WASM: {e}")))?;

    let _ = tokio::fs::remove_file(&output_path).await;

    Ok(wasm_bytes)
}

/// Evaluate a dynamic-mode Javy WASM module with the given context and config.
/// Public sync version for use in PolicyEffects::collect() (sync context).
///
/// `plugin_module`: pre-compiled QuickJS engine plugin (compiled once at startup).
/// `bytecode_module`: per-function bytecode module (1-16 KB, compiled on the fly).
/// `log_level`: `"off"` = no capture, `"error"` / `"info"` = capture stderr + console.log from stdout.
pub fn evaluate_wasm_sync(
    engine: &Engine,
    plugin_module: &Module,
    bytecode_module: &Module,
    context: &serde_json::Value,
    config: &serde_json::Value,
    fuel_limit: u64,
    log_level: &str,
) -> Result<super::DecisionResult, super::RuntimeError> {
    evaluate_wasm(
        engine,
        plugin_module,
        bytecode_module,
        context,
        config,
        fuel_limit,
        log_level,
    )
}

/// Parse the JSON result from stdout, handling console.log output that may precede it.
///
/// The harness writes the result as the last thing to stdout. In Javy 8.x,
/// `console.log` also writes to stdout (fd 1), so earlier lines may be log output.
/// Returns (parsed_json, console_log_lines).
fn parse_stdout_result(stdout_str: &str) -> Result<(serde_json::Value, Vec<String>), RuntimeError> {
    // Fast path: try parsing the whole stdout as JSON (no console.log output)
    if let Ok(result) = serde_json::from_str::<serde_json::Value>(stdout_str)
        && result.get("fire").is_some()
    {
        return Ok((result, vec![]));
    }

    // Slow path: console.log output preceded the JSON result.
    // Scan lines in reverse to find the last valid JSON with a "fire" key.
    let lines: Vec<&str> = stdout_str.lines().collect();
    for (i, line) in lines.iter().enumerate().rev() {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line)
            && val.get("fire").is_some()
        {
            let log_lines: Vec<String> = lines[..i]
                .iter()
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect();
            return Ok((val, log_lines));
        }
    }

    Err(RuntimeError::InvalidResult(format!(
        "Failed to parse decision function output as JSON. Output: {stdout_str}"
    )))
}

/// Evaluate a dynamic-mode Javy WASM module (two-module linking).
fn evaluate_wasm(
    engine: &Engine,
    plugin_module: &Module,
    bytecode_module: &Module,
    context: &serde_json::Value,
    config: &serde_json::Value,
    fuel_limit: u64,
    log_level: &str,
) -> Result<DecisionResult, RuntimeError> {
    let start = std::time::Instant::now();

    // Prepare input JSON
    let input = serde_json::json!({
        "ctx": context,
        "config": config,
    });
    let input_bytes = serde_json::to_vec(&input)
        .map_err(|e| RuntimeError::ExecutionError(format!("Failed to serialize input: {e}")))?;

    // Create WASI-like store with stdin/stdout/stderr
    let mut store = Store::new(
        engine,
        WasiState {
            stdin: input_bytes,
            stdin_pos: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
        },
    );

    store
        .set_fuel(fuel_limit)
        .map_err(|e| RuntimeError::ExecutionError(format!("Failed to set fuel: {e}")))?;

    // Link WASI-like functions that Javy expects
    let mut linker = Linker::new(engine);

    // Javy IO uses javy_quickjs_provider_v3 which imports from "javy_quickjs_provider_v3"
    // But the standard Javy compile uses WASI preview 1 imports.
    // We need to provide fd_read, fd_write, etc.
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "fd_read",
            |mut caller: wasmtime::Caller<'_, WasiState>,
             fd: i32,
             iovs_ptr: i32,
             iovs_len: i32,
             nread_ptr: i32|
             -> i32 {
                if fd != 0 {
                    return 8; // EBADF
                }
                let memory = match caller.get_export("memory") {
                    Some(wasmtime::Extern::Memory(m)) => m,
                    _ => return 8,
                };

                // First pass: read iov entries from memory
                let mut iovs = Vec::new();
                {
                    let data = memory.data(&caller);
                    for i in 0..iovs_len {
                        let iov_offset = (iovs_ptr + i * 8) as usize;
                        if iov_offset + 8 > data.len() {
                            return 21; // EINVAL
                        }
                        let buf_ptr = u32::from_le_bytes(
                            data[iov_offset..iov_offset + 4].try_into().unwrap(),
                        ) as usize;
                        let buf_len = u32::from_le_bytes(
                            data[iov_offset + 4..iov_offset + 8].try_into().unwrap(),
                        ) as usize;
                        iovs.push((buf_ptr, buf_len));
                    }
                }

                // Copy stdin data to each iov buffer
                let mut total_read = 0u32;
                for (buf_ptr, buf_len) in iovs {
                    let stdin_pos = caller.data().stdin_pos;
                    let stdin_len = caller.data().stdin.len();
                    let remaining = stdin_len.saturating_sub(stdin_pos);
                    let to_read = remaining.min(buf_len);
                    if to_read > 0 {
                        let chunk: Vec<u8> =
                            caller.data().stdin[stdin_pos..stdin_pos + to_read].to_vec();
                        caller.data_mut().stdin_pos += to_read;
                        let data = memory.data_mut(&mut caller);
                        if buf_ptr + to_read <= data.len() {
                            data[buf_ptr..buf_ptr + to_read].copy_from_slice(&chunk);
                        }
                        total_read += to_read as u32;
                    }
                }

                // Write nread
                let data = memory.data_mut(&mut caller);
                let nread_offset = nread_ptr as usize;
                if nread_offset + 4 <= data.len() {
                    data[nread_offset..nread_offset + 4].copy_from_slice(&total_read.to_le_bytes());
                }
                0 // success
            },
        )
        .map_err(|e| RuntimeError::ExecutionError(format!("Failed to link fd_read: {e}")))?;

    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "fd_write",
            |mut caller: wasmtime::Caller<'_, WasiState>,
             fd: i32,
             iovs_ptr: i32,
             iovs_len: i32,
             nwritten_ptr: i32|
             -> i32 {
                if fd != 1 && fd != 2 {
                    return 8; // EBADF
                }
                let memory = match caller.get_export("memory") {
                    Some(wasmtime::Extern::Memory(m)) => m,
                    _ => return 8,
                };

                // First pass: collect data from iov buffers
                let mut collected = Vec::new();
                {
                    let data = memory.data(&caller);
                    for i in 0..iovs_len {
                        let iov_offset = (iovs_ptr + i * 8) as usize;
                        if iov_offset + 8 > data.len() {
                            return 21; // EINVAL
                        }
                        let buf_ptr = u32::from_le_bytes(
                            data[iov_offset..iov_offset + 4].try_into().unwrap(),
                        ) as usize;
                        let buf_len = u32::from_le_bytes(
                            data[iov_offset + 4..iov_offset + 8].try_into().unwrap(),
                        ) as usize;
                        if buf_ptr + buf_len <= data.len() {
                            collected.extend_from_slice(&data[buf_ptr..buf_ptr + buf_len]);
                        }
                    }
                }

                let total_written = collected.len() as u32;
                if fd == 1 {
                    caller.data_mut().stdout.extend_from_slice(&collected);
                } else {
                    caller.data_mut().stderr.extend_from_slice(&collected);
                }

                // Write nwritten
                let data = memory.data_mut(&mut caller);
                let nw_offset = nwritten_ptr as usize;
                if nw_offset + 4 <= data.len() {
                    data[nw_offset..nw_offset + 4].copy_from_slice(&total_written.to_le_bytes());
                }
                0 // success
            },
        )
        .map_err(|e| RuntimeError::ExecutionError(format!("Failed to link fd_write: {e}")))?;

    // Stub out other WASI functions that Javy may import
    linker
        .func_wrap("wasi_snapshot_preview1", "fd_close", |_fd: i32| -> i32 {
            0
        })
        .map_err(|e| RuntimeError::ExecutionError(format!("Failed to link fd_close: {e}")))?;
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "fd_seek",
            |_fd: i32, _offset: i64, _whence: i32, _new_offset_ptr: i32| -> i32 { 0 },
        )
        .map_err(|e| RuntimeError::ExecutionError(format!("Failed to link fd_seek: {e}")))?;
    linker
        .func_wrap("wasi_snapshot_preview1", "proc_exit", |_code: i32| {})
        .map_err(|e| RuntimeError::ExecutionError(format!("Failed to link proc_exit: {e}")))?;
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "environ_get",
            |_environ: i32, _environ_buf: i32| -> i32 { 0 },
        )
        .map_err(|e| RuntimeError::ExecutionError(format!("Failed to link environ_get: {e}")))?;
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "environ_sizes_get",
            |mut caller: wasmtime::Caller<'_, WasiState>, count_ptr: i32, size_ptr: i32| -> i32 {
                let memory = match caller.get_export("memory") {
                    Some(wasmtime::Extern::Memory(m)) => m,
                    _ => return 8,
                };
                let data = memory.data_mut(&mut caller);
                let cp = count_ptr as usize;
                let sp = size_ptr as usize;
                if cp + 4 <= data.len() {
                    data[cp..cp + 4].copy_from_slice(&0u32.to_le_bytes());
                }
                if sp + 4 <= data.len() {
                    data[sp..sp + 4].copy_from_slice(&0u32.to_le_bytes());
                }
                0
            },
        )
        .map_err(|e| {
            RuntimeError::ExecutionError(format!("Failed to link environ_sizes_get: {e}"))
        })?;
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "clock_time_get",
            |mut caller: wasmtime::Caller<'_, WasiState>,
             _id: i32,
             _precision: i64,
             time_ptr: i32|
             -> i32 {
                let memory = match caller.get_export("memory") {
                    Some(wasmtime::Extern::Memory(m)) => m,
                    _ => return 8,
                };
                let data = memory.data_mut(&mut caller);
                let tp = time_ptr as usize;
                if tp + 8 <= data.len() {
                    // Write 0 as the timestamp (u64 little-endian)
                    data[tp..tp + 8].copy_from_slice(&0u64.to_le_bytes());
                }
                0
            },
        )
        .map_err(|e| RuntimeError::ExecutionError(format!("Failed to link clock_time_get: {e}")))?;
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "random_get",
            |mut caller: wasmtime::Caller<'_, WasiState>, buf: i32, len: i32| -> i32 {
                let memory = match caller.get_export("memory") {
                    Some(wasmtime::Extern::Memory(m)) => m,
                    _ => return 8,
                };
                let data = memory.data_mut(&mut caller);
                let start = buf as usize;
                let end = start + len as usize;
                if end <= data.len() {
                    // Fill with zeros (deterministic, non-cryptographic)
                    data[start..end].fill(0);
                }
                0
            },
        )
        .map_err(|e| RuntimeError::ExecutionError(format!("Failed to link random_get: {e}")))?;
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "fd_fdstat_get",
            |mut caller: wasmtime::Caller<'_, WasiState>, fd: i32, stat_ptr: i32| -> i32 {
                let memory = match caller.get_export("memory") {
                    Some(wasmtime::Extern::Memory(m)) => m,
                    _ => return 8,
                };
                let data = memory.data_mut(&mut caller);
                let sp = stat_ptr as usize;
                // fdstat struct is 24 bytes: filetype(1) + pad(1) + flags(2) + rights_base(8) + rights_inheriting(8)
                if sp + 24 <= data.len() {
                    data[sp..sp + 24].fill(0);
                    // Set filetype based on fd
                    data[sp] = if fd <= 2 { 2 } else { 0 }; // 2 = character device for stdio
                }
                0
            },
        )
        .map_err(|e| RuntimeError::ExecutionError(format!("Failed to link fd_fdstat_get: {e}")))?;

    // Dynamic two-module linking:
    // 1. Instantiate the plugin (QuickJS engine) — gets WASI imports from linker
    let plugin_instance = linker.instantiate(&mut store, plugin_module).map_err(|e| {
        RuntimeError::ExecutionError(format!("Failed to instantiate plugin module: {e}"))
    })?;

    // 2. Register plugin exports under the namespace the bytecode module imports from.
    //    Javy 8.x uses "javy-default-plugin-v3" as the import module name.
    linker
        .instance(&mut store, "javy-default-plugin-v3", plugin_instance)
        .map_err(|e| {
            RuntimeError::ExecutionError(format!(
                "Failed to register plugin as javy-default-plugin-v3: {e}"
            ))
        })?;

    // 3. Instantiate bytecode module (imports eval_bytecode/memory from plugin)
    let bytecode_instance = linker
        .instantiate(&mut store, bytecode_module)
        .map_err(|e| {
            RuntimeError::ExecutionError(format!("Failed to instantiate bytecode module: {e}"))
        })?;

    // 4. Call _start on the bytecode instance
    let start_func = bytecode_instance
        .get_typed_func::<(), ()>(&mut store, "_start")
        .map_err(|e| {
            RuntimeError::ExecutionError(format!("Bytecode module missing _start export: {e}"))
        })?;

    let exec_result = start_func.call(&mut store, ());

    let elapsed = start.elapsed();
    let fuel_remaining = store.get_fuel().unwrap_or(0);
    let fuel_consumed = fuel_limit.saturating_sub(fuel_remaining);

    // Collect logs from stderr AND any console.log output on stdout.
    // In Javy 8.x, console.log writes to stdout (fd 1) by default.
    // We handle this by:
    // 1. Treating all non-JSON lines on stdout as console output (added to logs)
    // 2. Parsing only the last JSON line on stdout as the result (via parse_stdout_result)
    let mut logs: Vec<String> = if log_level != "off" {
        let stderr_str = String::from_utf8_lossy(&store.data().stderr);
        stderr_str
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect()
    } else {
        vec![]
    };

    match exec_result {
        Ok(()) => {
            let stdout = &store.data().stdout;
            let stdout_str = String::from_utf8_lossy(stdout);

            // Find the JSON result: the harness writes it last via Javy.IO.writeSync(1, ...).
            // Any prior lines are console.log output that leaked to stdout.
            let (result, stdout_logs) = parse_stdout_result(&stdout_str)?;

            if log_level != "off" {
                logs.extend(stdout_logs);
            }

            let fire = result
                .get("fire")
                .and_then(|v| v.as_bool())
                .ok_or_else(|| {
                    RuntimeError::InvalidResult(format!(
                        "Decision function must return {{ fire: boolean }}, got: {result}"
                    ))
                })?;

            Ok(DecisionResult {
                fire,
                logs,
                fuel_consumed,
                time_us: elapsed.as_micros() as u64,
                error: None,
            })
        }
        Err(e) => {
            let stderr_output = String::from_utf8_lossy(&store.data().stderr);
            let error_msg = if e.to_string().contains("fuel") {
                format!("Decision function exceeded fuel limit ({fuel_limit} instructions)")
            } else if !stderr_output.is_empty() {
                format!("Decision function execution failed: {stderr_output}")
            } else {
                format!("Decision function execution failed: {e}")
            };

            Err(RuntimeError::ExecutionError(error_msg))
        }
    }
}

#[async_trait]
impl DecisionRuntime for WasmDecisionRuntime {
    async fn compile(&self, _policy_id: &str, js_source: &str) -> Result<Vec<u8>, RuntimeError> {
        compile_with_javy(js_source).await
    }

    async fn evaluate(
        &self,
        _policy_id: &str,
        wasm_bytes: &[u8],
        context: &serde_json::Value,
        config: &serde_json::Value,
        fuel_limit: u64,
        log_level: &str,
    ) -> Result<DecisionResult, RuntimeError> {
        // Compile the small bytecode module (~1ms for 1-16 KB)
        let bytecode_module = Module::new(&self.engine, wasm_bytes).map_err(|e| {
            RuntimeError::ExecutionError(format!("Failed to compile bytecode module: {e}"))
        })?;

        // Clone what we need for the blocking spawn
        let engine = self.engine.clone();
        let plugin_module = self.plugin_module.clone(); // cheap Arc clone
        let context = context.clone();
        let config = config.clone();
        let log_level = log_level.to_string();

        // Run WASM evaluation on a blocking thread (wasmtime is synchronous)
        tokio::task::spawn_blocking(move || {
            evaluate_wasm(
                &engine,
                &plugin_module,
                &bytecode_module,
                &context,
                &config,
                fuel_limit,
                &log_level,
            )
        })
        .await
        .map_err(|e| RuntimeError::ExecutionError(format!("Task join error: {e}")))?
    }

    async fn validate(
        &self,
        js_source: &str,
        test_context: &serde_json::Value,
        test_config: &serde_json::Value,
    ) -> Result<ValidationResult, RuntimeError> {
        // Compile
        let wasm_bytes = match self.compile("validation", js_source).await {
            Ok(bytes) => bytes,
            Err(e) => {
                return Ok(ValidationResult {
                    success: false,
                    result: None,
                    error: Some(e.to_string()),
                });
            }
        };

        // Evaluate with default fuel limit
        match self
            .evaluate(
                "validation",
                &wasm_bytes,
                test_context,
                test_config,
                DEFAULT_FUEL_LIMIT,
                "info",
            )
            .await
        {
            Ok(result) => Ok(ValidationResult {
                success: true,
                result: Some(result),
                error: None,
            }),
            Err(e) => Ok(ValidationResult {
                success: false,
                result: None,
                error: Some(e.to_string()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_compile_simple_function() {
        let runtime = WasmDecisionRuntime::new().unwrap();
        let js = r#"
            function evaluate(ctx, config) {
                return { fire: true };
            }
        "#;
        let wasm = runtime.compile("test", js).await.unwrap();
        assert!(!wasm.is_empty());
    }

    #[tokio::test]
    async fn test_dynamic_module_size_small() {
        let runtime = WasmDecisionRuntime::new().unwrap();
        let js = r#"
            function evaluate(ctx, config) {
                return { fire: ctx.session.user.roles.includes("analyst") };
            }
        "#;
        let wasm = runtime.compile("test", js).await.unwrap();
        // Dynamic bytecode modules should be much smaller than the ~869 KB static modules
        assert!(
            wasm.len() < 50_000,
            "Bytecode WASM should be < 50 KB, got {} bytes",
            wasm.len()
        );
    }

    #[tokio::test]
    async fn test_strict_mode_rejects_global_leak() {
        let runtime = WasmDecisionRuntime::new().unwrap();
        // Assigning to an undeclared variable is a ReferenceError in strict mode
        let js = r#"
            function evaluate(ctx, config) {
                globalVar = 123;
                return { fire: true };
            }
        "#;
        let wasm = runtime.compile("test", js).await.unwrap();
        let ctx = serde_json::json!({"session": {"user": {"username": "test"}}});
        let result = runtime
            .evaluate(
                "test",
                &wasm,
                &ctx,
                &serde_json::json!({}),
                DEFAULT_FUEL_LIMIT,
                "off",
            )
            .await;
        assert!(
            result.is_err(),
            "Strict mode should reject undeclared global variable assignment"
        );
    }

    #[tokio::test]
    async fn test_evaluate_returns_fire_true() {
        let runtime = WasmDecisionRuntime::new().unwrap();
        let js = r#"
            function evaluate(ctx, config) {
                return { fire: true };
            }
        "#;
        let wasm = runtime.compile("test", js).await.unwrap();
        let ctx = serde_json::json!({"session": {"user": {"username": "alice"}}});
        let config = serde_json::json!({});
        let result = runtime
            .evaluate("test", &wasm, &ctx, &config, DEFAULT_FUEL_LIMIT, "off")
            .await
            .unwrap();
        assert!(result.fire);
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn test_evaluate_returns_fire_false() {
        let runtime = WasmDecisionRuntime::new().unwrap();
        let js = r#"
            function evaluate(ctx, config) {
                return { fire: false };
            }
        "#;
        let wasm = runtime.compile("test", js).await.unwrap();
        let ctx = serde_json::json!({});
        let config = serde_json::json!({});
        let result = runtime
            .evaluate("test", &wasm, &ctx, &config, DEFAULT_FUEL_LIMIT, "off")
            .await
            .unwrap();
        assert!(!result.fire);
    }

    #[tokio::test]
    async fn test_evaluate_with_config() {
        let runtime = WasmDecisionRuntime::new().unwrap();
        let js = r#"
            function evaluate(ctx, config) {
                return { fire: config.max_joins <= 3 };
            }
        "#;
        let wasm = runtime.compile("test", js).await.unwrap();
        let ctx = serde_json::json!({});
        let config = serde_json::json!({"max_joins": 2});
        let result = runtime
            .evaluate("test", &wasm, &ctx, &config, DEFAULT_FUEL_LIMIT, "off")
            .await
            .unwrap();
        assert!(result.fire);
    }

    #[tokio::test]
    async fn test_evaluate_role_based_decision() {
        let runtime = WasmDecisionRuntime::new().unwrap();
        let js = r#"
            function evaluate(ctx, config) {
                return { fire: ctx.session.user.roles.includes("analyst") };
            }
        "#;
        let wasm = runtime.compile("test", js).await.unwrap();

        // User with analyst role → fire
        let ctx = serde_json::json!({"session": {"user": {"roles": ["analyst", "viewer"]}}});
        let result = runtime
            .evaluate(
                "test",
                &wasm,
                &ctx,
                &serde_json::json!({}),
                DEFAULT_FUEL_LIMIT,
                "off",
            )
            .await
            .unwrap();
        assert!(result.fire);

        // User without analyst role → don't fire
        let ctx = serde_json::json!({"session": {"user": {"roles": ["viewer"]}}});
        let result = runtime
            .evaluate(
                "test",
                &wasm,
                &ctx,
                &serde_json::json!({}),
                DEFAULT_FUEL_LIMIT,
                "off",
            )
            .await
            .unwrap();
        assert!(!result.fire);
    }

    #[tokio::test]
    async fn test_compile_invalid_js() {
        let runtime = WasmDecisionRuntime::new().unwrap();
        let js = "this is not valid javascript {{{";
        let result = runtime.compile("test", js).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_validate_good_function() {
        let runtime = WasmDecisionRuntime::new().unwrap();
        let js = r#"
            function evaluate(ctx, config) {
                return { fire: true };
            }
        "#;
        let ctx = serde_json::json!({"session": {"user": {"username": "test"}}});
        let result = runtime
            .validate(js, &ctx, &serde_json::json!({}))
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.result.is_some());
        assert!(result.result.unwrap().fire);
    }

    #[tokio::test]
    async fn test_validate_bad_return() {
        let runtime = WasmDecisionRuntime::new().unwrap();
        let js = r#"
            function evaluate(ctx, config) {
                return { wrong: "shape" };
            }
        "#;
        let ctx = serde_json::json!({});
        let result = runtime
            .validate(js, &ctx, &serde_json::json!({}))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[tokio::test]
    async fn test_console_log_does_not_break_result_parsing() {
        let runtime = WasmDecisionRuntime::new().unwrap();
        let js = r#"
            function evaluate(ctx, config) {
                console.log('evaluate() called');
                console.log('config:', JSON.stringify(config));
                console.log('username:', ctx.session.user.username);
                return { fire: true };
            }
        "#;
        let wasm = runtime.compile("test", js).await.unwrap();
        let ctx = serde_json::json!({"session": {"user": {"username": "admin", "roles": []}}});
        let config = serde_json::json!({"key": "value"});
        let result = runtime
            .evaluate("test", &wasm, &ctx, &config, DEFAULT_FUEL_LIMIT, "info")
            .await
            .unwrap();
        assert!(
            result.fire,
            "console.log on stdout should not break JSON result parsing"
        );
        // console.log output is captured from stdout lines preceding the JSON result
        assert!(
            result.logs.iter().any(|l| l.contains("evaluate() called")),
            "console.log output should appear in logs"
        );
    }

    // --- parse_stdout_result unit tests ---

    #[test]
    fn test_parse_stdout_clean_json() {
        let (val, logs) = parse_stdout_result(r#"{"fire":true}"#).unwrap();
        assert!(val["fire"].as_bool().unwrap());
        assert!(logs.is_empty());
    }

    #[test]
    fn test_parse_stdout_with_console_log_lines() {
        // Reproduces the exact production bug: console.log on stdout before the JSON result
        let stdout = "{ctx.session.user.username}\n{\"fire\":true}";
        let (val, logs) = parse_stdout_result(stdout).unwrap();
        assert!(val["fire"].as_bool().unwrap());
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0], "{ctx.session.user.username}");
    }

    #[test]
    fn test_parse_stdout_multiple_log_lines() {
        let stdout = "evaluate() called\nconfig: {}\nusername: admin\nmatched admin, firing\n{\"fire\":true}";
        let (val, logs) = parse_stdout_result(stdout).unwrap();
        assert!(val["fire"].as_bool().unwrap());
        assert_eq!(logs.len(), 4);
        assert_eq!(logs[0], "evaluate() called");
        assert_eq!(logs[3], "matched admin, firing");
    }

    #[test]
    fn test_parse_stdout_no_valid_json() {
        let result = parse_stdout_result("not json at all\nstill not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_stdout_fire_false() {
        let stdout = "some log\n{\"fire\":false}";
        let (val, logs) = parse_stdout_result(stdout).unwrap();
        assert!(!val["fire"].as_bool().unwrap());
        assert_eq!(logs.len(), 1);
    }
}
