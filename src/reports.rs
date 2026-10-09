//! Immediate, read-only reports over a tracker snapshot.
//!
//! The expected lifetime data set is small, so these functions favor direct
//! full scans over caches or incrementally maintained aggregates.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;

use chrono::{Datelike as _, Duration, NaiveDate, Timelike as _};

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

/// Collects the statistics of every interval that is added to it.
#[derive(Default)]
struct Usage {
    stats: UsageStats,
    days: BTreeSet<NaiveDate>,
}

impl Usage {
    fn add(&mut self, start: i64, stop: i64) {
        let stats = &mut self.stats;
        stats.entries += 1;
        stats.total_seconds += (stop - start) as u64;
        stats.first_used = Some(stats.first_used.map_or(start, |value| value.min(start)));
        stats.last_used = Some(stats.last_used.map_or(start, |value| value.max(start)));
        self.days
            .extend(local_time::split_interval_by_day(start, stop).into_keys());
    }

    fn finish(mut self, related_count: usize) -> UsageStats {
        self.stats.active_days = self.days.len();
        self.stats.related_count = related_count;
        self.stats
    }
}

pub fn project_stats(store: &Store, project: ProjectId, now: i64) -> UsageStats {
    let mut usage = Usage::default();
    let mut activities = BTreeSet::new();
    for (activity, entry_project, start, stop) in usage_intervals(store, now) {
        if entry_project != Some(project) || stop <= start {
            continue;
        }
        usage.add(start, stop);
        activities.insert(activity.0);
    }
    usage.finish(activities.len())
}

/// The statistics of one project split by activity, largest total first.
/// Activities the project never used are left out.
pub fn project_activity_stats(
    store: &Store,
    project: ProjectId,
    now: i64,
) -> Vec<(ActivityId, UsageStats)> {
    let mut by_activity: BTreeMap<usize, Usage> = BTreeMap::new();
    for (activity, entry_project, start, stop) in usage_intervals(store, now) {
        if entry_project != Some(project) || stop <= start {
            continue;
        }
        by_activity.entry(activity.0).or_default().add(start, stop);
    }
    let mut stats: Vec<(ActivityId, UsageStats)> = by_activity
        .into_iter()
        .map(|(activity, usage)| (ActivityId(activity), usage.finish(0)))
        .collect();
    stats.sort_by_key(|(_, stats)| std::cmp::Reverse(stats.total_seconds));
    stats
}

pub fn activity_stats(store: &Store, activity: ActivityId, now: i64) -> UsageStats {
    let mut usage = Usage::default();
    let mut projects = BTreeSet::new();
    for (entry_activity, project, start, stop) in usage_intervals(store, now) {
        if entry_activity != activity || stop <= start {
            continue;
        }
        usage.add(start, stop);
        projects.extend(project.map(|project| project.0));
    }
    usage.finish(projects.len())
}

/// The statistics of one activity split by project, largest total first.
/// `None` is the time tracked without a project.
pub fn activity_project_stats(
    store: &Store,
    activity: ActivityId,
    now: i64,
) -> Vec<(Option<ProjectId>, UsageStats)> {
    let mut by_project: BTreeMap<Option<usize>, Usage> = BTreeMap::new();
    for (entry_activity, project, start, stop) in usage_intervals(store, now) {
        if entry_activity != activity || stop <= start {
            continue;
        }
        by_project
            .entry(project.map(|project| project.0))
            .or_default()
            .add(start, stop);
    }
    let mut stats: Vec<(Option<ProjectId>, UsageStats)> = by_project
        .into_iter()
        .map(|(project, usage)| (project.map(ProjectId), usage.finish(0)))
        .collect();
    stats.sort_by_key(|(_, stats)| std::cmp::Reverse(stats.total_seconds));
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

impl DayTotals {
    fn add(&mut self, other: &Self) {
        self.seconds += other.seconds;
        for (&project, &seconds) in &other.by_project {
            *self.by_project.entry(project).or_default() += seconds;
        }
        for (&activity, &seconds) in &other.by_activity {
            *self.by_activity.entry(activity).or_default() += seconds;
        }
    }
}

/// The days of a range that have tracked time.
fn tracked_days<'a>(
    day_totals: &'a BTreeMap<NaiveDate, DayTotals>,
    range: &RangeInclusive<NaiveDate>,
) -> impl Iterator<Item = (NaiveDate, &'a DayTotals)> {
    // `BTreeMap::range` panics on a range that ends before it starts.
    let range = (range.start() <= range.end()).then(|| range.clone());
    range
        .into_iter()
        .flat_map(move |range| day_totals.range(range))
        .filter(|(_, day)| day.seconds > 0)
        .map(|(&date, day)| (date, day))
}

