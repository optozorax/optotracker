//! Linux, by the freedesktop specifications that its desktops share.
//!
//! What Windows keeps in its registry is a desktop entry here: one in the
//! autostart folder to start with the session, and one among the applications
//! to appear in the menu. Both point into the application folder, where the
//! executable and its icon are.

use super::{install_current_executable, tr, AppPaths, SystemFont, SystemFonts};
use crate::app_icon;
use eframe::egui;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use zbus::zvariant::{OwnedValue, Value};

/// What the window calls itself, which is how a desktop finds the entry of
/// the menu, and its icon, for a window that is open.
pub const APP_ID: &str = "opto-tracker";
const DESKTOP_ENTRY: &str = "opto-tracker.desktop";
const ICON_FILE: &str = "icon.png";
const ICON_SIZE: u32 = 256;

fn autostart_entry() -> Option<PathBuf> {
    let directories = directories::BaseDirs::new()?;
    Some(
        directories
            .config_dir()
            .join("autostart")
            .join(DESKTOP_ENTRY),
    )
}

fn menu_entry() -> Option<PathBuf> {
    let directories = directories::BaseDirs::new()?;
    Some(
        directories
            .data_dir()
            .join("applications")
            .join(DESKTOP_ENTRY),
    )
}

pub fn autorun_enabled() -> bool {
    // The settings of some desktops switch an entry off by marking it
    // instead of deleting it.
    autostart_entry()
        .and_then(|entry| fs::read_to_string(entry).ok())
        .is_some_and(|entry| {
            !entry.lines().any(|line| {
                matches!(
                    line.trim(),
                    "Hidden=true" | "X-GNOME-Autostart-enabled=false"
                )
            })
        })
}

pub fn set_autorun(paths: &AppPaths, enabled: bool, start_minimized: bool) -> Result<(), String> {
    let failed = |error: io::Error| format!("{}: {error}", tr().autorun_not_updated);
    let entry = autostart_entry().ok_or_else(|| tr().autorun_not_updated.to_owned())?;
    if enabled {
        let arguments = if start_minimized { " --minimized" } else { "" };
        write_entry(&entry, &desktop_entry(paths, arguments)).map_err(failed)
    } else {
        match fs::remove_file(&entry) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(failed(error)),
            _ => Ok(()),
        }
    }
}

/// A desktop of Linux shows its applications in a menu rather than as files
/// on the desktop, so that is where the shortcut goes.
pub fn create_desktop_shortcut(paths: &AppPaths) -> Result<(), String> {
    install_current_executable(paths)?;
    let failed = |error: io::Error| format!("{}: {error}", tr().shortcut_not_created);
    let entry = menu_entry().ok_or_else(|| tr().shortcut_not_created.to_owned())?;
    let icon = app_icon::png(&app_icon::application_icon(ICON_SIZE))
        .map_err(|error| format!("{}: {error}", tr().shortcut_not_created))?;
    fs::write(paths.directory.join(ICON_FILE), icon).map_err(failed)?;
    write_entry(&entry, &desktop_entry(paths, "")).map_err(failed)
}

/// Puts a copy of the running executable into the application folder. The
/// copy takes the place of an older one in a single step, which also works
/// while the older one is still running.
pub fn install(source: &Path, paths: &AppPaths) -> io::Result<()> {
    let fresh = paths.installed_executable.with_extension("new");
    fs::copy(source, &fresh)?;
    fs::rename(&fresh, &paths.installed_executable)
}

fn write_entry(file: &Path, entry: &str) -> io::Result<()> {
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder)?;
    }
    fs::write(file, entry)
}

/// The entry that starts the installed executable with `arguments`.
fn desktop_entry(paths: &AppPaths, arguments: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=OptoTracker\n\
         Comment=Personal time tracker\n\
         Comment[ru]=Личный учёт времени\n\
         Exec={}{arguments}\n\
         Icon={}\n\
         Terminal=false\n\
         Categories=Office;ProjectManagement;\n\
         StartupWMClass={APP_ID}\n",
        exec_argument(&paths.installed_executable),
        string_value(&paths.directory.join(ICON_FILE)),
    )
}

