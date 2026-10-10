//! Desktop integration boundary.
//!
//! The UI calls this module using resolved paths. Platform-specific code lives
//! in the inner module and does not leak Windows APIs into the application.

use std::fs;
use std::path::Path;

use crate::i18n::tr;
use crate::paths::AppPaths;

pub fn install_current_executable(paths: &AppPaths) -> Result<(), String> {
    let source = std::env::current_exe()
        .map_err(|error| format!("{}: {error}", tr().executable_not_found))?;
    fs::create_dir_all(&paths.directory)
        .map_err(|error| format!("{}: {error}", tr().folder_not_created))?;
    if !same_file(&source, &paths.installed_executable) {
        fs::copy(&source, &paths.installed_executable)
            .map_err(|error| format!("{}: {error}", tr().install_failed))?;
    }
    Ok(())
}

pub fn is_installed(paths: &AppPaths) -> bool {
    paths.installed_executable.is_file()
}

pub fn open_app_directory(paths: &AppPaths) -> Result<(), String> {
    let directory = paths.directory.display();
    fs::create_dir_all(&paths.directory)
        .map_err(|error| format!("{} {directory}: {error}", tr().could_not_create))?;
    open::that(&paths.directory)
        .map_err(|error| format!("{} {directory}: {error}", tr().could_not_open))
}

fn same_file(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{install_current_executable, tr, AppPaths};
    use std::os::windows::ffi::OsStrExt as _;
    use std::os::windows::process::CommandExt as _;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::core::{Interface as _, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        DefSubclassProc, FOLDERID_Desktop, IShellLinkW, RemoveWindowSubclass, SHGetKnownFolderPath,
        SetWindowSubclass, ShellLink, KF_FLAG_DEFAULT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, PostMessageW, WM_NCDESTROY, WM_SETTINGCHANGE,
    };

    const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "OptoTracker";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn reg_command() -> Command {
        let mut command = Command::new("reg.exe");
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }

    pub fn autorun_enabled() -> bool {
        reg_command()
            .args(["query", RUN_KEY, "/v", VALUE_NAME])
            .status()
            .is_ok_and(|status| status.success())
    }

    pub fn set_autorun(
        paths: &AppPaths,
        enabled: bool,
        start_minimized: bool,
    ) -> Result<(), String> {
        let status = if enabled {
            let command_line = if start_minimized {
                format!("\"{}\" --minimized", paths.installed_executable.display())
            } else {
                format!("\"{}\"", paths.installed_executable.display())
            };
            reg_command()
                .args([
                    "add",
                    RUN_KEY,
                    "/v",
                    VALUE_NAME,
                    "/t",
                    "REG_SZ",
                    "/d",
                    &command_line,
                    "/f",
                ])
                .status()
        } else {
            reg_command()
                .args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])
                .status()
        }
        .map_err(|error| format!("{}: {error}", tr().autorun_not_updated))?;

        if status.success() {
            Ok(())
        } else {
            Err(tr().autorun_rejected.to_owned())
        }
    }

    pub fn create_desktop_shortcut(paths: &AppPaths) -> Result<(), String> {
        install_current_executable(paths)?;

        // SAFETY: COM objects and the shell-allocated path are handled according
        // to their ownership contracts, and all path buffers remain alive for
        // the duration of each call.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let desktop_pointer = SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None)
                .map_err(|error| format!("{}: {error}", tr().desktop_not_found))?;
            let desktop = desktop_pointer
                .to_string()
                .map(PathBuf::from)
                .map_err(|error| format!("{}: {error}", tr().desktop_path_unreadable));
            CoTaskMemFree(Some(desktop_pointer.0.cast()));
            let shortcut = desktop?.join("OptoTracker.lnk");

            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .map_err(|error| format!("{}: {error}", tr().shortcut_not_created))?;
            let executable = wide_path(&paths.installed_executable);
            let working_directory = wide_path(&paths.directory);
            link.SetPath(PCWSTR(executable.as_ptr()))
                .map_err(|error| format!("{}: {error}", tr().shortcut_target_not_set))?;
            link.SetWorkingDirectory(PCWSTR(working_directory.as_ptr()))
                .map_err(|error| format!("{}: {error}", tr().shortcut_folder_not_set))?;
            link.SetDescription(&HSTRING::from(tr().shortcut_description))
                .map_err(|error| format!("{}: {error}", tr().shortcut_description_not_set))?;
            link.SetIconLocation(PCWSTR(executable.as_ptr()), 0)
                .map_err(|error| format!("{}: {error}", tr().shortcut_icon_not_set))?;

            let persistence: IPersistFile = link
                .cast()
                .map_err(|error| format!("{}: {error}", tr().shortcut_not_saved))?;
            let shortcut = wide_path(&shortcut);
            persistence
                .Save(PCWSTR(shortcut.as_ptr()), true)
                .map_err(|error| format!("{}: {error}", tr().shortcut_not_saved))
        }
    }

    fn wide_path(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    static KEEPS_WINDOW_THEME: AtomicBool = AtomicBool::new(false);
    const SETTINGS_FILTER_ID: usize = 1;

    /// Says whether the title bar has a theme of its own (`keep`) or follows
    /// the theme of Windows.
    ///
    /// Whenever Windows announces that some setting has changed, winit puts
    /// the title bar back into the theme of Windows, even after it was asked
    /// for a particular one. Those announcements are therefore kept away from
    /// winit for as long as the title bar keeps its own theme. Afterwards
    /// winit is sent one, so that it catches up with what it has missed.
    pub fn keep_window_theme(frame: &eframe::Frame, keep: bool) {
        use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};

        let Ok(window_handle) = frame.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(handle) = window_handle.as_raw() else {
            return;
        };
        let hwnd = HWND(handle.hwnd.get() as *mut core::ffi::c_void);
        let kept = KEEPS_WINDOW_THEME.swap(keep, Ordering::Relaxed);

        // SAFETY: the HWND is owned by eframe for the duration of this call,
        // which is made on the thread of the window. Installing the same
        // filter again changes nothing.
        unsafe {
            let _ = SetWindowSubclass(hwnd, Some(settings_filter), SETTINGS_FILTER_ID, 0);
            if kept && !keep {
                let _ = PostMessageW(Some(hwnd), WM_SETTINGCHANGE, WPARAM(0), LPARAM(0));
            }
        }
    }

    unsafe extern "system" fn settings_filter(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        // SAFETY: the arguments are those of the message being handled.
        unsafe {
            if message == WM_SETTINGCHANGE && KEEPS_WINDOW_THEME.load(Ordering::Relaxed) {
                // The theme is all that winit takes from this message.
                return DefWindowProcW(hwnd, message, wparam, lparam);
            }
            if message == WM_NCDESTROY {
                let _ = RemoveWindowSubclass(hwnd, Some(settings_filter), SETTINGS_FILTER_ID);
            }
            DefSubclassProc(hwnd, message, wparam, lparam)
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::AppPaths;

    pub fn autorun_enabled() -> bool {
        false
    }

    pub fn set_autorun(
        _paths: &AppPaths,
        _enabled: bool,
        _start_minimized: bool,
    ) -> Result<(), String> {
        Err("Autorun is currently supported only on Windows".to_owned())
    }

    pub fn create_desktop_shortcut(_paths: &AppPaths) -> Result<(), String> {
        Err("Desktop shortcuts are currently supported only on Windows".to_owned())
    }

    pub fn keep_window_theme(_frame: &eframe::Frame, _keep: bool) {}
}

pub use platform::{autorun_enabled, create_desktop_shortcut, keep_window_theme, set_autorun};
