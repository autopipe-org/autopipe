use crate::config::{AppConfig, SshAuth};
use ssh2::Session;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;

/// Create an authenticated SSH session.
fn create_session(config: &AppConfig) -> Result<Session, String> {
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

    Ok(sess)
}

/// Execute a command on the remote server via SSH.
pub fn ssh_exec(config: &AppConfig, command: &str) -> Result<(String, i32), String> {
    let sess = create_session(config)?;

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

/// Read a remote file via SFTP.
pub fn sftp_read(config: &AppConfig, remote_path: &str) -> Result<String, String> {
    let sess = create_session(config)?;
    let sftp = sess.sftp().map_err(|e| format!("SFTP error: {}", e))?;

    let mut file = sftp
        .open(Path::new(remote_path))
        .map_err(|e| format!("Cannot open remote file '{}': {}", remote_path, e))?;

    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|e| format!("Read error: {}", e))?;

    Ok(content)
}

/// Write content to a remote file via SFTP.
pub fn sftp_write(config: &AppConfig, remote_path: &str, content: &str) -> Result<(), String> {
    let sess = create_session(config)?;
    let sftp = sess.sftp().map_err(|e| format!("SFTP error: {}", e))?;

    // Ensure parent directory exists
    if let Some(parent) = Path::new(remote_path).parent() {
        let mkdir_cmd = format!("mkdir -p '{}'", parent.display());
        let _ = ssh_exec(config, &mkdir_cmd);
    }

    let mut file = sftp
        .create(Path::new(remote_path))
        .map_err(|e| format!("Cannot create remote file '{}': {}", remote_path, e))?;

    file.write_all(content.as_bytes())
        .map_err(|e| format!("Write error: {}", e))?;

    Ok(())
}

/// List files in a remote directory via SFTP.
pub fn sftp_list(config: &AppConfig, remote_dir: &str) -> Result<Vec<SftpEntry>, String> {
    let sess = create_session(config)?;
    let sftp = sess.sftp().map_err(|e| format!("SFTP error: {}", e))?;

    let entries = sftp
        .readdir(Path::new(remote_dir))
        .map_err(|e| format!("Cannot list '{}': {}", remote_dir, e))?;

    let mut result: Vec<SftpEntry> = entries
        .into_iter()
        .map(|(path, stat)| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let is_dir = stat.is_dir();
            let size = stat.size.unwrap_or(0);
            SftpEntry { name, is_dir, size }
        })
        .collect();

    result.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name))
    });

    Ok(result)
}

/// SFTP directory entry.
pub struct SftpEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
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
