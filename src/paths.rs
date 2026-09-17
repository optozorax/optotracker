use std::path::PathBuf;

pub const DATA_FILE: &str = "data.json";
pub const PERSISTENCE_FILE: &str = "temp.ron";

#[cfg(target_os = "windows")]
pub const EXECUTABLE_FILE: &str = "OptoTracker.exe";
#[cfg(not(target_os = "windows"))]
pub const EXECUTABLE_FILE: &str = "opto-tracker";

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub directory: PathBuf,
    pub data: PathBuf,
    pub persistence: PathBuf,
    pub installed_executable: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self, String> {
        let directory = directories::BaseDirs::new()
            .map(|directories| directories.data_dir().join("OptoTracker"))
            .ok_or_else(|| {
                "Could not locate the current user's application-data folder".to_owned()
            })?;
        Ok(Self {
            data: directory.join(DATA_FILE),
            persistence: directory.join(PERSISTENCE_FILE),
            installed_executable: directory.join(EXECUTABLE_FILE),
            directory,
        })
    }
}
