use eframe::egui;
use std::sync::mpsc;

use crate::claude_config;
use crate::config::{AppConfig, SshAuth};

#[derive(PartialEq)]
enum Tab {
    Setup,
    Connection,
    Ssh,
    GitHub,
    Status,
}

/// Messages from the device-flow background thread.
enum GitHubMsg {
    /// Device code received — show user_code to user.
    DeviceCode {
        device_code: String,
        user_code: String,
        verification_uri: String,
        interval: u64,
    },
    /// Polling succeeded — token obtained.
    Token(String),
    /// An error occurred.
    Error(String),
}

pub struct AutoPipeApp {
    config: AppConfig,
    active_tab: Tab,
    ssh_password_input: String,
    ssh_key_path_input: String,
    ssh_auth_type: usize, // 0=Agent, 1=Key, 2=Password
    status_message: String,
    should_minimize: bool,
    minimized_to_tray: bool,
    // GitHub device flow state
    github_rx: Option<mpsc::Receiver<GitHubMsg>>,
    github_user_code: Option<String>,
    github_verification_uri: Option<String>,
    github_polling: bool,
    github_username: Option<String>,
}

impl AutoPipeApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = AppConfig::load();
        let (ssh_auth_type, ssh_key_path_input, ssh_password_input) = match &config.ssh_auth {
            SshAuth::Agent => (0, String::new(), String::new()),
            SshAuth::Key { key_path } => (1, key_path.clone(), String::new()),
            SshAuth::Password { password } => (2, String::new(), password.clone()),
        };

        // If token exists, try to resolve GitHub username
        let github_username = None; // Will be resolved on GitHub tab open

        Self {
            config,
            active_tab: Tab::Setup,
            ssh_password_input,
            ssh_key_path_input,
            ssh_auth_type,
            status_message: String::new(),
            should_minimize: false,
            minimized_to_tray: false,
            github_rx: None,
            github_user_code: None,
            github_verification_uri: None,
            github_polling: false,
            github_username,
        }
    }

    pub fn should_minimize(&self) -> bool {
        self.should_minimize
    }

    fn save_config(&mut self) {
        self.config.ssh_auth = match self.ssh_auth_type {
            1 => SshAuth::Key {
                key_path: self.ssh_key_path_input.clone(),
            },
            2 => SshAuth::Password {
                password: self.ssh_password_input.clone(),
            },
            _ => SshAuth::Agent,
        };

        match self.config.save() {
            Ok(_) => self.status_message = "Settings saved.".into(),
            Err(e) => self.status_message = format!("Save failed: {}", e),
        }
    }
}

impl eframe::App for AutoPipeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, Tab::Setup, "Setup");
                ui.selectable_value(&mut self.active_tab, Tab::Connection, "Connection");
                ui.selectable_value(&mut self.active_tab, Tab::Ssh, "SSH");
                ui.selectable_value(&mut self.active_tab, Tab::GitHub, "GitHub");
                ui.selectable_value(&mut self.active_tab, Tab::Status, "Status");
            });
        });

        // Process GitHub device flow messages
        if let Some(rx) = self.github_rx.take() {
            let mut keep_rx = true;
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    GitHubMsg::DeviceCode {
                        device_code: _,
                        user_code,
                        verification_uri,
                        interval: _,
                    } => {
                        self.github_user_code = Some(user_code);
                        self.github_verification_uri = Some(verification_uri);
                    }
                    GitHubMsg::Token(token) => {
                        self.config.github_token = Some(token);
                        self.github_polling = false;
                        self.github_user_code = None;
                        self.github_verification_uri = None;
                        keep_rx = false;
                        let _ = self.config.save();
                        self.status_message = "GitHub login successful!".into();
                        // Fetch username
                        let token = self.config.github_token.clone().unwrap();
                        let (tx, rx2) = mpsc::channel();
                        std::thread::spawn(move || {
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async {
                                match fetch_github_username(&token).await {
                                    Ok(name) => { let _ = tx.send(Some(name)); }
                                    Err(_) => { let _ = tx.send(None); }
                                }
                            });
                        });
                        if let Ok(name) = rx2.recv() {
                            self.github_username = name;
                        }
                    }
                    GitHubMsg::Error(e) => {
                        self.github_polling = false;
                        self.github_user_code = None;
                        keep_rx = false;
                        self.status_message = format!("GitHub login failed: {}", e);
                    }
                }
            }
            if keep_rx {
                self.github_rx = Some(rx);
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| match self.active_tab {
            Tab::Setup => self.draw_setup_tab(ui),
            Tab::Connection => self.draw_connection_tab(ui),
            Tab::Ssh => self.draw_ssh_tab(ui),
            Tab::GitHub => self.draw_github_tab(ui),
            Tab::Status => self.draw_status_tab(ui),
        });

        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    self.save_config();
                }
                if ui.button("Register & Minimize to Tray").clicked() {
                    self.save_config();
                    let config_path = AppConfig::config_path();
                    match claude_config::register_mcp_server(
                        &config_path.to_string_lossy(),
                    ) {
                        Ok(_) => {
                            self.config.mcp_registered = true;
                            let _ = self.config.save();
                            self.status_message =
                                "MCP server registered in Claude Desktop. You can now use autopipe tools in Claude Desktop.".into();
                            self.should_minimize = true;
                        }
                        Err(e) => {
                            self.status_message =
                                format!("Failed to register MCP: {}", e);
                        }
                    }
                }
                if !self.status_message.is_empty() {
                    ui.label(&self.status_message);
                }
            });
        });

        // Minimize to tray: hide window from taskbar
        if self.should_minimize {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.should_minimize = false;
            self.minimized_to_tray = true;
        }
    }
}

