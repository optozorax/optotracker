//! Minimal persistent model for a single-user time tracker.
//!
//! Intentionally omitted from Clockify's model: users, workspaces, clients,
//! memberships, billing, approval flows, estimates, kiosks, tags and custom
//! fields. Activities are normalized into reusable records with stable IDs.

use serde::{Deserialize, Serialize};

pub const CURRENT_FORMAT_VERSION: u32 = 2;
pub const MAX_ENTRY_SECONDS: i64 = 24 * 60 * 60;

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

/// Stable identity for a completed entry. Unlike a vector position, this does
/// not change when entries are sorted or another entry is deleted.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct EntryId(pub u64);

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
    /// Version 1 stores did not have entry IDs. The storage adapter assigns
    /// them while decoding that version.
    #[serde(default)]
    pub id: EntryId,
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
            format_version: CURRENT_FORMAT_VERSION,
            projects: Vec::new(),
            activities: Vec::new(),
            time_entries: Vec::new(),
            active_entry: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StopOutcome {
    pub entry_id: EntryId,
    pub capped_at_limit: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ModelError {
    #[error("unsupported data format version {0}")]
    UnsupportedFormat(u32),
    #[error("entry ID {0} is duplicated or reserved")]
    InvalidEntryId(u64),
    #[error("project and activity names cannot be empty")]
    EmptyName,
    #[error("activity {0} does not exist")]
    MissingActivity(usize),
    #[error("project {0} does not exist")]
    MissingProject(usize),
    #[error("an entry stops before it starts")]
    NegativeDuration,
    #[error("a time entry cannot be longer than 24 hours")]
    EntryTooLong,
    #[error("a timer is already running")]
    TimerAlreadyRunning,
    #[error("no timer is running")]
    NoActiveTimer,
    #[error("the system clock is earlier than the timer start")]
    ClockMovedBackwards,
    #[error("the timer start cannot be in the future")]
    TimerStartInFuture,
    #[error("the timer start must be less than 24 hours ago")]
    TimerStartTooOld,
    #[error("the entry ID space is exhausted")]
    EntryIdExhausted,
    #[error("a timestamp is outside the supported range")]
    TimestampOutOfRange,
}

impl Store {
    /// Upgrade the only historical format supported by this application.
    /// This is schema compatibility, independent of file-location migration.
    pub fn upgrade_v1(mut self) -> Self {
        for (index, entry) in self.time_entries.iter_mut().enumerate() {
            entry.id = EntryId(index as u64 + 1);
        }
        self.format_version = CURRENT_FORMAT_VERSION;
        self
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        use std::collections::BTreeSet;

        if self.format_version != CURRENT_FORMAT_VERSION {
            return Err(ModelError::UnsupportedFormat(self.format_version));
        }
        if self
            .projects
            .iter()
            .any(|project| project.name.trim().is_empty())
            || self
                .activities
                .iter()
                .any(|activity| activity.name.trim().is_empty())
        {
            return Err(ModelError::EmptyName);
        }

        let mut entry_ids = BTreeSet::new();
        for entry in &self.time_entries {
            if entry.id.0 == 0 || !entry_ids.insert(entry.id) {
                return Err(ModelError::InvalidEntryId(entry.id.0));
            }
            self.validate_references(entry.activity, entry.project)?;
            let duration = entry
                .time
                .stopped_at
                .0
                .checked_sub(entry.time.started_at.0)
                .ok_or(ModelError::TimestampOutOfRange)?;
            if duration < 0 {
                return Err(ModelError::NegativeDuration);
            }
            if duration > MAX_ENTRY_SECONDS {
                return Err(ModelError::EntryTooLong);
            }
        }
        if let Some(active) = &self.active_entry {
            self.validate_references(active.activity, active.project)?;
        }
        Ok(())
    }

    pub fn start_timer(
        &mut self,
        activity: ActivityId,
        project: Option<ProjectId>,
        now: UnixSeconds,
    ) -> Result<(), ModelError> {
        if self.active_entry.is_some() {
            return Err(ModelError::TimerAlreadyRunning);
        }
        self.validate_references(activity, project)?;
        self.active_entry = Some(ActiveTimeEntry {
            activity,
            project,
            started_at: now,
        });
        Ok(())
    }

    /// Move a running timer's start without changing its activity or project.
    pub fn set_timer_start(
        &mut self,
        started_at: UnixSeconds,
        now: UnixSeconds,
    ) -> Result<(), ModelError> {
        if self.active_entry.is_none() {
            return Err(ModelError::NoActiveTimer);
        }
        let elapsed = now
            .0
            .checked_sub(started_at.0)
            .ok_or(ModelError::TimestampOutOfRange)?;
        if elapsed < 0 {
            return Err(ModelError::TimerStartInFuture);
        }
        if elapsed >= MAX_ENTRY_SECONDS {
            return Err(ModelError::TimerStartTooOld);
        }
        let active = self
            .active_entry
            .as_mut()
            .ok_or(ModelError::NoActiveTimer)?;
        active.started_at = started_at;
        Ok(())
    }

    /// Complete the active timer. Timers are deliberately limited to 24 hours;
    /// a forgotten timer is capped at that boundary instead of producing a
    /// multi-day entry.
    pub fn stop_timer(&mut self, now: UnixSeconds) -> Result<StopOutcome, ModelError> {
        let active = self
            .active_entry
            .as_ref()
            .ok_or(ModelError::NoActiveTimer)?;
        if now.0 < active.started_at.0 {
            return Err(ModelError::ClockMovedBackwards);
        }
        let elapsed = now
            .0
            .checked_sub(active.started_at.0)
            .ok_or(ModelError::TimestampOutOfRange)?;
        let deadline = active
            .started_at
            .0
            .checked_add(MAX_ENTRY_SECONDS)
            .ok_or(ModelError::TimestampOutOfRange)?;
        let capped_at_limit = elapsed >= MAX_ENTRY_SECONDS;
        let stopped_at = UnixSeconds(now.0.min(deadline));
        let active = self.active_entry.take().expect("checked above");
        let entry_id = self.next_entry_id()?;
        self.time_entries.push(TimeEntry {
            id: entry_id,
            activity: active.activity,
            project: active.project,
            time: TimeRange {
                started_at: active.started_at,
                stopped_at,
            },
        });
        self.time_entries.sort_by_key(|entry| entry.time.started_at);
        Ok(StopOutcome {
            entry_id,
            capped_at_limit,
        })
    }

    pub fn entry(&self, id: EntryId) -> Option<&TimeEntry> {
        self.time_entries.iter().find(|entry| entry.id == id)
    }

    pub fn entry_mut(&mut self, id: EntryId) -> Option<&mut TimeEntry> {
        self.time_entries.iter_mut().find(|entry| entry.id == id)
    }

    pub fn remove_entry(&mut self, id: EntryId) -> Option<TimeEntry> {
        let index = self.time_entries.iter().position(|entry| entry.id == id)?;
        Some(self.time_entries.remove(index))
    }

    fn validate_references(
        &self,
        activity: ActivityId,
        project: Option<ProjectId>,
    ) -> Result<(), ModelError> {
        if activity.0 >= self.activities.len() {
            return Err(ModelError::MissingActivity(activity.0));
        }
        if let Some(project) = project.filter(|project| project.0 >= self.projects.len()) {
            return Err(ModelError::MissingProject(project.0));
        }
        Ok(())
    }

    fn next_entry_id(&self) -> Result<EntryId, ModelError> {
        self.time_entries
            .iter()
            .map(|entry| entry.id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .map(EntryId)
            .ok_or(ModelError::EntryIdExhausted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with_activity() -> Store {
        Store {
            activities: vec![Activity {
                name: "Work".to_owned(),
            }],
            ..Store::default()
        }
    }

    #[test]
    fn entry_ids_survive_sorting_and_deletion() {
        let mut store = store_with_activity();
        store
            .start_timer(ActivityId(0), None, UnixSeconds(20))
            .unwrap();
        let second = store.stop_timer(UnixSeconds(30)).unwrap().entry_id;
        store
            .start_timer(ActivityId(0), None, UnixSeconds(0))
            .unwrap();
        let first = store.stop_timer(UnixSeconds(10)).unwrap().entry_id;

        assert_eq!(store.entry(first).unwrap().time.started_at, UnixSeconds(0));
        assert_eq!(
            store.entry(second).unwrap().time.started_at,
            UnixSeconds(20)
        );
        store.remove_entry(first).unwrap();
        assert_eq!(
            store.entry(second).unwrap().time.started_at,
            UnixSeconds(20)
        );
    }

    #[test]
    fn forgotten_timer_is_capped_at_24_hours() {
        let mut store = store_with_activity();
        store
            .start_timer(ActivityId(0), None, UnixSeconds(100))
            .unwrap();
        let outcome = store
            .stop_timer(UnixSeconds(100 + MAX_ENTRY_SECONDS + 60))
            .unwrap();

        assert!(outcome.capped_at_limit);
        assert_eq!(
            store
                .entry(outcome.entry_id)
                .unwrap()
                .time
                .duration_seconds(),
            Some(MAX_ENTRY_SECONDS as u64)
        );
    }

    #[test]
    fn changing_running_start_keeps_timer_active_and_validates_boundaries() {
        let mut store = store_with_activity();
        assert_eq!(
            store.set_timer_start(UnixSeconds(700), UnixSeconds(1_000)),
            Err(ModelError::NoActiveTimer)
        );
        store
            .start_timer(ActivityId(0), None, UnixSeconds(1_000))
            .unwrap();
        store
            .set_timer_start(UnixSeconds(700), UnixSeconds(1_000))
            .unwrap();
        assert_eq!(
            store.active_entry.as_ref().unwrap().started_at,
            UnixSeconds(700)
        );
        assert_eq!(
            store.set_timer_start(UnixSeconds(1_001), UnixSeconds(1_000)),
            Err(ModelError::TimerStartInFuture)
        );
        assert_eq!(
            store.set_timer_start(UnixSeconds(1_000 - MAX_ENTRY_SECONDS), UnixSeconds(1_000)),
            Err(ModelError::TimerStartTooOld)
        );
        assert_eq!(
            store.active_entry.as_ref().unwrap().started_at,
            UnixSeconds(700)
        );
        let outcome = store.stop_timer(UnixSeconds(1_100)).unwrap();
        assert_eq!(
            store
                .entry(outcome.entry_id)
                .unwrap()
                .time
                .duration_seconds(),
            Some(400)
        );
    }
}
