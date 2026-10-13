//! macOS, where an application is a bundle: a folder that Finder shows as
//! one file, with the executable, its icon and a description inside.
//!
//! The installed copy is such a bundle in the application folder. What
//! Windows keeps in its registry is a launch agent here, a description of
//! what to start that is read when the user signs in.

use super::{install_current_executable, tr, AppPaths, SystemFont, SystemFonts};
use crate::app_icon;
use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{define_class, msg_send, sel, DefinedClass as _, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const BUNDLE: &str = "OptoTracker.app";
const EXECUTABLE: &str = "OptoTracker";
const ICON: &str = "OptoTracker.icns";
/// Names the application to macOS, and its launch agent.
const IDENTIFIER: &str = "io.github.optozorax.OptoTracker";

fn launch_agent() -> Option<PathBuf> {
    let directories = directories::BaseDirs::new()?;
    let agents = directories.home_dir().join("Library/LaunchAgents");
    Some(agents.join(format!("{IDENTIFIER}.plist")))
}

pub fn autorun_enabled() -> bool {
    launch_agent().is_some_and(|agent| agent.is_file())
}

/// The agent is only written or removed. macOS reads it at the next sign-in;
/// loading it now would start a second copy at once, and unloading it would
/// stop the copy that it has started.
pub fn set_autorun(paths: &AppPaths, enabled: bool, start_minimized: bool) -> Result<(), String> {
    let failed = |error: io::Error| format!("{}: {error}", tr().autorun_not_updated);
    let agent = launch_agent().ok_or_else(|| tr().autorun_not_updated.to_owned())?;
    if enabled {
        if let Some(agents) = agent.parent() {
            fs::create_dir_all(agents).map_err(failed)?;
        }
        let description = launch_agent_plist(&paths.installed_executable, start_minimized);
        fs::write(&agent, description).map_err(failed)
    } else {
        match fs::remove_file(&agent) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(failed(error)),
            _ => Ok(()),
        }
    }
}

fn launch_agent_plist(executable: &Path, start_minimized: bool) -> String {
    let mut arguments = format!("        <string>{}</string>\n", xml_text(executable));
    if start_minimized {
        arguments.push_str("        <string>--minimized</string>\n");
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{IDENTIFIER}</string>
    <key>ProgramArguments</key>
    <array>
{arguments}    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#
    )
}

fn xml_text(path: &Path) -> String {
    path.to_string_lossy()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The shortcut is a link on the desktop, which Finder shows with the icon
/// of the application and opens like the application itself.
pub fn create_desktop_shortcut(paths: &AppPaths) -> Result<(), String> {
    install_current_executable(paths)?;
    let directories =
        directories::UserDirs::new().ok_or_else(|| tr().desktop_not_found.to_owned())?;
    let desktop = directories
        .desktop_dir()
        .ok_or_else(|| tr().desktop_not_found.to_owned())?;
    let shortcut = desktop.join("OptoTracker");
    // A link from before is replaced. Anything else by that name is not
    // ours, and creating the link fails over it.
    if fs::symlink_metadata(&shortcut).is_ok_and(|existing| existing.is_symlink()) {
        let _ = fs::remove_file(&shortcut);
    }
    std::os::unix::fs::symlink(paths.directory.join(BUNDLE), &shortcut)
        .map_err(|error| format!("{}: {error}", tr().shortcut_not_created))
}

/// Puts a copy of the running application into the application folder, as a
/// bundle. The copy is completed next to an older one before it takes its
/// place.
pub fn install(source: &Path, paths: &AppPaths) -> io::Result<()> {
    let bundle = paths.directory.join(BUNDLE);
    let fresh = paths.directory.join(format!("{BUNDLE}.new"));
    remove_bundle(&fresh)?;
    match running_bundle(source) {
        // The whole bundle, so that everything its signature covers stays
        // the way it was signed.
        Some(running) => copy_folder(running, &fresh)?,
        None => write_bundle(source, &fresh)?,
    }
    remove_bundle(&bundle)?;
    fs::rename(&fresh, &bundle)
}

/// Writes the running executable as a bundle into `folder`. This is how a
/// release is packaged, and how a build of one's own becomes an application.
pub fn bundle_into(folder: &Path) -> Result<PathBuf, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("{}: {error}", tr().executable_not_found))?;
    let bundle = folder.join(BUNDLE);
    remove_bundle(&bundle)
        .and_then(|()| write_bundle(&executable, &bundle))
        .map_err(|error| format!("{}: {error}", tr().install_failed))?;
    Ok(bundle)
}

