use crate::config::{AppConfig, SshAuth};
use ssh2::Session;
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

// ── SSH session for remote commands ─────────────────────────────────

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

    Ok(sess)
}

fn ssh_exec_once(config: &AppConfig, command: &str) -> Result<(String, i32), String> {
    let sess = create_session(config)?;

    let mut channel = sess
        .channel_session()
        .map_err(|e| format!("Channel error: {}", e))?;

    channel
        .exec(command)
        .map_err(|e| format!("Exec error: {}", e))?;

    let mut stdout_bytes = Vec::new();
    channel
        .read_to_end(&mut stdout_bytes)
        .map_err(|e| format!("Read stdout error: {}", e))?;
    let stdout = String::from_utf8_lossy(&stdout_bytes).into_owned();

    let mut stderr_bytes = Vec::new();
    channel
        .stderr()
        .read_to_end(&mut stderr_bytes)
        .map_err(|e| format!("Read stderr error: {}", e))?;
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

    Ok((output, exit_status))
}

/// Execute a command on the remote server via SSH.
/// Retries up to 3 times on connection/auth failures.
pub fn ssh_exec(config: &AppConfig, command: &str) -> Result<(String, i32), String> {
    let max_retries = 3;
    let mut last_err = String::new();

    for attempt in 0..max_retries {
        match ssh_exec_once(config, command) {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_err = e;
                // Only retry on connection/auth errors, not exec errors
                if last_err.contains("auth failed")
                    || last_err.contains("TCP connect")
                    || last_err.contains("handshake")
                {
                    if attempt < max_retries - 1 {
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        continue;
                    }
                } else {
                    return Err(last_err);
                }
            }
        }
    }

    Err(format!("{} (after {} retries)", last_err, max_retries))
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