impl AutoPipeApp {
    /// Restore the window from tray (make visible and focus).
    pub fn restore_from_tray(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.minimized_to_tray = false;
    }

    pub fn is_minimized_to_tray(&self) -> bool {
        self.minimized_to_tray
    }
}

impl AutoPipeApp {
    fn draw_setup_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("AutoPipe Setup Guide");
        ui.add_space(15.0);

        // Step 1
        ui.heading("Step 1: Install an MCP-Compatible AI App");
        ui.add_space(5.0);
        ui.label("AutoPipe works with any MCP-compatible AI application.");
        ui.label("For example, Claude Desktop:");
        ui.hyperlink_to(
            "https://claude.ai/download",
            "https://claude.ai/download",
        );

        ui.add_space(15.0);
        ui.separator();
        ui.add_space(10.0);

        // Step 2
        ui.heading("Step 2: Configure Settings");
        ui.add_space(5.0);
        ui.label("Go to the Connection tab to set the registry server URL.");
        ui.label("Go to the SSH tab to configure the remote server connection.");

        ui.add_space(15.0);
        ui.separator();
        ui.add_space(10.0);

        // Step 3
        ui.heading("Step 3: Register MCP Tools");
        ui.add_space(5.0);
        ui.label("Click 'Register & Minimize to Tray' at the bottom.");
        ui.label("This auto-registers autopipe tools in Claude Desktop.");
        ui.label("For other MCP apps, add the MCP server config manually:");
        ui.add_space(3.0);
        ui.code("desktop --mcp-server");
        ui.add_space(3.0);
        ui.label("After registration, restart your AI app to load the tools.");

        ui.add_space(15.0);
        ui.separator();
        ui.add_space(10.0);