/// The bundle that `executable` is the executable of, if it was made here.
fn running_bundle(executable: &Path) -> Option<&Path> {
    let folder = executable.parent()?;
    let contents = folder.parent()?;
    let bundle = contents.parent()?;
    let is_bundle = executable.file_name()? == EXECUTABLE
        && folder.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app"
        && contents.join("Info.plist").is_file();
    is_bundle.then_some(bundle)
}

fn remove_bundle(bundle: &Path) -> io::Result<()> {
    match fs::remove_dir_all(bundle) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn write_bundle(executable: &Path, bundle: &Path) -> io::Result<()> {
    let contents = bundle.join("Contents");
    fs::create_dir_all(contents.join("MacOS"))?;
    fs::create_dir_all(contents.join("Resources"))?;
    copy_file(executable, &contents.join("MacOS").join(EXECUTABLE))?;
    fs::write(contents.join("Resources").join(ICON), icns()?)?;
    fs::write(contents.join("PkgInfo"), "APPL????")?;
    fs::write(contents.join("Info.plist"), info_plist())
}

fn copy_folder(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else if kind.is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(entry.path())?, &target)?;
        } else {
            copy_file(&entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Copies what a file holds and whether it may be executed, and nothing
/// more. `fs::copy` would also carry over the mark that macOS puts on what
/// was downloaded, and the copy would be questioned like a new download.
fn copy_file(from: &Path, to: &Path) -> io::Result<()> {
    let mut source = fs::File::open(from)?;
    let mut target = fs::File::create(to)?;
    io::copy(&mut source, &mut target)?;
    target.set_permissions(source.metadata()?.permissions())
}

fn info_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>OptoTracker</string>
    <key>CFBundleDisplayName</key>
    <string>OptoTracker</string>
    <key>CFBundleIdentifier</key>
    <string>{IDENTIFIER}</string>
    <key>CFBundleExecutable</key>
    <string>{EXECUTABLE}</string>
    <key>CFBundleIconFile</key>
    <string>{ICON}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.productivity</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
"#,
        version = env!("CARGO_PKG_VERSION"),
    )
}

/// The icon of the bundle: a PNG of every size that macOS asks for, each
/// under the names that macOS knows the size by.
fn icns() -> io::Result<Vec<u8>> {
    // The second name of a size is for a screen that shows two pixels per
    // point, where it stands for the icon of half that size.
    const PICTURES: [(u32, &[&[u8; 4]]); 6] = [
        (32, &[b"ic11"]),
        (64, &[b"ic12"]),
        (128, &[b"ic07"]),
        (256, &[b"ic08", b"ic13"]),
        (512, &[b"ic09", b"ic14"]),
        (1024, &[b"ic10"]),
    ];

    let mut body = Vec::new();
    for (size, names) in PICTURES {
        let picture = app_icon::png(&app_icon::application_icon(size)).map_err(io::Error::other)?;
        for name in names {
            body.extend_from_slice(*name);
            body.extend_from_slice(&(picture.len() as u32 + 8).to_be_bytes());
            body.extend_from_slice(&picture);
        }
    }

    let mut file = Vec::with_capacity(body.len() + 8);
    file.extend_from_slice(b"icns");
    file.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
    file.extend_from_slice(&body);
    Ok(file)
}

/// San Francisco, the font of macOS itself, and the fonts that every Mac
/// has in case it cannot be read.
pub fn system_fonts() -> SystemFonts {
    // One file holds San Francisco in every weight.
    let san_francisco = |weight| SystemFont {
        weight: Some(weight),
        ..SystemFont::file("/System/Library/Fonts/SFNS.ttf")
    };
    let helvetica = |index| SystemFont {
        index,
        ..SystemFont::file("/System/Library/Fonts/Helvetica.ttc")
    };
    SystemFonts {
        regular: vec![
            san_francisco(400.0),
            SystemFont::file("/System/Library/Fonts/Supplemental/Arial.ttf"),
            helvetica(0),
        ],
        semibold: vec![
            san_francisco(600.0),
            SystemFont::file("/System/Library/Fonts/Supplemental/Arial Bold.ttf"),
            helvetica(1),
        ],
    }
}

/// The class and the name of the event that macOS sends to an application
/// that is opened while it already runs.
const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
const REOPEN_APPLICATION: u32 = u32::from_be_bytes(*b"rapp");

