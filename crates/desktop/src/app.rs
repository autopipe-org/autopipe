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
    Plugins,
    Status,
}

/// Messages from the plugin background thread.
enum PluginMsg {
    /// Registry plugin list fetched.
    RegistryList(Vec<RegistryPlugin>),
    /// Plugin installed successfully.
    InstallOk(String),
    /// An error occurred.
    Error(String),
}

/// A plugin entry from the registry API.
#[derive(Clone)]
struct RegistryPlugin {
    name: String,
    description: String,
    author: String,
    version: String,
    extensions: Vec<String>,
    github_url: String,
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
    // Plugin state
    installed_plugins: Vec<PluginInfo>,
    plugins_loaded: bool,
    plugin_rx: Option<mpsc::Receiver<PluginMsg>>,
    plugin_registry: Vec<RegistryPlugin>,
    plugin_registry_loaded: bool,
    plugin_loading: bool,
    plugin_search: String,
    plugin_confirm: Option<RegistryPlugin>,
    plugin_status: String,
}

/// Info about an installed plugin, read from manifest.json.
struct PluginInfo {
    name: String,
    version: String,
    description: String,
    extensions: Vec<String>,
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
            installed_plugins: Vec::new(),
            plugins_loaded: false,
            plugin_rx: None,
            plugin_registry: Vec::new(),
            plugin_registry_loaded: false,
            plugin_loading: false,
            plugin_search: String::new(),
            plugin_confirm: None,
            plugin_status: String::new(),
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
                ui.selectable_value(&mut self.active_tab, Tab::Plugins, "Plugins");
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

