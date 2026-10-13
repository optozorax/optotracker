use std::path::PathBuf;

pub const DATA_FILE: &str = "data.json";
pub const PERSISTENCE_FILE: &str = "temp.ron";

/// What an installed copy is called in the application folder: the
/// executable, or on macOS the bundle that holds it.
#[cfg(target_os = "windows")]
pub const INSTALLED_APPLICATION: &str = "OptoTracker.exe";
#[cfg(target_os = "macos")]
pub const INSTALLED_APPLICATION: &str = "OptoTracker.app";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const INSTALLED_APPLICATION: &str = "opto-tracker";

/// The executable of an installed copy, from the application folder.
#[cfg(not(target_os = "macos"))]
const INSTALLED_EXECUTABLE: &str = INSTALLED_APPLICATION;
#[cfg(target_os = "macos")]
const INSTALLED_EXECUTABLE: &str = "OptoTracker.app/Contents/MacOS/OptoTracker";

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub directory: PathBuf,
    pub data: PathBuf,
    pub persistence: PathBuf,
    pub installed_executable: PathBuf,
}

impl AppPaths {
    /// The folder that everything is kept in: `%APPDATA%\OptoTracker` on
    /// Windows, `~/Library/Application Support/OptoTracker` on macOS and
    /// `~/.local/share/OptoTracker` on Linux.
    pub fn discover() -> Result<Self, String> {
        let directory = directories::BaseDirs::new()
            .map(|directories| directories.data_dir().join("OptoTracker"))
            .ok_or_else(|| {
                "Could not locate the current user's application-data folder".to_owned()
            })?;
        Ok(Self::in_directory(directory))
    }

    pub fn in_directory(directory: PathBuf) -> Self {
        Self {
            data: directory.join(DATA_FILE),
            persistence: directory.join(PERSISTENCE_FILE),
            installed_executable: directory.join(INSTALLED_EXECUTABLE),
            directory,
        }
    }
}
