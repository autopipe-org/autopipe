//! Background MCP HTTP server daemon owned by the GUI.
//!
//! The desktop tray app spawns one MCP HTTP server in a background thread
//! using a dedicated tokio runtime. This module exposes a small handle the
//! GUI can use to:
//!   - read the current URL / token / port (for the "Register" button and
//!     "Copy snippet" button)
//!   - request a graceful shutdown (e.g. on port change, then restart)
//!
//! The daemon is intentionally simple: one shot. To change the port, the
//! caller shuts the current handle down and starts a new one.

use std::sync::{Arc, Mutex};
use std::thread;

use tokio_util::sync::CancellationToken;

use crate::config::{self, AppConfig};
use crate::mcp::server::{bind_with_fallback, run_mcp_http_server};

/// Snapshot of the running MCP server's network identity.
#[derive(Clone, Debug)]
pub struct McpServerInfo {
    /// Full URL clients connect to (e.g. `http://127.0.0.1:47823/mcp`).
    pub url: String,
    /// Bearer token clients must present in `Authorization: Bearer <token>`.
    pub token: String,
    /// Port actually bound by the OS (may differ from `configured_port` if
    /// the preferred port was occupied).
    pub bound_port: u16,
    /// Port the user requested in config.
    pub configured_port: u16,
}

/// Reasons the daemon's startup may have failed.
#[derive(Clone, Debug)]
pub enum McpStartError {
    Token(String),
    Bind(String),
}

impl std::fmt::Display for McpStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpStartError::Token(e) => write!(f, "Failed to load/create MCP token: {}", e),
            McpStartError::Bind(e) => write!(f, "Failed to bind MCP HTTP port: {}", e),
        }
    }
}

/// Internal state shared between the GUI and the daemon thread.
struct DaemonState {
    info: Option<McpServerInfo>,
    error: Option<McpStartError>,
    shutdown: CancellationToken,
}

/// Handle to a running MCP daemon thread. Drop or call `shutdown()` to stop it.
pub struct McpDaemonHandle {
    state: Arc<Mutex<DaemonState>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl McpDaemonHandle {
    /// Start the MCP HTTP server on a background tokio runtime.
    ///
    /// `preferred_port` is the port the user has configured. If occupied,
    /// `bind_with_fallback` walks nearby ports and finally falls back to
    /// an OS-assigned port.
    pub fn start(preferred_port: u16) -> Self {
        let shutdown = CancellationToken::new();
        let state = Arc::new(Mutex::new(DaemonState {
            info: None,
            error: None,
            shutdown: shutdown.clone(),
        }));

        let state_for_thread = state.clone();
        let thread = thread::Builder::new()
            .name("autopipe-mcp-daemon".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let mut s = state_for_thread.lock().unwrap();
                        s.error = Some(McpStartError::Bind(format!(
                            "tokio runtime: {}",
                            e
                        )));
                        return;
                    }
                };

                rt.block_on(async move {
                    Self::run(preferred_port, state_for_thread, shutdown).await;
                });
            })
            .expect("spawn MCP daemon thread");

        Self {
            state,
            thread: Some(thread),
        }
    }

    async fn run(
        preferred_port: u16,
        state: Arc<Mutex<DaemonState>>,
        shutdown: CancellationToken,
    ) {
        let token = match config::load_or_create_mcp_token() {
            Ok(t) => t,
            Err(e) => {
                state.lock().unwrap().error =
                    Some(McpStartError::Token(e.to_string()));
                return;
            }
        };

        let (listener, actual_port) = match bind_with_fallback(preferred_port).await {
            Ok(x) => x,
            Err(e) => {
                state.lock().unwrap().error =
                    Some(McpStartError::Bind(e.to_string()));
                return;
            }
        };

        let url = format!("http://127.0.0.1:{}/mcp", actual_port);

        // Persist the actual port so other CLIs (--register from a separate
        // process) and the next launch can see it.
        let mut cfg = AppConfig::load();
        if cfg.mcp_actual_port != Some(actual_port) {
            cfg.mcp_actual_port = Some(actual_port);
            let _ = cfg.save();
        }

        {
            let mut s = state.lock().unwrap();
            s.info = Some(McpServerInfo {
                url: url.clone(),
                token: token.clone(),
                bound_port: actual_port,
                configured_port: preferred_port,
            });
        }

        if let Err(e) = run_mcp_http_server(listener, token, shutdown).await {
            tracing::error!(error = %e, "MCP HTTP server exited with error");
        }

        state.lock().unwrap().info = None;
    }

    /// Current server snapshot, or `None` if not yet bound or already stopped.
    pub fn info(&self) -> Option<McpServerInfo> {
        self.state.lock().unwrap().info.clone()
    }

    /// Last startup error, if any.
    pub fn error(&self) -> Option<McpStartError> {
        self.state.lock().unwrap().error.clone()
    }

    /// Returns true once `info()` is populated or `error()` is set.
    pub fn is_settled(&self) -> bool {
        let s = self.state.lock().unwrap();
        s.info.is_some() || s.error.is_some()
    }

    /// Request a graceful shutdown of the server.
    pub fn shutdown(&self) {
        self.state.lock().unwrap().shutdown.cancel();
    }
}

impl Drop for McpDaemonHandle {
    fn drop(&mut self) {
        self.shutdown();
        if let Some(t) = self.thread.take() {
            // Best-effort wait so the port is released before a subsequent
            // restart with the same port.
            let _ = t.join();
        }
    }
}