        // Process plugin messages
        if let Some(rx) = self.plugin_rx.take() {
            let mut keep_rx = true;
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    PluginMsg::RegistryList(list) => {
                        self.plugin_registry = list;
                        self.plugin_registry_loaded = true;
                        self.plugin_loading = false;
                        keep_rx = false;
                    }
                    PluginMsg::InstallOk(name) => {
                        self.plugin_status = format!("Installed \"{}\" successfully.", name);
                        self.plugin_loading = false;
                        self.installed_plugins =
                            scan_installed_plugins(&self.config.full_plugins_dir());
                        keep_rx = false;
                    }
                    PluginMsg::Error(e) => {
                        self.plugin_status = format!("Error: {}", e);
                        self.plugin_loading = false;
                        keep_rx = false;
                    }
                }
            }
            if keep_rx {
                self.plugin_rx = Some(rx);
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| match self.active_tab {
            Tab::Setup => self.draw_setup_tab(ui),
            Tab::Connection => self.draw_connection_tab(ui),
            Tab::Ssh => self.draw_ssh_tab(ui),
            Tab::GitHub => self.draw_github_tab(ui),
            Tab::Plugins => self.draw_plugins_tab(ui),
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
        egui::ScrollArea::vertical().show(ui, |ui| {
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
        });
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

    fn draw_plugins_tab(&mut self, ui: &mut egui::Ui) {
        // Scan installed plugins on first load
        if !self.plugins_loaded {
            self.installed_plugins = scan_installed_plugins(&self.config.full_plugins_dir());
            self.plugins_loaded = true;
        }

        // Install confirmation dialog (modal window)
        let mut confirm_install = false;
        let mut cancel_confirm = false;
        if let Some(ref plugin) = self.plugin_confirm {
            let name = plugin.name.clone();
            let author = plugin.author.clone();
            egui::Window::new("Install Plugin")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    ui.add_space(5.0);
                    ui.colored_label(
                        egui::Color32::YELLOW,
                        "⚠ Warning: Plugins execute JavaScript in your browser.",
                    );
                    ui.add_space(10.0);
                    ui.label(format!("Plugin: {}", &name));
                    ui.label(format!("Author: {}", &author));
                    ui.add_space(10.0);
                    ui.label("Do you trust this plugin author?");
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Install").clicked() {
                            confirm_install = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel_confirm = true;
                        }
                    });
                });
        }

        if cancel_confirm {
            self.plugin_confirm = None;
        }

        if confirm_install {
            if let Some(plugin) = self.plugin_confirm.take() {
                let github_url = plugin.github_url.clone();
                let plugins_dir = self.config.full_plugins_dir();
                let plugin_name = plugin.name.clone();
                let (tx, rx) = mpsc::channel();
                self.plugin_rx = Some(rx);
                self.plugin_loading = true;
                self.plugin_status = format!("Installing {}...", &plugin_name);
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async {
                        install_plugin_from_github(&github_url, &plugins_dir, &plugin_name, tx)
                            .await;
                    });
                });
            }
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Viewer Plugins");
            ui.add_space(5.0);
            ui.label(
                "Manage plugins that extend the Results Viewer with custom file format support.",
            );
            ui.add_space(5.0);

            // Plugin creation guide link
            ui.horizontal(|ui| {
                ui.label("Want to create your own plugin?");
                let guide_url = format!(
                    "{}/plugins/guide",
                    self.config.registry_url.trim_end_matches('/')
                );
                ui.hyperlink_to("Plugin Creation Guide", &guide_url);
            });

            ui.add_space(10.0);

            // Plugins directory
            ui.label("Plugins Directory:");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.config.plugins_dir)
                        .desired_width(350.0),
                );
                if ui.button("Open").clicked() {
                    let dir = self.config.full_plugins_dir();
                    let path = std::path::Path::new(&dir);
                    if !path.exists() {
                        let _ = std::fs::create_dir_all(path);
                    }
                    let _ = open::that(&dir);
                }
            });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);

            // --- Registry ---
            ui.heading("Install from Registry");
            ui.add_space(5.0);

            // Status message
            if !self.plugin_status.is_empty() {
                ui.colored_label(egui::Color32::LIGHT_BLUE, &self.plugin_status.clone());
                ui.add_space(5.0);
            }

            ui.horizontal(|ui| {
                ui.label("Search:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.plugin_search).desired_width(250.0),
                );
                let btn_text = if self.plugin_loading {
                    "Loading..."
                } else {
                    "Search"
                };
                if ui.add_enabled(!self.plugin_loading, egui::Button::new(btn_text)).clicked() {
                    let registry_url = self.config.registry_url.clone();
                    let query = self.plugin_search.clone();
                    let (tx, rx) = mpsc::channel();
                    self.plugin_rx = Some(rx);
                    self.plugin_loading = true;
                    self.plugin_status = String::new();
                    std::thread::spawn(move || {
                        let rt = tokio::runtime::Runtime::new().unwrap();
                        rt.block_on(async {
                            fetch_registry_plugins(&registry_url, &query, tx).await;
                        });
                    });
                }
                if !self.plugin_registry_loaded && !self.plugin_loading {
                    if ui.button("Load All").clicked() {
                        let registry_url = self.config.registry_url.clone();
                        let (tx, rx) = mpsc::channel();
                        self.plugin_rx = Some(rx);
                        self.plugin_loading = true;
                        self.plugin_status = String::new();
                        std::thread::spawn(move || {
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async {
                                fetch_registry_plugins(&registry_url, "", tx).await;
                            });
                        });
                    }
                }
            });
            ui.add_space(5.0);

            if self.plugin_loading && self.plugin_registry.is_empty() {
                ui.spinner();
            }

            if self.plugin_registry.is_empty() && self.plugin_registry_loaded {
                ui.label("No plugins found in registry.");
            }

            // Collect install actions to avoid borrow issues
            let installed_names: Vec<String> =
                self.installed_plugins.iter().map(|p| p.name.clone()).collect();
            let mut install_plugin: Option<RegistryPlugin> = None;

            for plugin in &self.plugin_registry {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong(&plugin.name);
                        ui.label(format!("v{}", plugin.version));
                        ui.label(format!("by {}", plugin.author));
                    });
                    if !plugin.description.is_empty() {
                        ui.label(&plugin.description);
                    }
                    if !plugin.extensions.is_empty() {
                        ui.label(format!(
                            "Formats: {}",
                            plugin
                                .extensions
                                .iter()
                                .map(|e| format!(".{}", e))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                    let already = installed_names.contains(&plugin.name);
                    if already {
                        ui.colored_label(egui::Color32::GREEN, "✓ Installed");
                    } else if ui.button("Install").clicked() {
                        install_plugin = Some(plugin.clone());
                    }
                });
                ui.add_space(3.0);
            }

            if let Some(p) = install_plugin {
                self.plugin_confirm = Some(p);
            }

            ui.add_space(15.0);
            ui.separator();
            ui.add_space(10.0);

            // --- Built-in Viewers ---
            ui.heading("Built-in Viewers");
            ui.add_space(5.0);
            ui.label("The Results Viewer includes built-in support for:");
            ui.add_space(3.0);
            ui.label("  Images: PNG, JPG, GIF, SVG, WebP, BMP");
            ui.label("  Documents: PDF");
            ui.label("  Text: TXT, LOG, CSV, TSV, JSON, YAML, XML, FASTQ");
            ui.label("  Genomics (igv.js): BAM, CRAM, VCF, BCF, BED, GFF, GTF, FASTA");
            ui.label("  Single-cell (jsfive): h5ad");
        });
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

