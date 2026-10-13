use crate::app_icon;
use crate::i18n::tr;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent, TrayIconId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    Exit,
}

/// The icon in the notification area of Windows, in the menu bar of macOS,
/// or in the tray of a Linux desktop that has one.
pub struct TrayController {
    icon: TrayIcon,
    show_item: Option<MenuItem>,
    exit_item: MenuItem,
    actions: Receiver<TrayAction>,
}

impl TrayController {
    /// `wake` is called whenever an action is queued, so the caller can
    /// sleep between tray interactions instead of polling for them.
    pub fn new(running: bool, wake: impl Fn() + Send + Sync + 'static) -> Result<Self, String> {
        let menu = Menu::new();
        // On Windows a click on the icon shows the window. A click opens
        // the menu on macOS, and on some desktops of Linux, so there the
        // menu has to offer the window too.
        let show_item =
            (!cfg!(target_os = "windows")).then(|| MenuItem::new(tr().open_app, true, None));
        if let Some(show_item) = &show_item {
            menu.append(show_item).map_err(|error| error.to_string())?;
        }
        let exit_item = MenuItem::new(tr().exit, true, None);
        menu.append(&exit_item).map_err(|error| error.to_string())?;

        // The handlers must be registered before the icon exists: both
        // libraries settle on "no handler" when their first event arrives.
        let (sender, actions) = mpsc::channel();
        let wake = Arc::new(wake);
        let icon_id = TrayIconId::new("opto-tracker");
        MenuEvent::set_event_handler(Some({
            let show_id = show_item.as_ref().map(|item| item.id().clone());
            let exit_id = exit_item.id().clone();
            let sender = sender.clone();
            let wake = Arc::clone(&wake);
            move |event: MenuEvent| {
                let action = if event.id == exit_id {
                    TrayAction::Exit
                } else if show_id.as_ref() == Some(&event.id) {
                    TrayAction::Show
                } else {
                    return;
                };
                if sender.send(action).is_ok() {
                    wake();
                }
            }
        }));
        TrayIconEvent::set_event_handler(Some({
            let icon_id = icon_id.clone();
            move |event: TrayIconEvent| {
                let belongs_to_us = *event.id() == icon_id;
                // A click in the menu bar of macOS opens the menu.
                let should_show = !cfg!(target_os = "macos")
                    && matches!(
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
                if belongs_to_us && should_show && sender.send(TrayAction::Show).is_ok() {
                    wake();
                }
            }
        }));

        let mut icon = TrayIconBuilder::new()
            .with_id(icon_id)
            .with_menu(Box::new(menu))
            // In the menu bar of macOS every icon opens its menu.
            .with_menu_on_left_click(cfg!(target_os = "macos"))
            .with_tooltip(tooltip(running))
            .with_icon(tray_image(running)?);
        if cfg!(target_os = "linux") {
            // What a tray of Linux calls the icon. macOS would write it
            // out next to the icon.
            icon = icon.with_title("OptoTracker");
        }
        let icon = icon.build().map_err(|error| error.to_string())?;

        Ok(Self {
            icon,
            show_item,
            exit_item,
            actions,
        })
    }

    /// Writes the menu in the language that the interface has changed
    /// to. The tooltip follows with the next [`Self::set_running`].
    pub fn translate(&self) {
        if let Some(show_item) = &self.show_item {
            show_item.set_text(tr().open_app);
        }
        self.exit_item.set_text(tr().exit);
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
        let mut action = None;
        for queued in self.actions.try_iter() {
            if queued == TrayAction::Exit {
                return Some(TrayAction::Exit);
            }
            action = Some(queued);
        }
        action
    }
}

fn tray_image(running: bool) -> Result<tray_icon::Icon, String> {
    let icon = app_icon::tray_icon_data(running);
    tray_icon::Icon::from_rgba(icon.rgba, icon.width, icon.height)
        .map_err(|error| error.to_string())
}

#[cfg(not(target_os = "linux"))]
fn tooltip(running: bool) -> &'static str {
    if running {
        tr().tray_timer_running
    } else {
        "OptoTracker"
    }
}

/// A tray of Linux shows the tooltip under the name of the icon, so the
/// name is not said again.
#[cfg(target_os = "linux")]
fn tooltip(running: bool) -> &'static str {
    if running {
        tr().timer_running
    } else {
        ""
    }
}
