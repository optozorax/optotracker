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

/// Time tracked on one local day, in total and broken down by project and by
/// activity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DayTotals {
    pub seconds: u64,
    pub by_project: BTreeMap<Option<usize>, u64>,
    pub by_activity: BTreeMap<usize, u64>,
}

/// Splitting an interval at local midnight is the expensive part of a scan, so
/// every per-day breakdown is accumulated from the same pass.
pub fn totals_by_local_day(store: &Store, now: i64) -> BTreeMap<NaiveDate, DayTotals> {
    let mut totals: BTreeMap<NaiveDate, DayTotals> = BTreeMap::new();
    for (activity, project, start, stop) in usage_intervals(store, now) {
        for (date, seconds) in local_time::split_interval_by_day(start, stop) {
            let day = totals.entry(date).or_default();
            day.seconds += seconds;
            *day.by_project
                .entry(project.map(|project| project.0))
                .or_default() += seconds;
            *day.by_activity.entry(activity.0).or_default() += seconds;
        }
    }
    totals
}

pub fn add_interval_to_days(totals: &mut BTreeMap<NaiveDate, u64>, start: i64, stop: i64) {
    for (date, seconds) in local_time::split_interval_by_day(start, stop) {
        *totals.entry(date).or_default() += seconds;
    }
}

pub fn totals_by_week(day_totals: &BTreeMap<NaiveDate, DayTotals>) -> BTreeMap<NaiveDate, u64> {
    let mut totals = BTreeMap::new();
    for (&date, day) in day_totals {
        *totals.entry(week_start(date)).or_default() += day.seconds;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ActiveTimeEntry, Activity, EntryId, RgbColor, TimeEntry, TimeRange, UnixSeconds,
    };

    #[test]
    fn day_totals_split_overnight_entries_and_include_running_time() {
        let first = NaiveDate::from_ymd_opt(2026, 9, 14).unwrap();
        let second = first.succ_opt().unwrap();
        let local = |date: NaiveDate, hour: u32, minute: u32| {
            local_time::timestamp(date.and_hms_opt(hour, minute, 0).unwrap()).unwrap()
        };
        let start = local(first, 23, 30);
        let stop = local(second, 0, 30);
        let active_start = local(second, 1, 0);
        let now = local(second, 1, 15);
        let store = Store {
            activities: vec![
                Activity {
                    name: "One".into(),
                    color: RgbColor {
                        red: 1,
                        green: 2,
                        blue: 3,
                    },
                },
                Activity {
                    name: "Two".into(),
                    color: RgbColor {
                        red: 4,
                        green: 5,
                        blue: 6,
                    },
                },
            ],
            time_entries: vec![TimeEntry {
                id: EntryId(1),
                activity: ActivityId(0),
                project: Some(ProjectId(0)),
                time: TimeRange {
                    started_at: UnixSeconds(start),
                    stopped_at: UnixSeconds(stop),
                },
            }],
            active_entry: Some(ActiveTimeEntry {
                activity: ActivityId(1),
                project: None,
                started_at: UnixSeconds(active_start),
            }),
            ..Store::default()
        };

        let totals = totals_by_local_day(&store, now);
        assert_eq!(totals.len(), 2);
        assert_eq!(totals[&first].seconds, 30 * 60);
        assert_eq!(totals[&second].seconds, 45 * 60);
        assert_eq!(totals[&first].by_activity, BTreeMap::from([(0, 30 * 60)]));
        assert_eq!(
            totals[&second].by_activity,
            BTreeMap::from([(0, 30 * 60), (1, 15 * 60)])
        );
        assert_eq!(
            totals[&first].by_project,
            BTreeMap::from([(Some(0), 30 * 60)])
        );
        assert_eq!(
            totals[&second].by_project,
            BTreeMap::from([(Some(0), 30 * 60), (None, 15 * 60)])
        );
        assert_eq!(
            totals_by_week(&totals),
            BTreeMap::from([(week_start(first), 75 * 60)])
        );
    }
}