async fn fetch_registry_plugins(
    registry_url: &str,
    query: &str,
    tx: mpsc::Sender<PluginMsg>,
) {
    let client = reqwest::Client::new();
    let base = registry_url.trim_end_matches('/');
    let url = if query.is_empty() {
        format!("{}/api/plugins", base)
    } else {
        // Manual percent-encoding for the query
        let encoded: String = query
            .chars()
            .map(|c| match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
                ' ' => "+".to_string(),
                _ => format!("%{:02X}", c as u32),
            })
            .collect();
        format!("{}/api/plugins?q={}", base, encoded)
    };

    let resp = match client
        .get(&url)
        .header("User-Agent", "autopipe-desktop")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(PluginMsg::Error(format!("Registry request failed: {}", e)));
            return;
        }
    };

    if !resp.status().is_success() {
        let _ = tx.send(PluginMsg::Error(format!(
            "Registry returned status {}",
            resp.status()
        )));
        return;
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.send(PluginMsg::Error(format!("Invalid JSON: {}", e)));
            return;
        }
    };

    let arr = match body.as_array() {
        Some(a) => a,
        None => {
            let _ = tx.send(PluginMsg::RegistryList(Vec::new()));
            return;
        }
    };

    let plugins: Vec<RegistryPlugin> = arr
        .iter()
        .map(|v| RegistryPlugin {
            name: v["name"].as_str().unwrap_or("").to_string(),
            description: v["description"].as_str().unwrap_or("").to_string(),
            author: v["author"].as_str().unwrap_or("").to_string(),
            version: v["version"].as_str().unwrap_or("").to_string(),
            extensions: v["extensions"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|e| e.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            github_url: v["github_url"].as_str().unwrap_or("").to_string(),
        })
        .collect();

    let _ = tx.send(PluginMsg::RegistryList(plugins));
}

