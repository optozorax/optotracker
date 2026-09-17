//! Immediate, read-only reports over a tracker snapshot.
//!
//! The expected lifetime data set is small, so these functions favor direct
//! full scans over caches or incrementally maintained aggregates.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Datelike as _, Duration, NaiveDate};

use crate::local_time;
use crate::model::{ActivityId, ProjectId, Store};

#[derive(Clone, Default)]
pub struct UsageStats {
    pub entries: usize,
    pub total_seconds: u64,
    pub active_days: usize,
    pub first_used: Option<i64>,
    pub last_used: Option<i64>,
    pub related_count: usize,
}

pub fn project_stats(store: &Store, project: ProjectId, now: i64) -> UsageStats {
    let mut stats = UsageStats::default();
    let mut days = BTreeSet::new();
    let mut activity_totals: BTreeMap<usize, u64> = BTreeMap::new();
    for (activity, entry_project, start, stop) in usage_intervals(store, now) {
        if entry_project != Some(project) || stop <= start {
            continue;
        }
        let seconds = (stop - start) as u64;
        stats.entries += 1;
        stats.total_seconds += seconds;
        stats.first_used = Some(stats.first_used.map_or(start, |value| value.min(start)));
        stats.last_used = Some(stats.last_used.map_or(start, |value| value.max(start)));
        *activity_totals.entry(activity.0).or_default() += seconds;
        days.extend(local_time::split_interval_by_day(start, stop).into_keys());
    }
    stats.active_days = days.len();
    stats.related_count = activity_totals.len();
    stats
}

pub fn activity_stats(store: &Store, activity: ActivityId, now: i64) -> UsageStats {
    let mut stats = UsageStats::default();
    let mut days = BTreeSet::new();
    let mut project_totals: BTreeMap<Option<usize>, u64> = BTreeMap::new();
    for (entry_activity, project, start, stop) in usage_intervals(store, now) {
        if entry_activity != activity || stop <= start {
            continue;
        }
        let seconds = (stop - start) as u64;
        stats.entries += 1;
        stats.total_seconds += seconds;
        stats.first_used = Some(stats.first_used.map_or(start, |value| value.min(start)));
        stats.last_used = Some(stats.last_used.map_or(start, |value| value.max(start)));
        *project_totals
            .entry(project.map(|project| project.0))
            .or_default() += seconds;
        days.extend(local_time::split_interval_by_day(start, stop).into_keys());
    }
    stats.active_days = days.len();
    stats.related_count = project_totals
        .keys()
        .filter(|project| project.is_some())
        .count();
    stats
}

pub fn activity_totals(store: &Store, now: i64) -> Vec<u64> {
    let mut totals = vec![0; store.activities.len()];
    for (activity, _, start, stop) in usage_intervals(store, now) {
        if let Some(total) = totals.get_mut(activity.0) {
            *total += stop.checked_sub(start).unwrap_or(0).max(0) as u64;
        }
    }
    totals
}

pub fn totals_by_local_day(store: &Store, now: i64) -> BTreeMap<NaiveDate, u64> {
    let mut totals = BTreeMap::new();
    for entry in &store.time_entries {
        add_interval_to_days(
            &mut totals,
            entry.time.started_at.0,
            entry.time.stopped_at.0,
        );
    }
    if let Some(active) = &store.active_entry {
        add_interval_to_days(&mut totals, active.started_at.0, now);
    }
    totals
}

pub fn project_totals_by_local_day(
    store: &Store,
    now: i64,
) -> BTreeMap<NaiveDate, BTreeMap<Option<usize>, u64>> {
    let mut totals = BTreeMap::new();
    for entry in &store.time_entries {
        add_project_interval_to_days(
            &mut totals,
            entry.project.map(|project| project.0),
            entry.time.started_at.0,
            entry.time.stopped_at.0,
        );
    }
    if let Some(active) = &store.active_entry {
        add_project_interval_to_days(
            &mut totals,
            active.project.map(|project| project.0),
            active.started_at.0,
            now,
        );
    }
    totals
}

pub fn add_interval_to_days(totals: &mut BTreeMap<NaiveDate, u64>, start: i64, stop: i64) {
    for (date, seconds) in local_time::split_interval_by_day(start, stop) {
        *totals.entry(date).or_default() += seconds;
    }
}

pub fn totals_by_week(day_totals: &BTreeMap<NaiveDate, u64>) -> BTreeMap<NaiveDate, u64> {
    let mut totals = BTreeMap::new();
    for (&date, &seconds) in day_totals {
        *totals.entry(week_start(date)).or_default() += seconds;
    }
    totals
}

pub fn week_start(date: NaiveDate) -> NaiveDate {
    date - Duration::days(date.weekday().num_days_from_monday() as i64)
}

fn usage_intervals(
    store: &Store,
    now: i64,
) -> impl Iterator<Item = (ActivityId, Option<ProjectId>, i64, i64)> + '_ {
    let completed = store.time_entries.iter().map(|entry| {
        (
            entry.activity,
            entry.project,
            entry.time.started_at.0,
            entry.time.stopped_at.0,
        )
    });
    let active = store.active_entry.iter().map(move |entry| {
        (
            entry.activity,
            entry.project,
            entry.started_at.0,
            now.max(entry.started_at.0),
        )
    });
    completed.chain(active)
}

fn add_project_interval_to_days(
    totals: &mut BTreeMap<NaiveDate, BTreeMap<Option<usize>, u64>>,
    project: Option<usize>,
    start: i64,
    stop: i64,
) {
    for (date, seconds) in local_time::split_interval_by_day(start, stop) {
        *totals.entry(date).or_default().entry(project).or_default() += seconds;
    }
}
