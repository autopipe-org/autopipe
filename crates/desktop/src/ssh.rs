use crate::config::{AppConfig, SshAuth};
use ssh2::Session;
use std::collections::HashMap;
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

// ── Reused SSH sessions ─────────────────────────────────────────────
//
// Opening a fresh TCP + handshake + auth for every command is the dominant
// per-command cost and a source of transient "hiccup" failures under load. So
// we keep one live session per SSH target and reuse it: each command just opens
// a new channel on the shared session. ssh2's `Session` is a cheap
// `Arc<Mutex<..>>` clone and serializes its own inner access, so sharing/reusing
// it across threads is safe.
//
// Reconnect safety:
//   * If the stored session has died (idle timeout, dropped TCP), that surfaces
//     when we open a channel — BEFORE the command runs — so we discard it,
//     reconnect, and retry safely (the command had no chance to take effect).
//   * A failure AFTER the command was exec'd is NOT retried (it may already have
//     had side effects); we just drop the session so the next command reconnects.

fn sessions() -> &'static Mutex<HashMap<String, Session>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, Session>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn session_key(config: &AppConfig) -> String {
    format!("{}@{}:{}", config.ssh_user, config.ssh_host, config.ssh_port)
}

fn create_session(config: &AppConfig) -> Result<Session, String> {
    let addr = format!("{}:{}", config.ssh_host, config.ssh_port);
    let sock_addr = addr
        .to_socket_addrs()
        .map_err(|e| format!("DNS resolve failed: {}", e))?
        .next()
        .ok_or_else(|| "No address resolved".to_string())?;

    let tcp = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(10))
        .map_err(|e| format!("TCP connect failed: {}", e))?;

    tcp.set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| format!("Set read timeout failed: {}", e))?;
    tcp.set_write_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| format!("Set write timeout failed: {}", e))?;

    let mut sess = Session::new().map_err(|e| format!("SSH session error: {}", e))?;
    sess.set_tcp_stream(tcp);
    sess.handshake()
        .map_err(|e| format!("SSH handshake failed: {}", e))?;

    match &config.ssh_auth {
        SshAuth::Key { key_path } => {
            sess.userauth_pubkey_file(&config.ssh_user, None, Path::new(key_path), None)
                .map_err(|e| format!("Key auth failed: {}", e))?;
        }
        SshAuth::Password { password } => {
            sess.userauth_password(&config.ssh_user, password)
                .map_err(|e| format!("Password auth failed: {}", e))?;
        }
        SshAuth::Agent => {
            sess.userauth_agent(&config.ssh_user)
                .map_err(|e| format!("Agent auth failed: {}", e))?;
        }
    }

    // Keep idle sessions alive so they aren't dropped between commands.
    sess.set_keepalive(true, 30);
    Ok(sess)
}

/// Outcome of trying to run a command on a (reused) session.
enum RunOutcome {
    /// Command executed; carries (combined output, exit status).
    Ran(String, i32),
    /// The session was dead at channel-open (before the command ran). Safe to
    /// reconnect and retry — the command had no chance to take effect.
    Reconnect(String),
    /// The transport failed after the command was exec'd. NOT safe to retry
    /// (side effects may have happened); the session is discarded.
    Failed(String),
}

fn run_on_session(sess: &Session, command: &str) -> RunOutcome {
    // A dead session fails here, before the command is delivered → safe to retry.
    let mut channel = match sess.channel_session() {
        Ok(c) => c,
        Err(e) => return RunOutcome::Reconnect(format!("Channel error: {}", e)),
    };

    // From this point the command has been delivered to the shell → do NOT retry.
    if let Err(e) = channel.exec(command) {
        return RunOutcome::Failed(format!("Exec error: {}", e));
    }

    let mut stdout_bytes = Vec::new();
    if let Err(e) = channel.read_to_end(&mut stdout_bytes) {
        return RunOutcome::Failed(format!("Read stdout error: {}", e));
    }
    let stdout = String::from_utf8_lossy(&stdout_bytes).into_owned();

    let mut stderr_bytes = Vec::new();
    if let Err(e) = channel.stderr().read_to_end(&mut stderr_bytes) {
        return RunOutcome::Failed(format!("Read stderr error: {}", e));
    }
    let stderr = String::from_utf8_lossy(&stderr_bytes).into_owned();

    channel.wait_close().ok();
    let exit_status = channel.exit_status().unwrap_or(-1);

    // Combine stdout and stderr
    let output = if stderr.is_empty() {
        stdout
    } else if stdout.is_empty() {
        stderr
    } else {
        format!("{}\n{}", stdout, stderr)
    };
    RunOutcome::Ran(output, exit_status)
}

/// Execute a command on the remote server via SSH, reusing a live session when
/// possible and reconnecting on connection failures. Retries up to 3 times, but
/// only reconnects-and-retries when the failure happened before the command ran.
pub fn ssh_exec(config: &AppConfig, command: &str) -> Result<(String, i32), String> {
    let key = session_key(config);
    let max_attempts: usize = 3;
    let mut last_err = String::new();

    for attempt in 0..max_attempts {
        // Reuse the stored session if present, else connect a new one.
        let sess = {
            let mut store = match sessions().lock() {
                Ok(g) => g,
                Err(_) => return Err("SSH session store poisoned".to_string()),
            };
            match store.get(&key).cloned() {
                Some(s) => s,
                None => match create_session(config) {
                    Ok(s) => {
                        store.insert(key.clone(), s.clone());
                        s
                    }
                    Err(e) => {
                        drop(store);
                        last_err = e;
                        std::thread::sleep(Duration::from_millis(300u64 << attempt));
                        continue;
                    }
                },
            }
        };

        match run_on_session(&sess, command) {
            RunOutcome::Ran(output, status) => return Ok((output, status)),
            RunOutcome::Reconnect(e) => {
                // Dead before the command ran → drop the session and retry.
                last_err = e;
                if let Ok(mut store) = sessions().lock() {
                    store.remove(&key);
                }
                std::thread::sleep(Duration::from_millis(300u64 << attempt));
            }
            RunOutcome::Failed(e) => {
                // Failed after exec → don't retry (possible side effects). Drop
                // the suspect session so the next command reconnects.
                if let Ok(mut store) = sessions().lock() {
                    store.remove(&key);
                }
                return Err(e);
            }
        }
    }

    Err(format!("{} (after {} attempts)", last_err, max_attempts))
}

/// Test SSH connection.
pub fn test_connection(config: &AppConfig) -> Result<String, String> {
    let (output, status) = ssh_exec(config, "echo 'AutoPipe SSH OK' && hostname")?;
    if status == 0 {
        Ok(output.trim().to_string())
    } else {
        Err(format!("SSH test failed with exit code {}", status))
    }
}
