//! Local-calendar policy for the application.
//!
//! Stored timestamps are UTC Unix seconds, while all dates shown to the user
//! and all day boundaries follow the computer's current local timezone.

use std::collections::BTreeMap;

use chrono::{
    DateTime, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone as _, Timelike as _,
};

use crate::model::{TimeRange, UnixSeconds, MAX_ENTRY_SECONDS};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TimeEndpoint {
    Start,
    Stop,
}

pub fn datetime(seconds: i64) -> DateTime<Local> {
    DateTime::from_timestamp(seconds, 0)
        .unwrap_or(DateTime::UNIX_EPOCH)
        .with_timezone(&Local)
}

pub fn timestamp(value: NaiveDateTime) -> Result<i64, String> {
    match Local.from_local_datetime(&value) {
        LocalResult::Single(value) => Ok(value.timestamp()),
        LocalResult::Ambiguous(first, _) => Ok(first.timestamp()),
        LocalResult::None => Err("That local time does not exist on this computer".to_owned()),
    }
}

pub fn split_interval_by_day(start: i64, stop: i64) -> BTreeMap<NaiveDate, u64> {
    let mut totals = BTreeMap::new();
    if stop <= start {
        return totals;
    }

    let mut cursor = start;
    while cursor < stop {
        let date = datetime(cursor).date_naive();
        let Some(next_date) = date.succ_opt() else {
            break;
        };
        let next_midnight = timestamp(next_date.and_hms_opt(0, 0, 0).expect("valid midnight"))
            .unwrap_or(stop)
            .max(cursor + 1);
        let segment_end = stop.min(next_midnight);
        *totals.entry(date).or_default() += (segment_end - cursor) as u64;
        cursor = segment_end;
    }
    totals
}

pub fn edit_time(
    original: TimeRange,
    endpoint: TimeEndpoint,
    edited_text: &str,
) -> Result<TimeRange, String> {
    let original_start = datetime(original.started_at.0);
    let original_stop = datetime(original.stopped_at.0);
    let original_endpoint = match endpoint {
        TimeEndpoint::Start => original_start.time(),
        TimeEndpoint::Stop => original_stop.time(),
    };
    let edited = NaiveTime::parse_from_str(edited_text.trim(), "%H:%M")
        .map_err(|_| "Time must use HH:MM".to_owned())?
        .with_second(original_endpoint.second())
        .and_then(|time| time.with_nanosecond(original_endpoint.nanosecond()))
        .ok_or_else(|| "Time is out of range".to_owned())?;
    let date = original_start.date_naive();
    let start_time = if endpoint == TimeEndpoint::Start {
        edited
    } else {
        original_start.time()
    };
    let stop_time = if endpoint == TimeEndpoint::Stop {
        edited
    } else {
        original_stop.time()
    };
    if start_time == stop_time {
        return Err("Start and stop time cannot be equal".to_owned());
    }
    let stop_date = if stop_time < start_time {
        date.succ_opt()
            .ok_or_else(|| "Date is out of range".to_owned())?
    } else {
        date
    };
    let started_at = timestamp(NaiveDateTime::new(date, start_time))?;
    let stopped_at = timestamp(NaiveDateTime::new(stop_date, stop_time))?;
    let duration = stopped_at - started_at;
    if duration <= 0 || duration > MAX_ENTRY_SECONDS {
        return Err("A time entry must be between 1 second and 24 hours".to_owned());
    }
    Ok(TimeRange {
        started_at: UnixSeconds(started_at),
        stopped_at: UnixSeconds(stopped_at),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_durations_add_up_to_the_original_interval() {
        let start = timestamp(
            NaiveDate::from_ymd_opt(2026, 9, 15)
                .unwrap()
                .and_hms_opt(23, 30, 0)
                .unwrap(),
        )
        .unwrap();
        let stop = timestamp(
            NaiveDate::from_ymd_opt(2026, 9, 16)
                .unwrap()
                .and_hms_opt(2, 20, 0)
                .unwrap(),
        )
        .unwrap();

        let split = split_interval_by_day(start, stop);
        assert_eq!(split.values().sum::<u64>(), (stop - start) as u64);
        assert_eq!(
            split[&NaiveDate::from_ymd_opt(2026, 9, 15).unwrap()],
            30 * 60
        );
        assert_eq!(
            split[&NaiveDate::from_ymd_opt(2026, 9, 16).unwrap()],
            2 * 60 * 60 + 20 * 60
        );
    }

    #[test]
    fn stop_before_start_is_interpreted_as_the_following_day() {
        let start = timestamp(
            NaiveDate::from_ymd_opt(2026, 9, 15)
                .unwrap()
                .and_hms_opt(23, 0, 0)
                .unwrap(),
        )
        .unwrap();
        let stop = timestamp(
            NaiveDate::from_ymd_opt(2026, 9, 16)
                .unwrap()
                .and_hms_opt(0, 2, 0)
                .unwrap(),
        )
        .unwrap();

        let edited = edit_time(
            TimeRange {
                started_at: UnixSeconds(start),
                stopped_at: UnixSeconds(stop),
            },
            TimeEndpoint::Stop,
            "02:20",
        )
        .unwrap();
        assert_eq!(edited.duration_seconds(), Some(3 * 60 * 60 + 20 * 60));
    }

    #[test]
    fn unchanged_displayed_time_preserves_hidden_seconds() {
        let start = timestamp(
            NaiveDate::from_ymd_opt(2026, 9, 16)
                .unwrap()
                .and_hms_opt(15, 2, 17)
                .unwrap(),
        )
        .unwrap();
        let stop = timestamp(
            NaiveDate::from_ymd_opt(2026, 9, 16)
                .unwrap()
                .and_hms_opt(15, 58, 43)
                .unwrap(),
        )
        .unwrap();
        let original = TimeRange {
            started_at: UnixSeconds(start),
            stopped_at: UnixSeconds(stop),
        };

        assert_eq!(
            edit_time(original, TimeEndpoint::Start, "15:02").unwrap(),
            original
        );
    }
}
