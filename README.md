# Personal Time Tracker

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

The current version can start and stop the single active timer, change the
activity or project of a running or completed entry, and delete completed
entries. Every change is saved immediately.
