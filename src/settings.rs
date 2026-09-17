use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use chrono::Datelike as _;

pub const DATA_FILE: &str = "data.json";
pub const PERSISTENCE_FILE: &str = "temp.ron";
pub const EXECUTABLE_FILE: &str = "OptoTracker.exe";
const LEGACY_DATA_FILE: &str = "time_tracker_store.json";
const LEGACY_PERSISTENCE_FILE: &str = "OptoTracker.ron";

pub fn app_directory() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("OptoTracker")
}

pub fn data_path() -> PathBuf {
    app_directory().join(DATA_FILE)
}

pub fn persistence_path() -> PathBuf {
    app_directory().join(PERSISTENCE_FILE)
}

pub fn installed_executable_path() -> PathBuf {
    app_directory().join(EXECUTABLE_FILE)
}

pub fn prepare_data(source: &Path) -> Result<PathBuf, String> {
    let directory = app_directory();
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Could not create {}: {error}", directory.display()))?;

    let destination = data_path();
    let legacy_data = directory.join(LEGACY_DATA_FILE);
    if !destination.exists() && legacy_data.exists() {
        fs::rename(&legacy_data, &destination).map_err(|error| {
            format!(
                "Could not rename {} to {}: {error}",
                legacy_data.display(),
                destination.display()
            )
        })?;
    } else if !destination.exists() && source.exists() && !same_file(source, &destination) {
        fs::copy(source, &destination).map_err(|error| {
            format!("Could not copy data to {}: {error}", destination.display())
        })?;
    }

    let persistence = persistence_path();
    let legacy_persistence = directory.join(LEGACY_PERSISTENCE_FILE);
    if !persistence.exists() && legacy_persistence.exists() {
        fs::rename(&legacy_persistence, &persistence).map_err(|error| {
            format!(
                "Could not rename {} to {}: {error}",
                legacy_persistence.display(),
                persistence.display()
            )
        })?;
    }
    Ok(destination)
}

pub fn install_current_executable() -> Result<PathBuf, String> {
    let source = std::env::current_exe()
        .map_err(|error| format!("Could not locate the running executable: {error}"))?;
    let destination = installed_executable_path();
    fs::create_dir_all(app_directory())
        .map_err(|error| format!("Could not create the application folder: {error}"))?;
    if !same_file(&source, &destination) {
        fs::copy(&source, &destination)
            .map_err(|error| format!("Could not install OptoTracker: {error}"))?;
    }
    Ok(destination)
}

pub fn is_installed() -> bool {
    installed_executable_path().is_file()
}

pub fn back_up_if_due(source: &Path) -> Result<(), String> {
    if !source.is_file() {
        return Ok(());
    }
    let week = chrono::Local::now().iso_week();
    let directory = app_directory().join("backups");
    back_up_for_week(source, &directory, week.year(), week.week())
}

fn back_up_for_week(source: &Path, directory: &Path, year: i32, week: u32) -> Result<(), String> {
    let backup = directory.join(format!("data-{}-W{:02}.json", year, week));
    if backup.exists() {
        return Ok(());
    }

    fs::create_dir_all(directory)
        .map_err(|error| format!("Could not create the backup folder: {error}"))?;
    let temporary = directory.join(format!(".backup-{}.tmp", std::process::id()));
    fs::copy(source, &temporary)
        .map_err(|error| format!("Could not create the weekly backup: {error}"))?;
    fs::rename(&temporary, &backup)
        .map_err(|error| format!("Could not finish the weekly backup: {error}"))
}

pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("data");
    let temporary = path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    file.write_all(contents)
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    drop(file);

    let result = replace_file(path, &temporary);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(target_os = "windows")]
fn replace_file(destination: &Path, replacement: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_WRITE_THROUGH};

    if !destination.exists() {
        return fs::rename(replacement, destination).map_err(|error| error.to_string());
    }
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let replacement: Vec<u16> = replacement
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both UTF-16 path buffers are terminated and remain alive for the call.
    unsafe {
        ReplaceFileW(
            PCWSTR(destination.as_ptr()),
            PCWSTR(replacement.as_ptr()),
            PCWSTR::null(),
            REPLACEFILE_WRITE_THROUGH,
            None,
            None,
        )
    }
    .map_err(|error| error.to_string())
}

#[cfg(not(target_os = "windows"))]
fn replace_file(destination: &Path, replacement: &Path) -> Result<(), String> {
    fs::rename(replacement, destination).map_err(|error| error.to_string())
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
            let executable = installed_executable_path();
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

    pub fn open_app_directory() -> Result<(), String> {
        Command::new("explorer.exe")
            .arg(app_directory())
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Could not open the application folder: {error}"))
    }

    pub fn create_desktop_shortcut() -> Result<(), String> {
        let executable = super::install_current_executable()?;
        let working_directory = app_directory();

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

    pub fn open_app_directory() -> Result<(), String> {
        Err("Opening the application folder is currently supported only on Windows".to_owned())
    }

    pub fn create_desktop_shortcut() -> Result<(), String> {
        Err("Desktop shortcuts are currently supported only on Windows".to_owned())
    }
}

pub use windows_integration::{
    autorun_enabled, create_desktop_shortcut, open_app_directory, set_autorun,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_directory(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "opto-tracker-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn atomic_write_replaces_existing_contents() {
        let directory = test_directory("atomic");
        let path = directory.join("store.json");
        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn weekly_backup_is_created_only_once() {
        let directory = test_directory("backup");
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("store.json");
        let backups = directory.join("backups");
        fs::write(&source, b"original").unwrap();
        back_up_for_week(&source, &backups, 2026, 38).unwrap();
        fs::write(&source, b"changed").unwrap();
        back_up_for_week(&source, &backups, 2026, 38).unwrap();
        assert_eq!(
            fs::read(backups.join("data-2026-W38.json")).unwrap(),
            b"original"
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
