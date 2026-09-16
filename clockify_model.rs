//! Minimal persistent model for a single-user time tracker.
//!
//! Intentionally omitted from Clockify's model: users, workspaces, clients,
//! memberships, billing, approval flows, estimates, kiosks, tags and custom
//! fields. Activities are normalized into reusable records with stable IDs.

use serde::{Deserialize, Serialize};

/// Seconds since the Unix epoch, always stored as UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixSeconds(pub i64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectId(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActivityId(pub usize);

/// A completed measurement. Running timers are stored separately in
/// `Store::active_entry`, so historical entries cannot be started again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    pub started_at: UnixSeconds,
    pub stopped_at: UnixSeconds,
}

impl TimeRange {
    pub fn duration_seconds(self) -> Option<u64> {
        self.stopped_at
            .0
            .checked_sub(self.started_at.0)
            .and_then(|value| u64::try_from(value).ok())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectStatus {
    #[default]
    Active,
    Finished,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RgbColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub color: RgbColor,
    pub status: ProjectStatus,
}

/// Activities are independent from projects and may be reused by any project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeEntry {
    pub activity: ActivityId,
    pub project: Option<ProjectId>,
    pub time: TimeRange,
}

/// The only entry that has a start time without a stop time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveTimeEntry {
    pub activity: ActivityId,
    pub project: Option<ProjectId>,
    pub started_at: UnixSeconds,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    pub format_version: u32,

    /// A project's position is its `ProjectId`. Never reorder or remove items;
    /// mark completed projects as `ProjectStatus::Finished` instead.
    pub projects: Vec<Project>,

    /// An activity's position is its `ActivityId`. Keep this append-only too.
    pub activities: Vec<Activity>,

    pub time_entries: Vec<TimeEntry>,

    /// `Option` enforces that at most one timer can be active.
    pub active_entry: Option<ActiveTimeEntry>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            format_version: 1,
            projects: Vec::new(),
            activities: Vec::new(),
            time_entries: Vec::new(),
            active_entry: None,
        }
    }
}
