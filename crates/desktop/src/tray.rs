use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub enum TrayAction {
    ShowSettings,
    Quit,
}

pub struct AppTray {
    _tray: TrayIcon,
    show_id: MenuId,
    quit_id: MenuId,
}

impl AppTray {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let menu = Menu::new();
        let show_item = MenuItem::new("Settings", true, None);
        let quit_item = MenuItem::new("Quit", true, None);
        let show_id = show_item.id().clone();
        let quit_id = quit_item.id().clone();

        menu.append(&show_item)?;
        menu.append(&quit_item)?;

        let icon = create_default_icon();

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false) // left-click restores window, right-click shows menu
            .with_tooltip("AutoPipe - Running")
            .with_icon(icon)
            .build()?;

        Ok(Self {
            _tray: tray,
            show_id,
            quit_id,
        })
    }

    /// Check for menu events (non-blocking).
    pub fn poll_action(&self) -> Option<TrayAction> {
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if *event.id() == self.show_id {
                return Some(TrayAction::ShowSettings);
            }
            if *event.id() == self.quit_id {
                return Some(TrayAction::Quit);
            }
        }
        None
    }

    pub fn show_id(&self) -> &MenuId {
        &self.show_id
    }

    pub fn quit_id(&self) -> &MenuId {
        &self.quit_id
    }
}

fn create_default_icon() -> Icon {
    let size = 16;
    let mut rgba = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let idx = (y * size + x) * 4;
            rgba[idx] = 50;
            rgba[idx + 1] = 180;
            rgba[idx + 2] = 50;
            rgba[idx + 3] = 255;
        }
    }
    Icon::from_rgba(rgba, size as u32, size as u32).expect("Failed to create icon")
}