/// Calls `show` whenever the application is opened while it already runs:
/// by a click on its icon in the Dock, or by opening it once more in Finder.
/// A window that was sent to the menu bar would otherwise stay away.
pub fn on_reopen(show: impl Fn() + 'static) {
    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    let handler = ReopenHandler::new(main_thread, Box::new(show));
    let manager = NSAppleEventManager::sharedAppleEventManager();
    // SAFETY: the handler takes the two descriptors that an Apple event is
    // delivered with, and is kept for as long as the process runs.
    unsafe {
        let _: () = msg_send![
            &*manager,
            setEventHandler: &*handler,
            andSelector: sel!(handleReopen:withReplyEvent:),
            forEventClass: CORE_EVENT_CLASS,
            andEventID: REOPEN_APPLICATION,
        ];
    }
    // The manager does not keep its handlers alive.
    std::mem::forget(handler);
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "OptoTrackerReopenHandler"]
    #[thread_kind = MainThreadOnly]
    #[ivars = Box<dyn Fn()>]
    struct ReopenHandler;

    impl ReopenHandler {
        #[unsafe(method(handleReopen:withReplyEvent:))]
        fn handle_reopen(&self, _event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            (self.ivars())();
        }
    }
);

impl ReopenHandler {
    fn new(main_thread: MainThreadMarker, show: Box<dyn Fn()>) -> Retained<Self> {
        let this = Self::alloc(main_thread).set_ivars(show);
        // SAFETY: `init` of `NSObject` takes nothing and cannot fail.
        unsafe { msg_send![super(this), init] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("opto-tracker-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn the_icon_holds_a_picture_for_every_size() {
        let icon = icns().unwrap();
        assert_eq!(&icon[..4], b"icns");
        assert_eq!(
            u32::from_be_bytes(icon[4..8].try_into().unwrap()) as usize,
            icon.len()
        );

        let mut kinds = Vec::new();
        let mut offset = 8;
        while offset < icon.len() {
            let length = u32::from_be_bytes(icon[offset + 4..offset + 8].try_into().unwrap());
            // Every picture is a PNG.
            assert_eq!(&icon[offset + 8..offset + 12], b"\x89PNG");
            kinds.push(String::from_utf8_lossy(&icon[offset..offset + 4]).into_owned());
            offset += length as usize;
        }
        assert_eq!(offset, icon.len());
        assert_eq!(
            kinds,
            ["ic11", "ic12", "ic07", "ic08", "ic13", "ic09", "ic14", "ic10"]
        );
    }

    #[test]
    fn an_executable_is_installed_as_a_bundle() {
        let folder = temporary_folder("bundle");
        let executable = folder.join("built");
        fs::write(&executable, "the executable").unwrap();
        let paths = AppPaths::in_directory(folder.join("OptoTracker"));
        fs::create_dir_all(&paths.directory).unwrap();

        install(&executable, &paths).unwrap();
        assert_eq!(
            fs::read(&paths.installed_executable).unwrap(),
            b"the executable"
        );
        let contents = paths.directory.join(BUNDLE).join("Contents");
        let description = fs::read_to_string(contents.join("Info.plist")).unwrap();
        assert!(description.contains("<string>io.github.optozorax.OptoTracker</string>"));
        assert!(description.contains(concat!("<string>", env!("CARGO_PKG_VERSION"), "</string>")));
        assert!(contents.join("Resources/OptoTracker.icns").is_file());

        // From a bundle, everything in it is copied: also what a signature
        // has added.
        fs::create_dir_all(contents.join("_CodeSignature")).unwrap();
        fs::write(contents.join("_CodeSignature/CodeResources"), "sealed").unwrap();
        let installed = paths.installed_executable.clone();
        let copy = AppPaths::in_directory(folder.join("Copy"));
        fs::create_dir_all(&copy.directory).unwrap();
        install(&installed, &copy).unwrap();
        let copied = copy.directory.join(BUNDLE).join("Contents");
        assert_eq!(
            fs::read(&copy.installed_executable).unwrap(),
            b"the executable"
        );
        assert_eq!(
            fs::read(copied.join("_CodeSignature/CodeResources")).unwrap(),
            b"sealed"
        );
        assert!(!copy.directory.join("OptoTracker.app.new").exists());

        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_launch_agent_starts_the_installed_executable() {
        let executable = Path::new("/Users/me/Library/R&D <1>/OptoTracker");
        let agent = launch_agent_plist(executable, true);
        assert!(agent.contains("<string>/Users/me/Library/R&amp;D &lt;1&gt;/OptoTracker</string>"));
        assert!(agent.contains("<string>--minimized</string>"));
        assert!(!launch_agent_plist(executable, false).contains("--minimized"));
    }
}
