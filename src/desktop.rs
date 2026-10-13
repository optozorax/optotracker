//! Desktop integration boundary.
//!
//! The UI calls this module using resolved paths. What every system does in
//! its own way lives in one file per system, and none of their APIs leak into
//! the application.

use std::fs;
use std::path::{Path, PathBuf};

use crate::i18n::tr;
use crate::paths::AppPaths;

#[cfg(target_os = "windows")]
#[path = "desktop/windows.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "desktop/macos.rs"]
mod platform;
// Linux and FreeBSD: `build.rs` gives the two one name.
#[cfg(freedesktop)]
#[path = "desktop/freedesktop.rs"]
mod platform;
#[cfg(not(any(target_os = "windows", target_os = "macos", freedesktop)))]
compile_error!("OptoTracker is written for Windows, macOS, Linux and FreeBSD");

pub use platform::{autorun_enabled, create_desktop_shortcut, set_autorun};

#[cfg(target_os = "macos")]
pub use platform::{bundle_into, on_reopen, system_fonts};
#[cfg(freedesktop)]
pub use platform::{
    event_loop_builder, system_fonts, system_theme, watch_system_theme, window_can_hide, APP_ID,
};
#[cfg(target_os = "windows")]
pub use platform::{keep_window_theme, system_fonts};

pub fn install_current_executable(paths: &AppPaths) -> Result<(), String> {
    let source = std::env::current_exe()
        .map_err(|error| format!("{}: {error}", tr().executable_not_found))?;
    fs::create_dir_all(&paths.directory)
        .map_err(|error| format!("{}: {error}", tr().folder_not_created))?;
    if !same_file(&source, &paths.installed_executable) {
        platform::install(&source, paths)
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

/// A font file of the system and the face to take from it.
pub struct SystemFont {
    pub path: PathBuf,
    /// Which of the faces, for a file that holds several.
    pub index: u32,
    /// The weight, for a file whose one face can be drawn in any weight.
    pub weight: Option<f32>,
}

impl SystemFont {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn file(path: &str) -> Self {
        Self {
            path: path.into(),
            index: 0,
            weight: None,
        }
    }
}

/// The fonts that the system draws its own interface with, the likeliest
/// first: the regular ones, and the ones for text that stands out.
pub struct SystemFonts {
    pub regular: Vec<SystemFont>,
    pub semibold: Vec<SystemFont>,
}

/// Only the title bar of Windows has to be told to keep a theme.
#[cfg(not(target_os = "windows"))]
pub fn keep_window_theme(_frame: &eframe::Frame, _keep: bool) {}

/// Only Linux and FreeBSD offer a choice between windowing systems.
#[cfg(not(freedesktop))]
pub fn event_loop_builder() -> Option<eframe::EventLoopBuilderHook> {
    None
}

/// Whether a window can be taken off the screen and brought back, which
/// sending it to the tray comes down to.
#[cfg(not(freedesktop))]
pub fn window_can_hide() -> bool {
    true
}

/// The theme of the system where the window does not learn it by itself,
/// which is the case on Linux and FreeBSD only.
#[cfg(not(freedesktop))]
pub fn system_theme() -> Option<eframe::egui::Theme> {
    None
}

#[cfg(not(freedesktop))]
pub fn watch_system_theme(_changed: impl Fn() + Send + 'static) {}

/// Only macOS has a Dock, whose icon opens an application that already runs.
#[cfg(not(target_os = "macos"))]
pub fn on_reopen(_show: impl Fn() + 'static) {}