/// A path as the value of a key of a desktop entry.
fn string_value(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
}

/// A path as one argument of the `Exec` key: between quotes, with a backslash
/// before the characters that mean something between them, and then escaped
/// once more like any other value.
fn exec_argument(path: &Path) -> String {
    let mut quoted = String::from('"');
    for character in path.to_string_lossy().chars() {
        match character {
            '"' | '`' | '$' | '\\' => quoted.push('\\'),
            // Starts a field code such as `%f` otherwise.
            '%' => quoted.push('%'),
            _ => {}
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted.replace('\\', "\\\\").replace('\n', "\\n")
}

static USES_X11: AtomicBool = AtomicBool::new(false);

/// Picks X11 wherever it is offered, which under Wayland means XWayland.
///
/// Wayland gives a window no way to leave the screen and come back, so it
/// could not be sent to the tray there. Started with an empty `DISPLAY`, the
/// application uses Wayland itself and does without the tray.
pub fn event_loop_builder() -> Option<eframe::EventLoopBuilderHook> {
    use winit::platform::x11::EventLoopBuilderExtX11 as _;

    std::env::var_os("DISPLAY").filter(|display| !display.is_empty())?;
    if !x11_is_installed() {
        return None;
    }
    USES_X11.store(true, Ordering::Relaxed);
    Some(Box::new(|builder| {
        builder.with_x11();
    }))
}

/// Whether the libraries are there that winit opens a window of X11 with.
/// It stops the application over a missing one instead of turning to
/// Wayland, so they are tried here first.
fn x11_is_installed() -> bool {
    [
        "libX11.so.6",
        "libX11-xcb.so.1",
        "libXcursor.so.1",
        "libXi.so.6",
        "libxkbcommon-x11.so.0",
    ]
    .into_iter()
    // SAFETY: these are the libraries of the system that winit loads itself
    // right after, and loading them runs nothing of ours.
    .all(|library| unsafe { libloading::Library::new(library) }.is_ok())
}

pub fn window_can_hide() -> bool {
    USES_X11.load(Ordering::Relaxed)
}

/// The font that the desktop is set to use for text without serifs, as
/// fontconfig finds it. Without fontconfig the fonts of egui remain.
pub fn system_fonts() -> SystemFonts {
    SystemFonts {
        regular: matching_font("sans-serif:weight=regular", 400.0),
        semibold: matching_font("sans-serif:weight=demibold", 600.0),
    }
}

fn matching_font(pattern: &str, weight: f32) -> Vec<SystemFont> {
    let found = || {
        let output = Command::new("fc-match")
            .args(["--format", "%{file}\n%{index}", pattern])
            .output()
            .ok()?;
        let output = String::from_utf8(output.stdout).ok()?;
        let (file, index) = output.split_once('\n')?;
        let path = PathBuf::from(file);
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        // The formats that hold outlines. A desktop may still have a font
        // of pictures as its first choice.
        matches!(extension.as_str(), "ttf" | "otf" | "ttc" | "otc").then_some(SystemFont {
            path,
            // Above the number of the face, fontconfig keeps which of the
            // named weights of a variable font it has picked.
            index: index.trim().parse::<u32>().ok()? & 0xffff,
            weight: Some(weight),
        })
    };
    found().into_iter().collect()
}

const SCHEME_UNKNOWN: u8 = 0;
const SCHEME_LIGHT: u8 = 1;
const SCHEME_DARK: u8 = 2;

static COLOR_SCHEME: AtomicU8 = AtomicU8::new(SCHEME_UNKNOWN);

/// The color scheme that the desktop is set to, once
/// [`watch_system_theme`] has learned it.
pub fn system_theme() -> Option<egui::Theme> {
    match COLOR_SCHEME.load(Ordering::Relaxed) {
        SCHEME_LIGHT => Some(egui::Theme::Light),
        SCHEME_DARK => Some(egui::Theme::Dark),
        _ => None,
    }
}

/// Follows the color scheme of the desktop and calls `changed` whenever it
/// becomes another one.
///
/// A window of Linux is not told the scheme. Desktops publish it through the
/// settings portal, which is asked here on a thread of its own, since a
/// desktop without a portal may take long to say so. The first answer is
/// waited for briefly, so that the window does not open in the wrong theme.
pub fn watch_system_theme(changed: impl Fn() + Send + 'static) {
    let (first_answer, answered) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = follow_color_scheme(|| {
            // Nobody waits for the answers after the first.
            if first_answer.send(()).is_err() {
                changed();
            }
        });
    });
    let _ = answered.recv_timeout(Duration::from_millis(200));
}

