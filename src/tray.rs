#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    Exit,
}

#[cfg(target_os = "windows")]
mod platform {
    use super::TrayAction;
    use crate::app_icon;
    use crate::i18n::tr;
    use std::sync::mpsc::{self, Receiver};
    use std::sync::Arc;
    use tray_icon::{
        menu::{Menu, MenuEvent, MenuItem},
        MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent, TrayIconId,
    };

    pub struct TrayController {
        icon: TrayIcon,
        exit_item: MenuItem,
        actions: Receiver<TrayAction>,
    }

    impl TrayController {
        /// `wake` is called whenever an action is queued, so the caller can
        /// sleep between tray interactions instead of polling for them.
        pub fn new(running: bool, wake: impl Fn() + Send + Sync + 'static) -> Result<Self, String> {
            let menu = Menu::new();
            let exit_item = MenuItem::new(tr().exit, true, None);
            menu.append(&exit_item).map_err(|error| error.to_string())?;

            // The handlers must be registered before the icon exists: both
            // libraries settle on "no handler" when their first event arrives.
            let (sender, actions) = mpsc::channel();
            let wake = Arc::new(wake);
            let icon_id = TrayIconId::new("opto-tracker");
            MenuEvent::set_event_handler(Some({
                let exit_id = exit_item.id().clone();
                let sender = sender.clone();
                let wake = Arc::clone(&wake);
                move |event: MenuEvent| {
                    if event.id == exit_id && sender.send(TrayAction::Exit).is_ok() {
                        wake();
                    }
                }
            }));
            TrayIconEvent::set_event_handler(Some({
                let icon_id = icon_id.clone();
                move |event: TrayIconEvent| {
                    let belongs_to_us = *event.id() == icon_id;
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
                    if belongs_to_us && should_show && sender.send(TrayAction::Show).is_ok() {
                        wake();
                    }
                }
            }));

            let icon = TrayIconBuilder::new()
                .with_id(icon_id)
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .with_tooltip(tooltip(running))
                .with_icon(tray_image(running)?)
                .build()
                .map_err(|error| error.to_string())?;

            Ok(Self {
                icon,
                exit_item,
                actions,
            })
        }

        /// Writes the menu in the language that the interface has changed
        /// to. The tooltip follows with the next [`Self::set_running`].
        pub fn translate(&self) {
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

    fn tooltip(running: bool) -> &'static str {
        if running {
            tr().tray_timer_running
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
        pub fn new(
            _running: bool,
            _wake: impl Fn() + Send + Sync + 'static,
        ) -> Result<Self, String> {
            Err("system tray integration is unavailable on this platform".to_owned())
        }

        pub fn translate(&self) {}

        pub fn set_running(&self, _running: bool) -> Result<(), String> {
            Ok(())
        }

        pub fn poll_action(&self) -> Option<TrayAction> {
            None
        }
    }
}

pub use platform::TrayController;
