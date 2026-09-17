//! Desktop integration boundary.
//!
//! The UI calls this module using resolved paths. Platform-specific code lives
//! in the inner module and does not leak Windows APIs into the application.

use std::fs;
use std::path::Path;

use crate::paths::AppPaths;

pub fn install_current_executable(paths: &AppPaths) -> Result<(), String> {
    let source = std::env::current_exe()
        .map_err(|error| format!("Could not locate the running executable: {error}"))?;
    fs::create_dir_all(&paths.directory)
        .map_err(|error| format!("Could not create the application folder: {error}"))?;
    if !same_file(&source, &paths.installed_executable) {
        fs::copy(&source, &paths.installed_executable)
            .map_err(|error| format!("Could not install OptoTracker: {error}"))?;
    }
    Ok(())
}

pub fn is_installed(paths: &AppPaths) -> bool {
    paths.installed_executable.is_file()
}

pub fn open_app_directory(paths: &AppPaths) -> Result<(), String> {
    fs::create_dir_all(&paths.directory)
        .map_err(|error| format!("Could not create {}: {error}", paths.directory.display()))?;
    open::that(&paths.directory)
        .map_err(|error| format!("Could not open {}: {error}", paths.directory.display()))
}

fn same_file(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{install_current_executable, AppPaths};
    use std::os::windows::ffi::OsStrExt as _;
    use std::os::windows::process::CommandExt as _;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use windows::core::{Interface as _, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        FOLDERID_Desktop, IShellLinkW, SHGetKnownFolderPath, ShellLink, KF_FLAG_DEFAULT,
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
        .map_err(|error| format!("Could not update Windows autorun: {error}"))?;

        if status.success() {
            Ok(())
        } else {
            Err("Windows rejected the autorun change".to_owned())
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
                .map_err(|error| format!("Could not locate the Desktop folder: {error}"))?;
            let desktop = desktop_pointer
                .to_string()
                .map(PathBuf::from)
                .map_err(|error| format!("Could not read the Desktop folder path: {error}"));
            CoTaskMemFree(Some(desktop_pointer.0.cast()));
            let shortcut = desktop?.join("OptoTracker.lnk");

            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .map_err(|error| format!("Could not create a desktop shortcut: {error}"))?;
            let executable = wide_path(&paths.installed_executable);
            let working_directory = wide_path(&paths.directory);
            link.SetPath(PCWSTR(executable.as_ptr()))
                .map_err(|error| format!("Could not set the shortcut target: {error}"))?;
            link.SetWorkingDirectory(PCWSTR(working_directory.as_ptr()))
                .map_err(|error| format!("Could not set the shortcut folder: {error}"))?;
            link.SetDescription(windows::core::w!("Open OptoTracker"))
                .map_err(|error| format!("Could not set the shortcut description: {error}"))?;
            link.SetIconLocation(PCWSTR(executable.as_ptr()), 0)
                .map_err(|error| format!("Could not set the shortcut icon: {error}"))?;

            let persistence: IPersistFile = link
                .cast()
                .map_err(|error| format!("Could not save the desktop shortcut: {error}"))?;
            let shortcut = wide_path(&shortcut);
            persistence
                .Save(PCWSTR(shortcut.as_ptr()), true)
                .map_err(|error| format!("Could not save the desktop shortcut: {error}"))
        }
    }

    fn wide_path(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
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
}

pub use platform::{autorun_enabled, create_desktop_shortcut, set_autorun};
