# OptoTracker

A minimal native time-tracking app built with Rust, eframe and egui.

The current application targets Windows and requires Rust 1.95 or newer.

Run it from this directory:

```powershell
cargo run --release
```

By default it reads and writes `%APPDATA%\OptoTracker\data.json`. To use another
store directly, pass its path as the first argument. The file is not copied or
migrated:

```powershell
cargo run --release -- path\to\store.json
```

The current version can start and stop the single active timer, restart an old
entry's project/activity combination, create projects and activities, change
the activity or project of a running or completed entry, edit each `HH:MM`
value directly in an always-visible text field, and delete completed entries.
Hidden seconds are preserved when editing minutes. A stop time earlier than its
start is interpreted as the following day. Timers and edited entries are limited
to 24 hours; a forgotten running timer stops at that limit. Day and week totals
split overnight entries at the computer's local midnight. The full-width
Projects and Activities views support renaming and show usage statistics;
projects also support color and status editing. Every valid change is saved
immediately, and a failed save reverts the in-memory change.

See [ARCHITECTURE.md](ARCHITECTURE.md) for module boundaries, persistence rules,
and the intended path to future Linux support.
