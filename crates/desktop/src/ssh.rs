use crate::config::{AppConfig, SshAuth};
use ssh2::Session;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};

// ── SSH session for remote commands ─────────────────────────────────

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

/// Test SSH connection.
pub fn test_connection(config: &AppConfig) -> Result<String, String> {
    let (output, status) = ssh_exec(config, "echo 'AutoPipe SSH OK' && hostname")?;
    if status == 0 {
        Ok(output.trim().to_string())
    } else {
        Err(format!("SSH test failed with exit code {}", status))
    }
}

// ── SSHFS mount/unmount ─────────────────────────────────────────────

/// Check if sshfs is available on this system.
pub fn check_sshfs_available() -> bool {
    #[cfg(target_os = "windows")]
    {
        Path::new(r"C:\Program Files\SSHFS-Win\bin\sshfs-win.exe").exists()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("which")
            .arg("sshfs")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

/// Check if the local mount path is currently mounted.
pub fn is_mounted(config: &AppConfig) -> bool {
    if config.local_mount_path.is_empty() {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        // Check net use output for the drive letter
        Command::new("net")
            .arg("use")
            .output()
            .map(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout);
                stdout.contains(&config.local_mount_path)
            })
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("mount")
            .output()
            .map(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout);
                stdout.contains(&config.local_mount_path)
            })
            .unwrap_or(false)
    }
}

/// Mount remote directory locally via SSHFS.
pub fn sshfs_mount(config: &AppConfig) -> Result<String, String> {
    if config.ssh_host.is_empty() {
        return Err("SSH host is not configured".into());
    }
    if config.remote_mount_root.is_empty() {
        return Err("Remote mount root is not configured".into());
    }
    if config.local_mount_path.is_empty() {
        return Err("Local mount path is not configured".into());
    }
    if is_mounted(config) {
        return Ok(format!("Already mounted at {}", config.local_mount_path));
    }
    if !check_sshfs_available() {
        #[cfg(target_os = "windows")]
        return Err("sshfs-win is not installed. Install WinFsp + SSHFS-Win first.".into());
        #[cfg(not(target_os = "windows"))]
        return Err("sshfs is not installed. Install macFUSE + sshfs (Mac) or sshfs (Linux) first.".into());
    }

    #[cfg(target_os = "windows")]
    {
        sshfs_mount_windows(config)
    }
    #[cfg(not(target_os = "windows"))]
    {
        sshfs_mount_unix(config)
    }
}

#[cfg(not(target_os = "windows"))]
fn sshfs_mount_unix(config: &AppConfig) -> Result<String, String> {
    std::fs::create_dir_all(&config.local_mount_path)
        .map_err(|e| format!("Cannot create mount point: {}", e))?;

    let remote = format!(
        "{}@{}:{}",
        config.ssh_user, config.ssh_host, config.remote_mount_root
    );
    let port_opt = format!("port={}", config.ssh_port);

    let output = match &config.ssh_auth {
        SshAuth::Key { key_path } => {
            let id_opt = format!("IdentityFile={}", key_path);
            Command::new("sshfs")
                .arg(&remote)
                .arg(&config.local_mount_path)
                .args(["-o", &port_opt])
                .args(["-o", &id_opt])
                .args(["-o", "StrictHostKeyChecking=no"])
                .output()
                .map_err(|e| format!("Failed to run sshfs: {}", e))?
        }
        SshAuth::Password { password } => {
            let mut child = Command::new("sshfs")
                .arg(&remote)
                .arg(&config.local_mount_path)
                .args(["-o", &port_opt])
                .args(["-o", "password_stdin"])
                .args(["-o", "StrictHostKeyChecking=no"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("Failed to run sshfs: {}", e))?;

            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(password.as_bytes());
            }

            child
                .wait_with_output()
                .map_err(|e| format!("sshfs error: {}", e))?
        }
        SshAuth::Agent => Command::new("sshfs")
            .arg(&remote)
            .arg(&config.local_mount_path)
            .args(["-o", &port_opt])
            .args(["-o", "StrictHostKeyChecking=no"])
            .output()
            .map_err(|e| format!("Failed to run sshfs: {}", e))?,
    };

    if output.status.success() {
        Ok(format!(
            "Mounted {} at {}",
            config.remote_mount_root, config.local_mount_path
        ))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("sshfs mount failed: {}", stderr.trim()))
    }
}

#[cfg(target_os = "windows")]
fn sshfs_mount_windows(config: &AppConfig) -> Result<String, String> {
    let drive = &config.local_mount_path; // e.g. "S:"
    let remote_path_win = config.remote_mount_root.replace('/', r"\");
    let port_part = if config.ssh_port != 22 {
        format!("!{}", config.ssh_port)
    } else {
        String::new()
    };

    let output = match &config.ssh_auth {
        SshAuth::Key { .. } | SshAuth::Agent => {
            let unc = format!(
                r"\\sshfs.kr\{}@{}{}{}",
                config.ssh_user, config.ssh_host, port_part, remote_path_win
            );
            Command::new("net")
                .args(["use", drive, &unc])
                .output()
                .map_err(|e| format!("Failed to run net use: {}", e))?
        }
        SshAuth::Password { password } => {
            let unc = format!(
                r"\\sshfs.r\{}@{}{}{}",
                config.ssh_user, config.ssh_host, port_part, remote_path_win
            );
            Command::new("net")
                .args([
                    "use",
                    drive,
                    &unc,
                    &format!("/user:{}", config.ssh_user),
                    password,
                ])
                .output()
                .map_err(|e| format!("Failed to run net use: {}", e))?
        }
    };

    if output.status.success() {
        Ok(format!(
            "Mounted {} at {}",
            config.remote_mount_root, drive
        ))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("sshfs-win mount failed: {}", stderr.trim()))
    }
}

/// Unmount SSHFS.
pub fn sshfs_unmount(config: &AppConfig) -> Result<(), String> {
    if !is_mounted(config) {
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    let output = Command::new("net")
        .args(["use", &config.local_mount_path, "/delete", "/yes"])
        .output()
        .map_err(|e| format!("Failed to unmount: {}", e))?;

    #[cfg(target_os = "macos")]
    let output = Command::new("umount")
        .arg(&config.local_mount_path)
        .output()
        .map_err(|e| format!("Failed to unmount: {}", e))?;

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let output = Command::new("fusermount")
        .args(["-u", &config.local_mount_path])
        .output()
        .map_err(|e| format!("Failed to unmount: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Unmount failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}