        // Step 4
        ui.heading("Step 4: Use Your AI App");
        ui.add_space(5.0);
        ui.label("Open your MCP-compatible AI app and start a conversation.");
        ui.label("You can ask the AI to:");
        ui.label("  - Search for existing workflows and plugins");
        ui.label("  - Create new bioinformatics pipelines");
        ui.label("  - Build, run, and monitor pipelines");
        ui.label("  - Upload and publish workflows to the registry");
    }

    fn draw_connection_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Registry Connections");
        ui.add_space(10.0);

        ui.label("The first URL is used as the active registry for MCP tools.");
        ui.label("Example: http://192.168.100.30:8090");
        ui.add_space(10.0);

        let mut remove_idx: Option<usize> = None;
        let mut test_idx: Option<usize> = None;

        for i in 0..self.config.registry_urls.len() {
            ui.horizontal(|ui| {
                ui.label(format!("{}.", i + 1));
                ui.add(egui::TextEdit::singleline(&mut self.config.registry_urls[i])
                    .desired_width(350.0));
                if ui.button("Test").clicked() {
                    test_idx = Some(i);
                }
                if self.config.registry_urls.len() > 1 {
                    if ui.button("Remove").clicked() {
                        remove_idx = Some(i);
                    }
                }
            });
        }

        if let Some(idx) = remove_idx {
            self.config.registry_urls.remove(idx);
        }

        if let Some(idx) = test_idx {
            let url = self.config.registry_urls[idx].clone();
            self.status_message = match reqwest_test(&url) {
                Ok(_) => format!("Connection {} OK", idx + 1),
                Err(e) => format!("Connection {} failed: {}", idx + 1, e),
            };
        }

        ui.add_space(5.0);
        if ui.button("+ Add Registry URL").clicked() {
            self.config.registry_urls.push(String::new());
        }

        // Sync primary URL with first entry
        if let Some(first) = self.config.registry_urls.first() {
            self.config.registry_url = first.clone();
        }
    }

    fn draw_ssh_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("SSH Configuration");
        ui.add_space(5.0);
        ui.label("Configure the remote server where pipelines will be executed.");
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            ui.label("Host:");
            ui.text_edit_singleline(&mut self.config.ssh_host);
        });
        ui.horizontal(|ui| {
            ui.label("Port:");
            let mut port_str = self.config.ssh_port.to_string();
            if ui.text_edit_singleline(&mut port_str).changed() {
                self.config.ssh_port = port_str.parse().unwrap_or(22);
            }
        });
        ui.horizontal(|ui| {
            ui.label("User:");
            ui.text_edit_singleline(&mut self.config.ssh_user);
        });

        ui.add_space(5.0);
        ui.label("Authentication:");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.ssh_auth_type, 0, "SSH Agent");
            ui.radio_value(&mut self.ssh_auth_type, 1, "Key File");
            ui.radio_value(&mut self.ssh_auth_type, 2, "Password");
        });

        match self.ssh_auth_type {
            1 => {
                ui.horizontal(|ui| {
                    ui.label("Key Path:");
                    ui.text_edit_singleline(&mut self.ssh_key_path_input);
                });
            }
            2 => {
                ui.horizontal(|ui| {
                    ui.label("Password:");
                    ui.add(egui::TextEdit::singleline(&mut self.ssh_password_input).password(true));
                });
            }
            _ => {}
        }

        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.label("Remote Repo Path:");
            ui.text_edit_singleline(&mut self.config.repo_path);
        });

        if ui.button("Test SSH Connection").clicked() {
            self.save_config();
            match crate::ssh::test_connection(&self.config) {
                Ok(msg) => self.status_message = format!("SSH OK: {}", msg),
                Err(e) => self.status_message = format!("SSH Failed: {}", e),
            }
        }
    }

    fn draw_status_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Status");
        ui.add_space(10.0);

        // MCP registration
        let registered = claude_config::is_registered();
        ui.horizontal(|ui| {
            ui.label("MCP Server:");
            if registered {
                ui.colored_label(egui::Color32::GREEN, "Registered in Claude Desktop");
            } else {
                ui.colored_label(egui::Color32::RED, "Not registered");
            }
        });

        // Config path
        ui.horizontal(|ui| {
            ui.label("Config path:");
            ui.label(claude_config::claude_desktop_config_path().to_string_lossy().to_string());
        });

        // Registry URLs
        ui.label("Registry URLs:");
        for (i, url) in self.config.registry_urls.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("  {}.", i + 1));
                ui.label(url);
            });
        }

        ui.add_space(10.0);
        if registered {
            if ui.button("Unregister MCP Server").clicked() {
                match claude_config::unregister_mcp_server() {
                    Ok(_) => {
                        self.config.mcp_registered = false;
                        let _ = self.config.save();
                        self.status_message = "MCP server unregistered from Claude Desktop.".into();
                    }
                    Err(e) => {
                        self.status_message = format!("Unregister failed: {}", e);
                    }
                }
            }
        } else if ui.button("Register MCP Server").clicked() {
            self.save_config();
            let config_path = AppConfig::config_path();
            match claude_config::register_mcp_server(&config_path.to_string_lossy()) {
                Ok(_) => {
                    self.config.mcp_registered = true;
                    let _ = self.config.save();
                    self.status_message = "MCP server registered. Restart Claude Desktop to load tools.".into();
                }
                Err(e) => {
                    self.status_message = format!("Register failed: {}", e);
                }
            }
        }
    }

    fn draw_github_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("GitHub Integration");
        ui.add_space(10.0);

        if self.config.github_token.is_some() {
            // Logged in
            let username = self.github_username.clone().unwrap_or_else(|| "(unknown)".into());
            ui.horizontal(|ui| {
                ui.label("Logged in as:");
                ui.strong(&username);
            });

            ui.add_space(5.0);
            if ui.button("Logout").clicked() {
                self.config.github_token = None;
                self.github_username = None;
                let _ = self.config.save();
                self.status_message = "GitHub logged out.".into();
                return;
            }

            ui.add_space(15.0);
            ui.separator();
            ui.add_space(10.0);

            ui.label("Pipeline Repository:");
            ui.horizontal(|ui| {
                ui.label(format!("{}/", &username));
                ui.text_edit_singleline(&mut self.config.github_repo);
            });
            ui.label("Workflows will be committed to this repository.");

            // Resolve username if not loaded yet
            if self.github_username.is_none() {
                if let Some(ref token) = self.config.github_token {
                    let token = token.clone();
                    let (tx, rx) = mpsc::channel();
                    std::thread::spawn(move || {
                        let rt = tokio::runtime::Runtime::new().unwrap();
                        rt.block_on(async {
                            match fetch_github_username(&token).await {
                                Ok(name) => { let _ = tx.send(Some(name)); }
                                Err(_) => { let _ = tx.send(None); }
                            }
                        });
                    });
                    if let Ok(name) = rx.recv() {
                        self.github_username = name;
                    }
                }
            }
        } else if self.github_polling {
            // Waiting for user to authorize
            ui.label("Complete the authorization in your browser:");
            ui.add_space(10.0);

            if let Some(ref code) = self.github_user_code {
                ui.horizontal(|ui| {
                    ui.label("Your code:");
                    ui.heading(code);
                });
            }

            ui.add_space(5.0);
            if let Some(ref uri) = self.github_verification_uri {
                if ui.button("Open Browser").clicked() {
                    let _ = open::that(uri);
                }
                ui.label(format!("Or visit: {}", uri));
            }

            ui.add_space(10.0);
            ui.spinner();
            ui.label("Waiting for authorization...");

            if ui.button("Cancel").clicked() {
                self.github_polling = false;
                self.github_user_code = None;
                self.github_verification_uri = None;
                self.github_rx = None;
            }
        } else {
            // Not logged in
            ui.label("Connect your GitHub account to upload and publish workflows.");
            ui.add_space(10.0);

            if ui.button("Login with GitHub").clicked() {
                let registry_url = self.config.registry_url.clone();
                let (tx, rx) = mpsc::channel();
                self.github_rx = Some(rx);
                self.github_polling = true;

                std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async {
                        run_device_flow(&registry_url, tx).await;
                    });
                });
            }

            ui.add_space(15.0);
            ui.separator();
            ui.add_space(10.0);

            ui.label("Pipeline Repository:");
            ui.horizontal(|ui| {
                ui.label("<username>/");
                ui.text_edit_singleline(&mut self.config.github_repo);
            });
        }
    }
}

