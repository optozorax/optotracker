#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    Exit,
}

#[cfg(target_os = "windows")]
mod platform {
    use super::TrayAction;
    use crate::app_icon;
    use tray_icon::{
        menu::{Menu, MenuEvent, MenuItem},
        MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };

    pub struct TrayController {
        icon: TrayIcon,
        exit_item: MenuItem,
    }

    impl TrayController {
        pub fn new(running: bool) -> Result<Self, String> {
            let menu = Menu::new();
            let exit_item = MenuItem::new("Exit OptoTracker", true, None);
            menu.append(&exit_item).map_err(|error| error.to_string())?;

            let icon = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .with_tooltip(tooltip(running))
                .with_icon(tray_image(running)?)
                .build()
                .map_err(|error| error.to_string())?;

            Ok(Self { icon, exit_item })
        }

        pub fn set_running(&self, running: bool) -> Result<(), String> {
            self.icon
                .set_icon(Some(tray_image(running)?))
                .map_err(|error| error.to_string())?;
            self.icon
                .set_tooltip(Some(tooltip(running)))
                .map_err(|error| error.to_string())
        }

        pub fn poll_action(&self) -> Option<TrayAction> {
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == *self.exit_item.id() {
                    return Some(TrayAction::Exit);
                }
            }

            while let Ok(event) = TrayIconEvent::receiver().try_recv() {
                let belongs_to_us = event.id() == self.icon.id();
                let should_show = matches!(
                    event,
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } | TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    }
                );
                if belongs_to_us && should_show {
                    return Some(TrayAction::Show);
                }
            }
            None
        }
    }

    fn tray_image(running: bool) -> Result<tray_icon::Icon, String> {
        let icon = app_icon::tray_icon_data(running);
        tray_icon::Icon::from_rgba(icon.rgba, icon.width, icon.height)
            .map_err(|error| error.to_string())
    }

    fn tooltip(running: bool) -> &'static str {
        if running {
            "OptoTracker — timer running"
        } else {
            "OptoTracker"
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::TrayAction;

    pub struct TrayController;

    impl TrayController {
        pub fn new(_running: bool) -> Result<Self, String> {
            Err("system tray integration is unavailable on this platform".to_owned())
        }

        pub fn set_running(&self, _running: bool) -> Result<(), String> {
            Ok(())
        }

        pub fn poll_action(&self) -> Option<TrayAction> {
            None
        }
    }
}

pub use platform::TrayController;