pub fn range_totals(
    day_totals: &BTreeMap<NaiveDate, DayTotals>,
    range: &RangeInclusive<NaiveDate>,
) -> DayTotals {
    let mut totals = DayTotals::default();
    for (_, day) in tracked_days(day_totals, range) {
        totals.add(day);
    }
    totals
}

pub fn worked_days(
    day_totals: &BTreeMap<NaiveDate, DayTotals>,
    range: &RangeInclusive<NaiveDate>,
) -> usize {
    tracked_days(day_totals, range).count()
}

/// The day of a range with the most tracked time, and that time.
pub fn busiest_day(
    day_totals: &BTreeMap<NaiveDate, DayTotals>,
    range: &RangeInclusive<NaiveDate>,
) -> Option<(NaiveDate, u64)> {
    tracked_days(day_totals, range)
        .map(|(date, day)| (date, day.seconds))
        // The earliest of equally long days.
        .max_by_key(|&(date, seconds)| (seconds, std::cmp::Reverse(date)))
}

pub fn days_in(range: &RangeInclusive<NaiveDate>) -> u32 {
    ((*range.end() - *range.start()).num_days() + 1).max(0) as u32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BucketSize {
    Day,
    Week,
    Month,
}

/// The days that a chart draws as one bar, with what was tracked on them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bucket {
    pub first: NaiveDate,
    pub last: NaiveDate,
    pub totals: DayTotals,
}

/// Divides a range into days, weeks that start on Monday or calendar months.
/// A week or month that the range only partly covers is cut at its ends.
pub fn buckets(
    day_totals: &BTreeMap<NaiveDate, DayTotals>,
    range: &RangeInclusive<NaiveDate>,
    size: BucketSize,
) -> Vec<Bucket> {
    let mut buckets = Vec::new();
    let mut first = *range.start();
    while first <= *range.end() {
        let last = match size {
            BucketSize::Day => first,
            BucketSize::Week => week_start(first) + Duration::days(6),
            BucketSize::Month => month_end(first),
        }
        .min(*range.end());
        buckets.push(Bucket {
            first,
            last,
            totals: range_totals(day_totals, &(first..=last)),
        });
        let Some(next) = last.succ_opt() else {
            break;
        };
        first = next;
    }
    buckets
}

/// What one weekday of a range adds up to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WeekdayTotals {
    pub seconds: u64,
    /// How many times the weekday occurs in the range.
    pub days: u32,
    /// How many of those have tracked time.
    pub worked_days: u32,
}

/// The weekdays of a range, Monday first.
pub fn weekday_totals(
    day_totals: &BTreeMap<NaiveDate, DayTotals>,
    range: &RangeInclusive<NaiveDate>,
) -> [WeekdayTotals; 7] {
    let mut totals = [WeekdayTotals::default(); 7];
    let days = days_in(range);
    let first_weekday = range.start().weekday().num_days_from_monday();
    for (weekday, total) in totals.iter_mut().enumerate() {
        // Every weekday occurs once in each full week; the days left over
        // begin with the first weekday of the range.
        let after_first = (weekday as u32 + 7 - first_weekday) % 7;
        total.days = days / 7 + u32::from(after_first < days % 7);
    }
    for (date, day) in tracked_days(day_totals, range) {
        let total = &mut totals[date.weekday().num_days_from_monday() as usize];
        total.seconds += day.seconds;
        total.worked_days += 1;
    }
    totals
}

