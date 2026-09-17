#[cfg(target_os = "windows")]
mod platform {
    use windows::core::w;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

    pub struct SingleInstanceGuard(HANDLE);

    impl SingleInstanceGuard {
        pub fn acquire() -> Result<Option<Self>, String> {
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

        pub fn show_already_running_message() {
            // SAFETY: both strings are static and the message box has no owner.
            unsafe {
                MessageBoxW(
                    None,
                    w!("OptoTracker is already running. Open it from the system tray."),
                    w!("OptoTracker"),
                    MB_OK | MB_ICONINFORMATION,
                );
            }
        }

        pub fn show_error_message(message: &str) {
            use windows::core::PCWSTR;

            let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
            // SAFETY: both UTF-16 strings are terminated and remain alive for the call.
            unsafe {
                MessageBoxW(
                    None,
                    PCWSTR(message.as_ptr()),
                    w!("OptoTracker could not start"),
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
    pub struct SingleInstanceGuard;

    impl SingleInstanceGuard {
        pub fn acquire() -> Result<Option<Self>, String> {
            Ok(Some(Self))
        }

        pub fn show_already_running_message() {}

        pub fn show_error_message(message: &str) {
            eprintln!("OptoTracker could not start: {message}");
        }
    }
}

pub use platform::SingleInstanceGuard;
