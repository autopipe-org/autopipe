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

    let mut stdout = String::new();
    channel
        .read_to_string(&mut stdout)
        .map_err(|e| format!("Read stdout error: {}", e))?;

    let mut stderr = String::new();
    channel
        .stderr()
        .read_to_string(&mut stderr)
        .map_err(|e| format!("Read stderr error: {}", e))?;

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

/// Download a file from the remote server via SCP.
/// Returns the bytes of the downloaded file.
pub fn scp_download(config: &AppConfig, remote_path: &str) -> Result<Vec<u8>, String> {
    let sess = create_session(config)?;

    let (mut channel, stat) = sess
        .scp_recv(Path::new(remote_path))
        .map_err(|e| format!("SCP recv error: {}", e))?;

    let mut buf = Vec::with_capacity(stat.size() as usize);
    channel
        .read_to_end(&mut buf)
        .map_err(|e| format!("SCP read error: {}", e))?;

    // Close the SCP channel
    channel.send_eof().ok();
    channel.wait_eof().ok();
    channel.close().ok();
    channel.wait_close().ok();

    Ok(buf)
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