/// The time tracked in each hour of the day, on the days of a range.
pub fn hour_totals(store: &Store, now: i64, range: &RangeInclusive<NaiveDate>) -> [u64; 24] {
    let mut totals = [0; 24];
    for (_, _, start, stop) in usage_intervals(store, now) {
        if stop <= start {
            continue;
        }
        // The whole interval is read in the clock of its start. A clock
        // change in the middle of it misplaces at most an hour.
        let mut cursor = local_time::datetime(start).naive_local();
        let end = cursor + Duration::seconds(stop - start);
        while cursor < end {
            let seconds_into_hour = i64::from(cursor.minute() * 60 + cursor.second());
            let piece_end = end.min(cursor + Duration::seconds(3600 - seconds_into_hour));
            if range.contains(&cursor.date()) {
                totals[cursor.hour() as usize] += (piece_end - cursor).num_seconds() as u64;
            }
            cursor = piece_end;
        }
    }
    totals
}

/// Entries that follow each other within this many seconds are one session.
const SESSION_GAP_SECONDS: i64 = 5 * 60;

/// The tracked seconds of every session that began on a day of the range,
/// shortest first.
pub fn session_lengths(store: &Store, now: i64, range: &RangeInclusive<NaiveDate>) -> Vec<u64> {
    let mut intervals: Vec<(i64, i64)> = usage_intervals(store, now)
        .filter(|&(_, _, start, stop)| stop > start)
        .map(|(_, _, start, stop)| (start, stop))
        .collect();
    intervals.sort_unstable();
    // Each session is its start, its end and the time tracked in between.
    let mut sessions: Vec<(i64, i64, u64)> = Vec::new();
    for (start, stop) in intervals {
        let seconds = (stop - start) as u64;
        match sessions.last_mut() {
            Some(session) if start - session.1 < SESSION_GAP_SECONDS => {
                session.1 = session.1.max(stop);
                session.2 += seconds;
            }
            _ => sessions.push((start, stop, seconds)),
        }
    }
    let mut lengths: Vec<u64> = sessions
        .into_iter()
        .filter(|&(start, _, _)| range.contains(&local_time::datetime(start).date_naive()))
        .map(|(_, _, seconds)| seconds)
        .collect();
    lengths.sort_unstable();
    lengths
}

/// The longest run of consecutive days with tracked time in a range: its
/// first day and its length in days. The earliest of equally long runs.
pub fn longest_streak(
    day_totals: &BTreeMap<NaiveDate, DayTotals>,
    range: &RangeInclusive<NaiveDate>,
) -> Option<(NaiveDate, u32)> {
    let mut longest: Option<(NaiveDate, u32)> = None;
    let mut current: Option<(NaiveDate, u32)> = None;
    for (date, _) in tracked_days(day_totals, range) {
        current = match current {
            Some((first, length)) if first + Duration::days(i64::from(length)) == date => {
                Some((first, length + 1))
            }
            _ => Some((date, 1)),
        };
        if current.map(|(_, length)| length) > longest.map(|(_, length)| length) {
            longest = current;
        }
    }
    longest
}

/// How many days in a row have tracked time, up to today. A day that has
/// only just begun does not break the run that reached yesterday.
pub fn current_streak(day_totals: &BTreeMap<NaiveDate, DayTotals>, today: NaiveDate) -> u32 {
    let tracked = |date: NaiveDate| day_totals.get(&date).is_some_and(|day| day.seconds > 0);
    let mut date = if tracked(today) {
        today
    } else {
        today - Duration::days(1)
    };
    let mut length = 0;
    while tracked(date) {
        length += 1;
        date -= Duration::days(1);
    }
    length
}

/// How the time of one project divides among activities.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectMix {
    pub seconds: u64,
    pub by_activity: BTreeMap<usize, u64>,
    /// When its first and its last entry started.
    pub first_used: Option<i64>,
    pub last_used: Option<i64>,
}

