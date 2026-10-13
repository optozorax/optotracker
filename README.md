# OptoTracker

A small time tracker for one person. It runs on your own computer, needs no
account and no server, and keeps everything in a single JSON file that you own.

<p align="center">
  <img src="screenshots/time-entries.png" width="760"
       alt="The Time entries tab: a running timer, a calendar of the year and the list of entries">
</p>

OptoTracker is inspired by [Clockify](https://clockify.me): start a timer, say
what you are working on, and later see where the time went. It keeps that core
and leaves out everything a team needs.

- **One file that you own.** All of your tracked time is in `data.json`, a
  readable file on your disk. This is the
  [file over app](https://stephango.com/file-over-app) philosophy known from
  Obsidian: applications come and go, and the file should outlast them. You can
  read it, back it up, put it under version control or analyze it with a short
  script, with or without OptoTracker.
- **Local, for one user.** There are no users, workspaces or permissions, and
  the application never connects to the network.
- **Simple.** The whole model is projects, activities and time entries. There
  is one timer, and every change is saved the moment you make it.

The screenshots show generated demo data.

## Features

### Timer and entries

- Pick an activity and, if you want, a project, and press **START**. One timer
  runs at a time.
- Started the timer late? Type the real start time into the field next to the
  clock. The timer keeps running.
- Finished entries are grouped by day and week, with totals. The start time,
  stop time, date, activity and project of an entry are all edited in place.
- ▶ starts a new timer with the activity and project of an old entry.
- A forgotten timer stops by itself after 24 hours, which is also the longest
  an entry can be.

### Calendar

The calendar shows the year the way GitHub shows contributions. It can color
the days by tracked time, as in the picture above, or by project or activity,
where each day is divided in proportion to the time spent on it. Hover over a
day to see its numbers, click it to jump to its entries, and drag or page the
calendar back through earlier years.

By project:

![The calendar colored by project](screenshots/calendar-projects.png)

By activity:

![The calendar colored by activity](screenshots/calendar-activities.png)

### Projects and activities

An activity is the kind of work, such as programming or writing. A project is
what the work is for. Activities are shared between projects, so you can ask
both what a project consisted of and where all the writing went.

Each tab lists the total time, the number of entries, the active days and the
first and last use. A project row expands into its activities, and an activity
row into its projects. Below the table, every project is drawn as a bar divided
by activity. A finished project keeps its history and moves below the active
ones in the project selector.

<p align="center">
  <img src="screenshots/projects.png"
       alt="The Projects tab: the table of projects with one row expanded, and every project as a bar divided by activity">
</p>

### Statistics

For a week, a month, a year, all time or any range of days, the Statistics tab
shows the total and how it compares with the period before, the time per day,
week or month divided by activity or project, and your habits: the average
weekday, the hours of the day you work at, the typical session and your streaks.

<p align="center">
  <img src="screenshots/statistics.png" width="760"
       alt="The Statistics tab for one year: totals, time per week by activity, lists of activities and projects, and habits">
</p>

### Light and dark

OptoTracker is light or dark as the system is: it follows the app mode of
Windows, the appearance of macOS and the color scheme of a Linux or FreeBSD
desktop. To keep one theme whatever the system is set to, choose it in
Settings.

<p align="center">
  <img src="screenshots/time-entries-dark.png" width="760"
       alt="The Time entries tab in the dark theme">
</p>

### English and Russian

The interface is in English or in Russian. Choose the language in Settings; it
changes at once.

### On your desktop

OptoTracker runs on Windows, macOS, Linux and FreeBSD, and on each of them it
behaves the way programs there do.

- Closing the window keeps OptoTracker running: in the tray on Windows, Linux
  and FreeBSD, where minimizing sends it there too, and in the menu bar on
  macOS. This can be turned off in Settings.
- The icons show whether a timer is running: those of the window, the taskbar
  and the tray, and on macOS a badge in the Dock.
- It can start when you sign in, without showing its window.
- Only one copy runs at a time. Everywhere but on Windows, starting it again
  brings the window of the running copy forward.

## Your data

Everything OptoTracker keeps is in one folder:

| System         | Application folder                          |
| -------------- | ------------------------------------------- |
| Windows        | `%APPDATA%\OptoTracker`                     |
| macOS          | `~/Library/Application Support/OptoTracker` |
| Linux, FreeBSD | `~/.local/share/OptoTracker`                |

Everything you track is in `data.json` there:

```json
{
  "format_version": 2,
  "projects": [
    {
      "name": "Clockwork Café",
      "color": { "red": 181, "green": 101, "blue": 29 },
      "status": "Active"
    }
  ],
  "activities": [
    { "name": "UI design", "color": { "red": 240, "green": 173, "blue": 127 } },
    { "name": "Programming", "color": { "red": 42, "green": 120, "blue": 214 } }
  ],
  "time_entries": [
    {
      "id": 1,
      "activity": 0,
      "project": 0,
      "time": { "started_at": 1791542700, "stopped_at": 1791548400 }
    }
  ],
  "active_entry": { "activity": 1, "project": 0, "started_at": 1791549660 }
}
```

- `activity` and `project` are positions in the two lists, and `project` may be
  `null`. For that reason projects and activities are never deleted or
  reordered; a project that is over is marked `Finished` instead.
- Times are Unix seconds in UTC. They are shown in the local time of the
  computer, and an entry that crosses midnight counts towards both days.
- `active_entry` is the running timer, or `null` when nothing is running.

OptoTracker is careful with this file:

- Every change is checked and written immediately, and the file is replaced in
  one step, so a crash cannot leave half of it behind.
- Once a week the file is copied to `backups/data-2026-W41.json` (the year and
  week number) next to it before it is changed.
- If the file cannot be read or is not valid, OptoTracker reports the problem
  and exits. It never starts empty over existing data.

To keep the file somewhere else, pass its path:

```powershell
OptoTracker.exe D:\Documents\time.json
```

The file is used where it is and nothing is copied. OptoTracker expects to be
the only program that changes the file while it is running.

The other files in the folder can all be deleted at any time. `temp.ron`
remembers the window size, your settings and your last selections. Everywhere
but on Windows, `instance.lock` and `instance.sock` are how a second copy
finds the one that runs, and on Linux and FreeBSD `icon.png` is the icon for
the applications menu.

## Installing

Download the file for your system from the
[latest release](../../releases/latest). It is the whole program and needs
nothing else. None of the files is code-signed, so Windows and macOS ask
before they run it for the first time.

**Windows.** Run `OptoTracker.exe`. Windows may warn about it the first time.

**macOS.** Unpack `OptoTracker-macos.zip` and open OptoTracker. It runs on
macOS 11 and later, on Apple and Intel processors. The first time, macOS
refuses to open it; allow it with **Open Anyway** under **Privacy & Security**
in System Settings.

**Linux.** Unpack `OptoTracker-linux-x86_64.tar.gz` and run `opto-tracker`. It
needs glibc 2.28 or later, which every distribution since 2019 has, and
nothing else beyond a desktop. The tray icon appears where the desktop shows
such icons: on KDE, Xfce, Cinnamon and others by themselves, on GNOME with
the AppIndicator extension, which Ubuntu comes with.

**FreeBSD.** Unpack `OptoTracker-freebsd-x86_64.tar.gz` and run
`opto-tracker`. It is built on FreeBSD 14 and needs nothing beyond a desktop
either. There it is the same program as on Linux, tray icon included.

To keep it, open **Settings** and press **Install OptoTracker here**. This
copies the program into the application folder, next to the data. The same
tab creates a shortcut and turns on starting when you sign in:

| System         | Installed as      | Shortcut                 | Start at sign-in                           |
| -------------- | ----------------- | ------------------------ | ------------------------------------------ |
| Windows        | `OptoTracker.exe` | on the desktop           | a `Run` value in the registry              |
| macOS          | `OptoTracker.app` | on the desktop           | a launch agent in `~/Library/LaunchAgents` |
| Linux, FreeBSD | `opto-tracker`    | in the applications menu | an entry in `~/.config/autostart`          |

The shortcut and the start at sign-in are the only things that OptoTracker
writes outside its folder, and only when you ask for them.

On Linux and FreeBSD a window can only be sent to the tray under X11, so
OptoTracker uses X11 where there is one, which on a Wayland desktop means
XWayland. Started with an empty `DISPLAY`, it uses Wayland itself and closes
with its window.

To build it yourself you need Rust 1.95 or newer:

```powershell
cargo run --release
```

On macOS this runs the bare executable. `opto-tracker --bundle FOLDER` writes
it into the folder as `OptoTracker.app`.

[ARCHITECTURE.md](ARCHITECTURE.md) describes the modules, the rules for the
data and where each system is dealt with.

## What it leaves out

There are no teams, clients, billing, tags or estimates, no sync, and no mobile
or web version. There is no import or export either: the data file is the
export.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
