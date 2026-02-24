use crate::config::{AppConfig, SshAuth};
use ssh2::Session;
use std::io::Read;
use std::net::TcpStream;
use std::path::Path;

/// Execute a command on the remote server via SSH.
pub fn ssh_exec(config: &AppConfig, command: &str) -> Result<(String, i32), String> {
    let addr = format!("{}:{}", config.ssh_host, config.ssh_port);
    let tcp = TcpStream::connect(&addr).map_err(|e| format!("TCP connect failed: {}", e))?;

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

    let mut channel = sess
        .channel_session()
        .map_err(|e| format!("Channel error: {}", e))?;

    channel
        .exec(command)
        .map_err(|e| format!("Exec error: {}", e))?;

    let mut output = String::new();
    channel
        .read_to_string(&mut output)
        .map_err(|e| format!("Read error: {}", e))?;

    channel.wait_close().ok();
    let exit_status = channel.exit_status().unwrap_or(-1);

    Ok((output, exit_status))
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
