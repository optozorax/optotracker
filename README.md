# OptoTracker

A minimal native time-tracking app built with Rust, eframe and egui.

Run it from this directory:

```powershell
cargo run --release
```

By default it reads and writes `data/time_tracker_store.json`. To use another
store, pass its path as the first argument:

```powershell
cargo run --release -- path\to\store.json
```

The current version can start and stop the single active timer, restart an old
entry's project/activity combination, create projects and activities, change
the activity or project of a running or completed entry, edit each `HH:MM`
value directly in an always-visible text field, and delete completed entries.
Hidden seconds are preserved when editing minutes. A stop time earlier than its
start is interpreted as the following day. Day and week totals split overnight
entries at local midnight. The full-width Projects and Activities views support
renaming and show usage statistics; projects also support color and status
editing. Every change is saved immediately.