/// The mix of every project, in the order of the projects.
pub fn project_mixes(store: &Store, now: i64) -> Vec<ProjectMix> {
    let mut mixes = vec![ProjectMix::default(); store.projects.len()];
    for (activity, project, start, stop) in usage_intervals(store, now) {
        let Some(mix) = project.and_then(|project| mixes.get_mut(project.0)) else {
            continue;
        };
        if stop <= start {
            continue;
        }
        let seconds = (stop - start) as u64;
        mix.seconds += seconds;
        *mix.by_activity.entry(activity.0).or_default() += seconds;
        mix.first_used = Some(mix.first_used.map_or(start, |value| value.min(start)));
        mix.last_used = Some(mix.last_used.map_or(start, |value| value.max(start)));
    }
    mixes
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

pub fn month_start(date: NaiveDate) -> NaiveDate {
    date.with_day(1).expect("every month has a first day")
}

pub fn month_end(date: NaiveDate) -> NaiveDate {
    let (year, month) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    NaiveDate::from_ymd_opt(year, month, 1)
        .and_then(|next_month| next_month.pred_opt())
        .unwrap_or(date)
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
        ActiveTimeEntry, Activity, EntryId, Project, ProjectStatus, RgbColor, TimeEntry, TimeRange,
        UnixSeconds,
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

    #[test]
    fn project_activities_cover_only_that_project_and_start_with_the_largest() {
        let day = NaiveDate::from_ymd_opt(2026, 9, 14).unwrap();
        let local = |hour: u32, minute: u32| {
            local_time::timestamp(day.and_hms_opt(hour, minute, 0).unwrap()).unwrap()
        };
        let entry = |id, activity, project, start, stop| TimeEntry {
            id: EntryId(id),
            activity: ActivityId(activity),
            project,
            time: TimeRange {
                started_at: UnixSeconds(start),
                stopped_at: UnixSeconds(stop),
            },
        };
        let project = Some(ProjectId(0));
        let store = Store {
            time_entries: vec![
                entry(1, 0, project, local(9, 0), local(9, 30)),
                entry(2, 1, project, local(10, 0), local(11, 0)),
                entry(3, 0, Some(ProjectId(1)), local(12, 0), local(14, 0)),
                entry(4, 1, None, local(15, 0), local(16, 0)),
            ],
            active_entry: Some(ActiveTimeEntry {
                activity: ActivityId(1),
                project,
                started_at: UnixSeconds(local(17, 0)),
            }),
            ..Store::default()
        };

        let activities = project_activity_stats(&store, ProjectId(0), local(17, 15));
        let summary: Vec<(ActivityId, u64, usize, usize)> = activities
            .iter()
            .map(|(activity, stats)| {
                (
                    *activity,
                    stats.total_seconds,
                    stats.entries,
                    stats.active_days,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                (ActivityId(1), 75 * 60, 2, 1),
                (ActivityId(0), 30 * 60, 1, 1)
            ]
        );
        assert_eq!(activities[0].1.first_used, Some(local(10, 0)));
        assert_eq!(activities[0].1.last_used, Some(local(17, 0)));

        let whole = project_stats(&store, ProjectId(0), local(17, 15));
        assert_eq!(whole.total_seconds, 105 * 60);
        assert_eq!(whole.related_count, 2);
        assert!(project_activity_stats(&store, ProjectId(2), local(17, 15)).is_empty());
    }

    fn date(month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, month, day).unwrap()
    }

    fn local(date: NaiveDate, hour: u32, minute: u32) -> i64 {
        local_time::timestamp(date.and_hms_opt(hour, minute, 0).unwrap()).unwrap()
    }

    fn entry(id: u64, activity: usize, project: Option<usize>, start: i64, stop: i64) -> TimeEntry {
        TimeEntry {
            id: EntryId(id),
            activity: ActivityId(activity),
            project: project.map(ProjectId),
            time: TimeRange {
                started_at: UnixSeconds(start),
                stopped_at: UnixSeconds(stop),
            },
        }
    }

    /// Day totals that hold nothing but the given minutes.
    fn tracked(days: &[(NaiveDate, u64)]) -> BTreeMap<NaiveDate, DayTotals> {
        days.iter()
            .map(|&(date, minutes)| {
                let totals = DayTotals {
                    seconds: minutes * 60,
                    ..DayTotals::default()
                };
                (date, totals)
            })
            .collect()
    }

    #[test]
    fn activity_projects_include_the_time_without_a_project() {
        let day = date(9, 14);
        let store = Store {
            time_entries: vec![
                entry(1, 0, Some(0), local(day, 9, 0), local(day, 10, 0)),
                entry(2, 0, None, local(day, 10, 0), local(day, 12, 0)),
                entry(3, 0, Some(1), local(day, 12, 0), local(day, 12, 30)),
                entry(4, 1, Some(1), local(day, 13, 0), local(day, 18, 0)),
            ],
            ..Store::default()
        };

        let projects: Vec<(Option<ProjectId>, u64)> =
            activity_project_stats(&store, ActivityId(0), local(day, 19, 0))
                .into_iter()
                .map(|(project, stats)| (project, stats.total_seconds))
                .collect();
        assert_eq!(
            projects,
            [
                (None, 120 * 60),
                (Some(ProjectId(0)), 60 * 60),
                (Some(ProjectId(1)), 30 * 60)
            ]
        );
    }

    #[test]
    fn buckets_are_cut_at_the_ends_of_the_range() {
        // From a Wednesday to the Tuesday two weeks later.
        let range = date(9, 30)..=date(10, 13);
        let days = tracked(&[
            (date(9, 29), 500),
            (date(9, 30), 10),
            (date(10, 4), 20),
            (date(10, 5), 30),
            (date(10, 13), 40),
            (date(10, 14), 500),
        ]);
        let summary = |size| -> Vec<(NaiveDate, NaiveDate, u64)> {
            buckets(&days, &range, size)
                .into_iter()
                .map(|bucket| (bucket.first, bucket.last, bucket.totals.seconds / 60))
                .collect()
        };
        assert_eq!(
            summary(BucketSize::Week),
            [
                (date(9, 30), date(10, 4), 30),
                (date(10, 5), date(10, 11), 30),
                (date(10, 12), date(10, 13), 40)
            ]
        );
        assert_eq!(
            summary(BucketSize::Month),
            [
                (date(9, 30), date(9, 30), 10),
                (date(10, 1), date(10, 13), 90)
            ]
        );
        assert_eq!(summary(BucketSize::Day).len(), 14);
        assert_eq!(range_totals(&days, &range).seconds, 100 * 60);
        assert_eq!(worked_days(&days, &range), 4);
        assert_eq!(busiest_day(&days, &range), Some((date(10, 13), 40 * 60)));
        // A range that ends before it starts holds nothing.
        assert_eq!(worked_days(&days, &(date(10, 13)..=date(9, 30))), 0);
    }

    #[test]
    fn weekdays_count_every_day_of_the_range() {
        // Ten days from a Monday: two Mondays, Tuesdays and Wednesdays.
        let range = date(10, 5)..=date(10, 14);
        let days = tracked(&[(date(10, 5), 60), (date(10, 12), 120), (date(10, 10), 30)]);
        let weekdays = weekday_totals(&days, &range);
        assert_eq!(weekdays.map(|weekday| weekday.days), [2, 2, 2, 1, 1, 1, 1]);
        assert_eq!(
            weekdays[0],
            WeekdayTotals {
                seconds: 180 * 60,
                days: 2,
                worked_days: 2
            }
        );
        assert_eq!(weekdays[5].seconds, 30 * 60);
        assert_eq!(weekdays[1].worked_days, 0);
    }

    #[test]
    fn hours_are_split_at_the_full_hour_and_kept_to_the_range() {
        let (first, second) = (date(9, 14), date(9, 15));
        let store = Store {
            time_entries: vec![entry(
                1,
                0,
                None,
                local(first, 22, 30),
                local(second, 0, 45),
            )],
            ..Store::default()
        };
        let now = local(second, 12, 0);

        let first_day = hour_totals(&store, now, &(first..=first));
        assert_eq!(first_day[22], 30 * 60);
        assert_eq!(first_day[23], 60 * 60);
        assert_eq!(first_day.iter().sum::<u64>(), 90 * 60);
        let both_days = hour_totals(&store, now, &(first..=second));
        assert_eq!(both_days[0], 45 * 60);
        assert_eq!(both_days.iter().sum::<u64>(), 135 * 60);
    }

    #[test]
    fn entries_close_together_are_one_session() {
        let day = date(9, 14);
        let store = Store {
            time_entries: vec![
                entry(1, 0, None, local(day, 9, 0), local(day, 10, 0)),
                // Three minutes later: the same session.
                entry(2, 1, None, local(day, 10, 3), local(day, 11, 0)),
                // Half an hour later: a new one, which the running timer joins.
                entry(3, 0, None, local(day, 11, 30), local(day, 12, 0)),
            ],
            active_entry: Some(ActiveTimeEntry {
                activity: ActivityId(0),
                project: None,
                started_at: UnixSeconds(local(day, 12, 2)),
            }),
            ..Store::default()
        };
        let now = local(day, 12, 10);

        assert_eq!(
            session_lengths(&store, now, &(day..=day)),
            [38 * 60, 117 * 60]
        );
        let day_before = day.pred_opt().unwrap();
        assert!(session_lengths(&store, now, &(day_before..=day_before)).is_empty());
    }

    #[test]
    fn streaks_are_days_in_a_row() {
        let days: Vec<(NaiveDate, u64)> = [1, 2, 3, 5, 6, 7, 8, 10]
            .into_iter()
            .map(|day| (date(10, day), 60))
            .collect();
        let days = tracked(&days);

        assert_eq!(
            longest_streak(&days, &(date(10, 1)..=date(10, 31))),
            Some((date(10, 5), 4))
        );
        // The run that the range cuts short loses to the one before it.
        assert_eq!(
            longest_streak(&days, &(date(10, 1)..=date(10, 6))),
            Some((date(10, 1), 3))
        );
        assert_eq!(longest_streak(&days, &(date(11, 1)..=date(11, 30))), None);
        assert_eq!(current_streak(&days, date(10, 8)), 4);
        // Nothing is tracked on the 9th yet, but the run reached the 8th.
        assert_eq!(current_streak(&days, date(10, 9)), 4);
        assert_eq!(current_streak(&days, date(10, 12)), 0);
    }

    #[test]
    fn project_mixes_add_up_each_activity_of_a_project() {
        let day = date(9, 14);
        let project = || Project {
            name: "Project".into(),
            color: RgbColor {
                red: 1,
                green: 2,
                blue: 3,
            },
            status: ProjectStatus::Active,
        };
        let store = Store {
            projects: vec![project(), project()],
            time_entries: vec![
                entry(1, 0, Some(0), local(day, 9, 0), local(day, 10, 0)),
                entry(2, 1, Some(0), local(day, 10, 0), local(day, 10, 30)),
                entry(3, 0, Some(0), local(day, 11, 0), local(day, 11, 15)),
                entry(4, 0, None, local(day, 12, 0), local(day, 13, 0)),
            ],
            ..Store::default()
        };

        let mixes = project_mixes(&store, local(day, 14, 0));
        assert_eq!(mixes.len(), 2);
        assert_eq!(mixes[0].seconds, 105 * 60);
        assert_eq!(
            mixes[0].by_activity,
            BTreeMap::from([(0, 75 * 60), (1, 30 * 60)])
        );
        assert_eq!(mixes[0].first_used, Some(local(day, 9, 0)));
        assert_eq!(mixes[0].last_used, Some(local(day, 11, 0)));
        assert_eq!(mixes[1], ProjectMix::default());
    }
}
