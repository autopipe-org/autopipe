use eframe::egui;

use crate::claude_config;
use crate::config::{AppConfig, SshAuth};

#[derive(PartialEq)]
enum Tab {
    Setup,
    Connection,
    Ssh,
    Status,
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
}

impl AutoPipeApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = AppConfig::load();
        let (ssh_auth_type, ssh_key_path_input, ssh_password_input) = match &config.ssh_auth {
            SshAuth::Agent => (0, String::new(), String::new()),
            SshAuth::Key { key_path } => (1, key_path.clone(), String::new()),
            SshAuth::Password { password } => (2, String::new(), password.clone()),
        };

        Self {
            config,
            active_tab: Tab::Setup,
            ssh_password_input,
            ssh_key_path_input,
            ssh_auth_type,
            status_message: String::new(),
            should_minimize: false,
            minimized_to_tray: false,
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
                ui.selectable_value(&mut self.active_tab, Tab::Status, "Status");
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.active_tab {
            Tab::Setup => self.draw_setup_tab(ui),
            Tab::Connection => self.draw_connection_tab(ui),
            Tab::Ssh => self.draw_ssh_tab(ui),
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

        // Minimize to tray: hide the window
        if self.should_minimize {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.should_minimize = false;
            self.minimized_to_tray = true;
        }
    }
}

impl AutoPipeApp {
    /// Called from main loop to check if tray "Settings" was clicked.
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
        ui.heading("Step 1: Install Claude Desktop");
        ui.add_space(5.0);
        ui.label("Download and install the Claude Desktop app:");
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
        ui.label("This registers autopipe tools in Claude Desktop.");
        ui.label("After registration, restart Claude Desktop to load the tools.");

        ui.add_space(15.0);
        ui.separator();
        ui.add_space(10.0);

        // Step 4
        ui.heading("Step 4: Use Claude Desktop");
        ui.add_space(5.0);
        ui.label("Open Claude Desktop and start a conversation.");
        ui.label("You can ask Claude to:");
        ui.label("  - Search for existing workflows");
        ui.label("  - Create new bioinformatics pipelines");
        ui.label("  - Build, run, and monitor pipelines");
        ui.label("  - Upload workflows to the registry for sharing");
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