async fn install_plugin_from_github(
    github_url: &str,
    plugins_dir: &str,
    plugin_name: &str,
    tx: mpsc::Sender<PluginMsg>,
) {
    let client = reqwest::Client::new();

    // Parse GitHub URL → raw content base URL
    // e.g. https://github.com/owner/repo → https://raw.githubusercontent.com/owner/repo/main/
    let raw_base = match parse_github_raw_url(github_url) {
        Some(url) => url,
        None => {
            let _ = tx.send(PluginMsg::Error(format!(
                "Cannot parse GitHub URL: {}",
                github_url
            )));
            return;
        }
    };

    // Download manifest.json
    let manifest_url = format!("{}/manifest.json", raw_base);
    let manifest_text = match client
        .get(&manifest_url)
        .header("User-Agent", "autopipe-desktop")
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => match r.text().await {
            Ok(t) => t,
            Err(e) => {
                let _ = tx.send(PluginMsg::Error(format!("Failed to read manifest: {}", e)));
                return;
            }
        },
        Ok(r) => {
            let _ = tx.send(PluginMsg::Error(format!(
                "manifest.json not found ({})",
                r.status()
            )));
            return;
        }
        Err(e) => {
            let _ = tx.send(PluginMsg::Error(format!("Download failed: {}", e)));
            return;
        }
    };

    let manifest: serde_json::Value = match serde_json::from_str(&manifest_text) {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.send(PluginMsg::Error(format!("Invalid manifest JSON: {}", e)));
            return;
        }
    };

    // Create plugin directory
    let dest = std::path::PathBuf::from(plugins_dir).join(plugin_name);
    if let Err(e) = std::fs::create_dir_all(&dest) {
        let _ = tx.send(PluginMsg::Error(format!("Cannot create directory: {}", e)));
        return;
    }

    // Save manifest.json
    if let Err(e) = std::fs::write(dest.join("manifest.json"), &manifest_text) {
        let _ = tx.send(PluginMsg::Error(format!("Failed to write manifest: {}", e)));
        return;
    }

    // Download entry file (e.g. index.js)
    let entry = manifest["entry"].as_str().unwrap_or("index.js");
    let entry_url = format!("{}/{}", raw_base, entry);
    match client
        .get(&entry_url)
        .header("User-Agent", "autopipe-desktop")
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => {
            if let Ok(bytes) = r.bytes().await {
                let _ = std::fs::write(dest.join(entry), &bytes);
            }
        }
        _ => {
            let _ = tx.send(PluginMsg::Error(format!("Failed to download {}", entry)));
            return;
        }
    }

    // Download style file if specified
    if let Some(style) = manifest["style"].as_str() {
        let style_url = format!("{}/{}", raw_base, style);
        if let Ok(r) = client
            .get(&style_url)
            .header("User-Agent", "autopipe-desktop")
            .send()
            .await
        {
            if r.status().is_success() {
                if let Ok(bytes) = r.bytes().await {
                    let _ = std::fs::write(dest.join(style), &bytes);
                }
            }
        }
    }

    let _ = tx.send(PluginMsg::InstallOk(plugin_name.to_string()));
}

/// Parse a GitHub URL into a raw.githubusercontent.com base URL.
/// Supports: https://github.com/owner/repo[/tree/branch[/subpath]]
fn parse_github_raw_url(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/').trim_end_matches(".git");
    // Try to match /tree/branch/subpath pattern
    if let Some(pos) = url.find("/tree/") {
        let prefix = &url[..pos]; // https://github.com/owner/repo
        let after = &url[pos + 6..]; // branch/subpath
        let owner_repo = prefix.trim_start_matches("https://github.com/");
        let (branch, subpath) = match after.find('/') {
            Some(i) => (&after[..i], format!("/{}", &after[i + 1..])),
            None => (after, String::new()),
        };
        Some(format!(
            "https://raw.githubusercontent.com/{}/{}{}",
            owner_repo, branch, subpath
        ))
    } else {
        // https://github.com/owner/repo → try main branch
        let owner_repo = url.trim_start_matches("https://github.com/");
        if owner_repo.contains('/') {
            Some(format!(
                "https://raw.githubusercontent.com/{}/main",
                owner_repo
            ))
        } else {
            None
        }
    }
}

fn scan_installed_plugins(plugins_dir: &str) -> Vec<PluginInfo> {
    let path = std::path::Path::new(plugins_dir);
    if !path.exists() {
        return Vec::new();
    }
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut plugins = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let manifest_path = entry.path().join("manifest.json");
        if !manifest_path.exists() {
            continue;
        }
        let content = match std::fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };
        plugins.push(PluginInfo {
            name: json["name"].as_str().unwrap_or("(unknown)").to_string(),
            version: json["version"].as_str().unwrap_or("0.0.0").to_string(),
            description: json["description"].as_str().unwrap_or("").to_string(),
            extensions: json["extensions"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    plugins
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
