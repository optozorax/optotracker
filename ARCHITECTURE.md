# Architecture

OptoTracker is designed for one local user and roughly ten years of data. At the
current rate that is only around 11,000 completed entries. Reports deliberately
scan the snapshot when requested; there are no aggregate caches or invalidation
rules to maintain.

## Boundaries

- `model.rs` is the time-tracking core. It owns data invariants, stable entry
  identity, and timer transitions. It has no UI, filesystem, or operating-system
  dependencies.
- `storage.rs` is the JSON adapter. It decodes supported format versions,
  validates snapshots, creates weekly backups, and atomically replaces the data
  file. Only a missing file creates a new store; malformed or inaccessible files
  stop startup.
- `local_time.rs` is the calendar policy. Timestamps are stored as Unix seconds,
  while displayed dates and day boundaries always use the computer's current
  local time.
- `reports.rs` contains read-only, full-scan calculations over a model snapshot.
  It does not cache results or mutate the store.
- `github_activity.rs` is a reusable egui calendar heatmap. The caller supplies
  cell fills, tooltip contents and the names on its axes, so it does not know
  about trackers, projects, activities, persistence, or languages.
- `date_picker.rs` is a reusable egui/chrono date-picker widget. It owns only
  transient popup state and edits a caller-owned local date. Its words come
  from the caller too.
- `pagination.rs` is reusable offset-based pagination. Its starting position
  may be any item rather than only a page-size boundary.
- `theme.rs` holds the colors of the interface as one palette for the light
  theme and one for the dark theme. egui decides which theme is shown, from
  the setting or from the system; code that paints with its own colors asks
  for the palette of the visuals it is drawn with. The reusable widgets do
  not use it and choose between their own light and dark colors.
- `i18n.rs` holds every text of the interface in English and in Russian: a
  table with one line per text, and methods for the texts that are built
  around a number, a name or a date. The language is chosen in Settings and
  kept for the whole process, so code that shows a text asks `tr()` for it.
  The model, the storage and the calendar policy stay free of languages: they
  report errors as values, and `i18n.rs` puts those into words.
- `paths.rs` resolves application paths once. `desktop.rs` with its
  `desktop/` folder, `tray.rs`, `single_instance.rs`, and the taskbar
  functions in `app_icon.rs` are the only places that contain desktop or
  operating-system integration.
- `main.rs` is the composition layer and egui UI. It turns UI interactions into
  model changes and persists them.

Presentation-only selections, such as the idle timer's activity/project and the
calendar display mode, live in egui's persisted memory. They remain separate
from the tracker data in `data.json` and are validated against it when restored.

## Data rules

Completed entries have stable `EntryId` values. Vector positions are only an
implementation detail for display order and never identify pending UI actions.
Version-1 files are decoded by assigning IDs in their existing order; the next
successful save writes version 2.

A completed entry may cross local midnight but cannot exceed 24 hours. A running
timer that reaches 24 hours is completed at that boundary. Project and activity
references must exist, names must not be blank, entry IDs must be unique, and
completed intervals cannot run backwards.

Saving validates the entire snapshot first. The UI retains the last saved
snapshot and restores it if persistence fails. Backup failure is reported as a
warning but does not block the primary atomic save.

## Windows, macOS and Linux

The model, storage, reports, local-calendar policy and the widgets are the
same on every system. What differs is behind the boundaries named above, and
each system does there what its own programs do.

Everything is kept in one folder, which `paths.rs` finds: the data, the UI
persistence and the installed copy of the program. What has to be elsewhere
for the system to find it, the start at sign-in and the shortcut, is written
only when the user asks for it and points back into the folder.

| | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Folder | `%APPDATA%\OptoTracker` | `~/Library/Application Support/OptoTracker` | `~/.local/share/OptoTracker` |
| Installed copy | `OptoTracker.exe` | `OptoTracker.app`, a bundle | `opto-tracker` |
| Start at sign-in | `Run` value in the registry | launch agent | desktop entry in the autostart folder |
| Shortcut | `.lnk` on the desktop | link on the desktop | desktop entry in the applications menu |
| Tray | notification area | menu bar | StatusNotifierItem over D-Bus |
| Running indicator | taskbar overlay | Dock badge | the icon of the window |
| Single instance | named mutex | file lock, and a socket to ask for the window | the same |
| Theme of the system | told to the window | told to the window | settings portal over D-Bus |

- `desktop.rs` holds what the systems share and one file per system in
  `desktop/`. The texts in `i18n.rs` that name a system, or that only one
  system says, are given once per system with `#[cfg]`.
- `tray.rs` is one implementation on the `tray-icon` crate. On Linux that
  crate speaks D-Bus by itself, so the executable needs no GTK. A desktop
  without a tray is not an error there: the window then closes for real.
- macOS expects an application to be a bundle. The executable writes its own
  (`--bundle`, and "Install" in Settings), with an icon that it draws, so
  that a release needs no packaging tools and no files beside the sources.
- A window of Wayland cannot leave the screen and come back, which sending
  it to the tray comes down to. On Linux the window is therefore opened with
  X11 where that is offered, and with Wayland only where it is not.
- The fonts are those of the system: a list of files on Windows and macOS,
  and whatever fontconfig names on Linux. The rows of the timer and of the
  entries measure the clock in the font that was found, since the digits of
  the systems differ in width.

Hiding and start-minimized behavior stay disabled unless a usable tray was
actually created. Unsupported desktop capabilities should return an explicit
error or remain disabled in the UI. They should not report success while doing
nothing.
