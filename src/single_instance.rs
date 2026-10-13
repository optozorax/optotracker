#[cfg(target_os = "windows")]
mod platform {
    use crate::i18n::tr;
    use std::path::Path;
    use windows::core::{w, HSTRING};
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

    pub struct SingleInstanceGuard(HANDLE);

    impl SingleInstanceGuard {
        pub fn acquire(_directory: Option<&Path>) -> Result<Option<Self>, String> {
            // SAFETY: the name is a static, terminated UTF-16 string and the
            // returned kernel handle is retained by the guard until shutdown.
            let handle = unsafe {
                CreateMutexW(
                    None,
                    false,
                    w!("Local\\OptoTracker-67f3b775-8082-4ce3-a831-47a71996318d"),
                )
            }
            .map_err(|error| format!("Could not create the single-instance lock: {error}"))?;
            let already_running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
            if already_running {
                unsafe {
                    let _ = CloseHandle(handle);
                }
                Ok(None)
            } else {
                Ok(Some(Self(handle)))
            }
        }

        /// The copy that runs is found in the tray; it is not asked to come
        /// forward.
        pub fn show_running_instance(_directory: Option<&Path>) -> bool {
            false
        }

        pub fn on_second_instance(&self, _show: impl Fn() + Send + 'static) {}

        pub fn show_already_running_message() {
            // SAFETY: both strings remain alive for the call and the message
            // box has no owner.
            unsafe {
                MessageBoxW(
                    None,
                    &HSTRING::from(tr().already_running),
                    w!("OptoTracker"),
                    MB_OK | MB_ICONINFORMATION,
                );
            }
        }

        pub fn show_error_message(message: &str) {
            // SAFETY: both strings remain alive for the call and the message
            // box has no owner.
            unsafe {
                MessageBoxW(
                    None,
                    &HSTRING::from(message),
                    &HSTRING::from(tr().could_not_start),
                    MB_OK,
                );
            }
        }
    }

    impl Drop for SingleInstanceGuard {
        fn drop(&mut self) {
            // SAFETY: this guard uniquely owns the valid mutex handle.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use crate::i18n::tr;
    use eframe::egui;
    use std::fs::{self, File, TryLockError};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};

    /// Locked by the copy that runs, for as long as it runs.
    const LOCK_FILE: &str = "instance.lock";
    /// Where the copy that runs is told that another one was started.
    const SOCKET_FILE: &str = "instance.sock";

    pub struct SingleInstanceGuard {
        _lock: Option<File>,
        listener: Option<(UnixListener, PathBuf)>,
    }

    impl SingleInstanceGuard {
        /// The lock and the socket are kept in `directory`, the application
        /// folder.
        ///
        /// A folder that cannot hold a lock does not stop the application:
        /// it then runs unguarded, and whatever is wrong with the folder
        /// is reported where the data is read and saved.
        pub fn acquire(directory: Option<&Path>) -> Result<Option<Self>, String> {
            let unguarded = Self {
                _lock: None,
                listener: None,
            };
            let Some(directory) = directory else {
                return Ok(Some(unguarded));
            };
            let lock = fs::create_dir_all(directory).and_then(|()| {
                File::options()
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .open(directory.join(LOCK_FILE))
            });
            let Ok(lock) = lock else {
                return Ok(Some(unguarded));
            };
            match lock.try_lock() {
                Ok(()) => {}
                Err(TryLockError::WouldBlock) => return Ok(None),
                Err(TryLockError::Error(_)) => return Ok(Some(unguarded)),
            }

            // Whatever is at the address was left behind by a copy that no
            // longer runs: the lock is ours. Without a socket the lock still
            // holds; a second copy then has to say that this one runs.
            let socket = directory.join(SOCKET_FILE);
            let _ = fs::remove_file(&socket);
            let listener = UnixListener::bind(&socket)
                .ok()
                .map(|listener| (listener, socket));
            Ok(Some(Self {
                _lock: Some(lock),
                listener,
            }))
        }

        /// Asks the copy that runs to show its window, and says whether it
        /// could be reached.
        pub fn show_running_instance(directory: Option<&Path>) -> bool {
            directory
                .is_some_and(|directory| UnixStream::connect(directory.join(SOCKET_FILE)).is_ok())
        }

        /// Calls `show` whenever another copy was started and has left the
        /// window to this one.
        pub fn on_second_instance(&self, show: impl Fn() + Send + 'static) {
            let Some(listener) = self
                .listener
                .as_ref()
                .and_then(|(listener, _)| listener.try_clone().ok())
            else {
                return;
            };
            std::thread::spawn(move || {
                for connection in listener.incoming() {
                    if connection.is_ok() {
                        show();
                    }
                }
            });
        }

        pub fn show_already_running_message() {
            message_window("OptoTracker", tr().already_running);
        }

        pub fn show_error_message(message: &str) {
            eprintln!("{}: {message}", tr().could_not_start);
            message_window(tr().could_not_start, message);
        }
    }

    impl Drop for SingleInstanceGuard {
        fn drop(&mut self) {
            if let Some((_, socket)) = &self.listener {
                let _ = fs::remove_file(socket);
            }
        }
    }

    /// Says something to the user before, or instead of, the application
    /// itself. These systems have no message box that every desktop
    /// provides, so the message gets a small window of its own.
    fn message_window(title: &str, message: &str) {
        struct MessageWindow(String);

        impl eframe::App for MessageWindow {
            fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
                let frame = egui::Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(14.0);
                // Below the message, so that the button keeps its place
                // under a message of any length.
                egui::Panel::bottom("close")
                    .frame(frame)
                    .show_separator_line(false)
                    .show(ui, |ui| {
                        if ui.button("OK").clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                egui::CentralPanel::default().frame(frame).show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| ui.label(&self.0));
                });
            }
        }

        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title(title)
                .with_inner_size([460.0, 170.0]),
            ..Default::default()
        };
        let message = message.to_owned();
        let _ = eframe::run_native(
            title,
            options,
            Box::new(move |cc| {
                crate::configure_style(&cc.egui_ctx);
                crate::configure_fonts(&cc.egui_ctx);
                Ok(Box::new(MessageWindow(message)))
            }),
        );
    }
}

pub use platform::SingleInstanceGuard;

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::SingleInstanceGuard;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn a_second_copy_gives_way_and_asks_for_the_window() {
        // A short name: the address of a socket has little room.
        let folder = std::env::temp_dir().join(format!("ot-instance-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);

        let first = SingleInstanceGuard::acquire(Some(&folder)).unwrap();
        let first = first.expect("nothing else holds the lock");
        let (asked, requests) = mpsc::channel();
        first.on_second_instance(move || asked.send(()).unwrap());

        assert!(SingleInstanceGuard::acquire(Some(&folder))
            .unwrap()
            .is_none());
        assert!(SingleInstanceGuard::show_running_instance(Some(&folder)));
        requests.recv_timeout(Duration::from_secs(10)).unwrap();

        // The lock ends with the copy that held it.
        drop(first);
        assert!(!SingleInstanceGuard::show_running_instance(Some(&folder)));
        assert!(SingleInstanceGuard::acquire(Some(&folder))
            .unwrap()
            .is_some());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
