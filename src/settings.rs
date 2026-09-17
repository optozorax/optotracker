use std::fs;
use std::path::{Path, PathBuf};

pub const DATA_FILE: &str = "data.json";
pub const PERSISTENCE_FILE: &str = "temp.ron";
pub const EXECUTABLE_FILE: &str = "OptoTracker.exe";

pub fn app_directory() -> Result<PathBuf, String> {
    directories::BaseDirs::new()
        .map(|directories| directories.data_dir().join("OptoTracker"))
        .ok_or_else(|| "Could not locate the current user's application-data folder".to_owned())
}

pub fn data_path() -> Result<PathBuf, String> {
    Ok(app_directory()?.join(DATA_FILE))
}

pub fn persistence_path() -> Result<PathBuf, String> {
    Ok(app_directory()?.join(PERSISTENCE_FILE))
}

pub fn installed_executable_path() -> Result<PathBuf, String> {
    Ok(app_directory()?.join(EXECUTABLE_FILE))
}

pub fn install_current_executable() -> Result<PathBuf, String> {
    let source = std::env::current_exe()
        .map_err(|error| format!("Could not locate the running executable: {error}"))?;
    let destination = installed_executable_path()?;
    fs::create_dir_all(app_directory()?)
        .map_err(|error| format!("Could not create the application folder: {error}"))?;
    if !same_file(&source, &destination) {
        fs::copy(&source, &destination)
            .map_err(|error| format!("Could not install OptoTracker: {error}"))?;
    }
    Ok(destination)
}

pub fn is_installed() -> bool {
    installed_executable_path().is_ok_and(|path| path.is_file())
}

pub fn open_app_directory() -> Result<(), String> {
    let directory = app_directory()?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Could not create {}: {error}", directory.display()))?;
    open::that(&directory)
        .map_err(|error| format!("Could not open {}: {error}", directory.display()))
}

fn same_file(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(target_os = "windows")]
mod windows_integration {
    use super::{app_directory, installed_executable_path};
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

    pub fn set_autorun(enabled: bool, start_minimized: bool) -> Result<(), String> {
        let status = if enabled {
            let executable = installed_executable_path()?;
            let command_line = if start_minimized {
                format!("\"{}\" --minimized", executable.display())
            } else {
                format!("\"{}\"", executable.display())
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

    pub fn create_desktop_shortcut() -> Result<(), String> {
        let executable = super::install_current_executable()?;
        let working_directory = app_directory()?;

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
            let executable_wide = wide_path(&executable);
            let working_directory_wide = wide_path(&working_directory);
            link.SetPath(PCWSTR(executable_wide.as_ptr()))
                .map_err(|error| format!("Could not set the shortcut target: {error}"))?;
            link.SetWorkingDirectory(PCWSTR(working_directory_wide.as_ptr()))
                .map_err(|error| format!("Could not set the shortcut folder: {error}"))?;
            link.SetDescription(windows::core::w!("Open OptoTracker"))
                .map_err(|error| format!("Could not set the shortcut description: {error}"))?;
            link.SetIconLocation(PCWSTR(executable_wide.as_ptr()), 0)
                .map_err(|error| format!("Could not set the shortcut icon: {error}"))?;

            let persistence: IPersistFile = link
                .cast()
                .map_err(|error| format!("Could not save the desktop shortcut: {error}"))?;
            let shortcut_wide = wide_path(&shortcut);
            persistence
                .Save(PCWSTR(shortcut_wide.as_ptr()), true)
                .map_err(|error| format!("Could not save the desktop shortcut: {error}"))
        }
    }

    fn wide_path(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }
}

#[cfg(not(target_os = "windows"))]
mod windows_integration {
    pub fn autorun_enabled() -> bool {
        false
    }

    pub fn set_autorun(_enabled: bool, _start_minimized: bool) -> Result<(), String> {
        Err("Autorun is currently supported only on Windows".to_owned())
    }

    pub fn create_desktop_shortcut() -> Result<(), String> {
        Err("Desktop shortcuts are currently supported only on Windows".to_owned())
    }
}

pub use windows_integration::{autorun_enabled, create_desktop_shortcut, set_autorun};