async fn fetch_github_username(token: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "autopipe-desktop")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("GitHub API error: {}", resp.status()));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    body["login"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No login field".to_string())
}

async fn run_device_flow(registry_url: &str, tx: mpsc::Sender<GitHubMsg>) {
    let client = reqwest::Client::new();
    let base = registry_url.trim_end_matches('/');

    // Step 1: Request device code from our registry server
    let url = format!("{}/api/auth/device", base);
    let resp = match client.post(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(GitHubMsg::Error(format!("Request to {} failed: {}", url, e)));
            return;
        }
    };

    let status = resp.status();
    let text = match resp.text().await {
        Ok(t) => t,
        Err(e) => {
            let _ = tx.send(GitHubMsg::Error(format!("Failed to read response: {}", e)));
            return;
        }
    };

    let body: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.send(GitHubMsg::Error(format!(
                "Invalid JSON (status {}): {} — body: {}",
                status, e, &text[..text.len().min(200)]
            )));
            return;
        }
    };

    let device_code = body["device_code"].as_str().unwrap_or_default().to_string();
    let user_code = body["user_code"].as_str().unwrap_or_default().to_string();
    let verification_uri = body["verification_uri"]
        .as_str()
        .unwrap_or("https://github.com/login/device")
        .to_string();
    let interval = body["interval"].as_u64().unwrap_or(5);

    if device_code.is_empty() {
        let _ = tx.send(GitHubMsg::Error(
            body["error"]
                .as_str()
                .unwrap_or("Failed to get device code")
                .to_string(),
        ));
        return;
    }

    let _ = tx.send(GitHubMsg::DeviceCode {
        device_code: device_code.clone(),
        user_code,
        verification_uri,
        interval,
    });

    // Step 2: Poll for token
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;

        let poll_resp = match client
            .post(format!("{}/api/auth/device/poll", base))
            .json(&serde_json::json!({ "device_code": device_code }))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(GitHubMsg::Error(e.to_string()));
                return;
            }
        };

        let poll_body: serde_json::Value = match poll_resp.json().await {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.send(GitHubMsg::Error(e.to_string()));
                return;
            }
        };

        if let Some(token) = poll_body["access_token"].as_str() {
            let _ = tx.send(GitHubMsg::Token(token.to_string()));
            return;
        }

        let error = poll_body["error"].as_str().unwrap_or("");
        match error {
            "authorization_pending" | "slow_down" => continue,
            "expired_token" => {
                let _ = tx.send(GitHubMsg::Error("Device code expired. Please try again.".into()));
                return;
            }
            _ => {
                let _ = tx.send(GitHubMsg::Error(format!("Poll error: {}", error)));
                return;
            }
        }
    }
}

fn reqwest_test(url: &str) -> Result<(), String> {
    let url = format!("{}/api/pipelines", url.trim_end_matches('/'));
    let resp = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            reqwest::get(&url)
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())
        })
    })
    .join()
    .map_err(|_| "Thread panicked".to_string())?;

    resp.map(|_| ())
}
