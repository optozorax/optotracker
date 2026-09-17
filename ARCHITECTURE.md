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
  cell fills and tooltip contents, so it does not know about trackers, projects,
  activities, or persistence.
- `date_picker.rs` is a reusable egui/chrono date-picker widget. It owns only
  transient popup state and edits a caller-owned local date.
- `pagination.rs` is reusable offset-based pagination. Its starting position
  may be any item rather than only a page-size boundary.
- `paths.rs` resolves application paths once. `desktop.rs`, `tray.rs`,
  `single_instance.rs`, and the taskbar functions in `app_icon.rs` are the only
  places that contain desktop or operating-system integration.
- `main.rs` is the composition layer and egui UI. It turns UI interactions into
  model changes and persists them.

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

## Adding Linux later

The model, storage, reports, local-calendar policy, and calendar widget should
need no changes. Linux work belongs at the existing boundaries:

1. Enable eframe's `x11` and/or `wayland` features and add a Linux build job.
2. Implement the non-Windows `TrayController` in `tray.rs`. Keep hiding and
   start-minimized behavior disabled unless a usable tray was actually created.
3. Implement the non-Windows guard in `single_instance.rs`, preferably by
   locking a separate file beside the data file.
4. Implement autorun and shortcut behavior in `desktop.rs`. Application paths
   already use the platform-aware `directories` crate, and opening the folder
   already uses the cross-platform `open` crate.
5. Decide whether a taskbar running indicator has a meaningful equivalent. It
   is optional decoration and must not affect timer behavior.

Unsupported desktop capabilities should return an explicit error or remain
disabled in the UI. They should not report success while doing nothing.