fn follow_color_scheme(changed: impl Fn()) -> zbus::Result<()> {
    const NAMESPACE: &str = "org.freedesktop.appearance";
    const KEY: &str = "color-scheme";

    let connection = zbus::blocking::Connection::session()?;
    let settings = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
    )?;
    // Listened to before the scheme is asked for, so that no change is missed.
    let changes = settings.receive_signal("SettingChanged")?;
    let scheme: OwnedValue = settings.call("Read", &(NAMESPACE, KEY))?;
    remember_color_scheme(&scheme);
    changed();
    for change in changes {
        let body = change.body();
        let Ok((namespace, key, scheme)) = body.deserialize::<(&str, &str, Value<'_>)>() else {
            continue;
        };
        if namespace == NAMESPACE && key == KEY {
            remember_color_scheme(&scheme);
            changed();
        }
    }
    Ok(())
}

fn remember_color_scheme(scheme: &Value<'_>) {
    COLOR_SCHEME.store(color_scheme(scheme), Ordering::Relaxed);
}

/// What the portal means by a value of `color-scheme`: 1 prefers dark, 2
/// prefers light, and 0 has no preference, which is how a desktop that only
/// knows "dark" and "not dark" says light.
fn color_scheme(scheme: &Value<'_>) -> u8 {
    match scheme {
        // The older of the two ways to ask wraps the answer once more.
        Value::Value(inner) => color_scheme(inner),
        Value::U32(1) => SCHEME_DARK,
        Value::U32(_) => SCHEME_LIGHT,
        _ => SCHEME_UNKNOWN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_quoted_for_the_exec_key() {
        assert_eq!(
            exec_argument(Path::new("/home/me/.local/share/OptoTracker/opto-tracker")),
            r#""/home/me/.local/share/OptoTracker/opto-tracker""#
        );
        assert_eq!(
            exec_argument(Path::new("/home/my name/50% $HOME/a\"b`c\\d")),
            r#""/home/my name/50%% \\$HOME/a\\"b\\`c\\\\d""#
        );
    }

    #[test]
    fn the_entry_starts_the_installed_executable() {
        let paths = AppPaths::in_directory("/data/OptoTracker".into());
        let entry = desktop_entry(&paths, " --minimized");
        assert!(entry.starts_with("[Desktop Entry]\nType=Application\nName=OptoTracker\n"));
        assert!(entry.contains("\nExec=\"/data/OptoTracker/opto-tracker\" --minimized\n"));
        assert!(entry.contains("\nIcon=/data/OptoTracker/icon.png\n"));
        assert!(entry.contains("\nStartupWMClass=opto-tracker\n"));
    }

    #[test]
    fn the_color_scheme_is_read_from_either_answer_of_the_portal() {
        assert_eq!(color_scheme(&Value::U32(1)), SCHEME_DARK);
        assert_eq!(color_scheme(&Value::U32(2)), SCHEME_LIGHT);
        assert_eq!(color_scheme(&Value::U32(0)), SCHEME_LIGHT);
        let wrapped = Value::Value(Box::new(Value::U32(1)));
        assert_eq!(color_scheme(&wrapped), SCHEME_DARK);
        assert_eq!(color_scheme(&Value::from("dark")), SCHEME_UNKNOWN);
    }
}
