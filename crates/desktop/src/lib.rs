//! Library entry point for the AutoPipe desktop crate.
//!
//! Historically this crate was a binary only. The new Tauri-based UI lives
//! in a separate Cargo project at `frontend/src-tauri/` so that the Tauri
//! CLI's conventions are respected; that project depends on this crate as
//! a library and re-uses everything below (config, MCP server, SSH, command
//! handlers, etc.) without duplication.
//!
//! The original binary (`bin/autopipe`) still works exactly as before — it
//! just `use`s these same modules from inside `main.rs`.

pub mod claude_config;
#[cfg(feature = "tauri-ui")]
pub mod commands;
pub mod config;
pub mod mcp;
pub mod ssh;
