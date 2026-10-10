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

/// Why an edited time or date cannot be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TimeError {
    #[error("That local time does not exist on this computer")]
    NoSuchLocalTime,
    #[error("Time is out of range")]
    TimeOutOfRange,
    #[error("Date is out of range")]
    DateOutOfRange,
    #[error("Start and stop time cannot be equal")]
    SameStartAndStop,
    #[error("A time entry must be between 1 second and 24 hours")]
    EntryLength,
    #[error("Time must use HH:MM or HHMM")]
    Format,
}

pub fn datetime(seconds: i64) -> DateTime<Local> {
    DateTime::from_timestamp(seconds, 0)
        .unwrap_or(DateTime::UNIX_EPOCH)
        .with_timezone(&Local)
}

pub fn timestamp(value: NaiveDateTime) -> Result<i64, TimeError> {
    match Local.from_local_datetime(&value) {
        LocalResult::Single(value) => Ok(value.timestamp()),
        LocalResult::Ambiguous(first, _) => Ok(first.timestamp()),
        LocalResult::None => Err(TimeError::NoSuchLocalTime),
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
) -> Result<TimeRange, TimeError> {
    let original_start = datetime(original.started_at.0);
    let original_stop = datetime(original.stopped_at.0);
    let original_endpoint = match endpoint {
        TimeEndpoint::Start => original_start.time(),
        TimeEndpoint::Stop => original_stop.time(),
    };
    let edited = parse_time(edited_text)?
        .with_second(original_endpoint.second())
        .and_then(|time| time.with_nanosecond(original_endpoint.nanosecond()))
        .ok_or(TimeError::TimeOutOfRange)?;
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
        return Err(TimeError::SameStartAndStop);
    }
    let stop_date = if stop_time < start_time {
        date.succ_opt().ok_or(TimeError::DateOutOfRange)?
    } else {
        date
    };
    let started_at = timestamp(NaiveDateTime::new(date, start_time))?;
    let stopped_at = timestamp(NaiveDateTime::new(stop_date, stop_time))?;
    let duration = stopped_at - started_at;
    if duration <= 0 || duration > MAX_ENTRY_SECONDS {
        return Err(TimeError::EntryLength);
    }
    Ok(TimeRange {
        started_at: UnixSeconds(started_at),
        stopped_at: UnixSeconds(stopped_at),
    })
}

/// Moves an entry to a new local start date while preserving its displayed
/// start and stop times and whether its stop falls on a later local date.
pub fn edit_date(original: TimeRange, date: NaiveDate) -> Result<TimeRange, TimeError> {
    let original_start = datetime(original.started_at.0);
    let original_stop = datetime(original.stopped_at.0);
    let day_offset = original_stop
        .date_naive()
        .signed_duration_since(original_start.date_naive())
        .num_days();
    let stop_date = date
        .checked_add_signed(chrono::Duration::days(day_offset))
        .ok_or(TimeError::DateOutOfRange)?;
    let started_at = timestamp(NaiveDateTime::new(date, original_start.time()))?;
    let stopped_at = timestamp(NaiveDateTime::new(stop_date, original_stop.time()))?;
    let duration = stopped_at - started_at;
    if duration <= 0 || duration > MAX_ENTRY_SECONDS {
        return Err(TimeError::EntryLength);
    }
    Ok(TimeRange {
        started_at: UnixSeconds(started_at),
        stopped_at: UnixSeconds(stopped_at),
    })
}

/// Interpret a running timer's edited clock time in local time. A clock time
/// later than the current time belongs to the previous day.
pub fn edit_active_start(
    original: UnixSeconds,
    now: UnixSeconds,
    time_text: &str,
) -> Result<UnixSeconds, TimeError> {
    let original_time = datetime(original.0).time();
    let mut time = parse_time(time_text)?;
    if time.hour() == original_time.hour() && time.minute() == original_time.minute() {
        time = time
            .with_second(original_time.second())
            .ok_or(TimeError::TimeOutOfRange)?;
    }
    let current = datetime(now.0);
    let date = if (time.hour(), time.minute()) > (current.hour(), current.minute()) {
        current
            .date_naive()
            .pred_opt()
            .ok_or(TimeError::DateOutOfRange)?
    } else {
        current.date_naive()
    };
    timestamp(NaiveDateTime::new(date, time)).map(UnixSeconds)
}

fn parse_time(text: &str) -> Result<NaiveTime, TimeError> {
    let text = text.trim();
    let format = match text.as_bytes() {
        [_, _, b':', _, _] => "%H:%M",
        [_, _, _, _] => "%H%M",
        _ => return Err(TimeError::Format),
    };
    NaiveTime::parse_from_str(text, format).map_err(|_| TimeError::Format)
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

    #[test]
    fn active_start_edit_uses_today_or_yesterday_from_the_clock_time() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 25).unwrap();
        let yesterday = today.pred_opt().unwrap();
        let now = UnixSeconds(timestamp(today.and_hms_opt(2, 0, 30).unwrap()).unwrap());
        let original = UnixSeconds(timestamp(today.and_hms_opt(1, 34, 17).unwrap()).unwrap());
        assert_eq!(edit_active_start(original, now, "01:34").unwrap(), original);
        assert_eq!(
            edit_active_start(original, now, "23:30").unwrap(),
            UnixSeconds(timestamp(yesterday.and_hms_opt(23, 30, 0).unwrap()).unwrap())
        );
        assert_eq!(
            edit_active_start(original, now, "01:15").unwrap(),
            UnixSeconds(timestamp(today.and_hms_opt(1, 15, 0).unwrap()).unwrap())
        );
    }

    #[test]
    fn compact_time_without_colon_is_accepted() {
        let start = timestamp(
            NaiveDate::from_ymd_opt(2026, 9, 16)
                .unwrap()
                .and_hms_opt(9, 15, 17)
                .unwrap(),
        )
        .unwrap();
        let stop = timestamp(
            NaiveDate::from_ymd_opt(2026, 9, 16)
                .unwrap()
                .and_hms_opt(10, 30, 43)
                .unwrap(),
        )
        .unwrap();

        let edited = edit_time(
            TimeRange {
                started_at: UnixSeconds(start),
                stopped_at: UnixSeconds(stop),
            },
            TimeEndpoint::Start,
            "0945",
        )
        .unwrap();

        assert_eq!(datetime(edited.started_at.0).time().hour(), 9);
        assert_eq!(datetime(edited.started_at.0).time().minute(), 45);
        assert_eq!(datetime(edited.started_at.0).time().second(), 17);
    }

    #[test]
    fn malformed_time_reports_both_accepted_formats() {
        let error = parse_time("945").unwrap_err();
        assert_eq!(error.to_string(), "Time must use HH:MM or HHMM");
    }

    #[test]
    fn editing_date_preserves_clock_times_and_hidden_seconds() {
        let original = TimeRange {
            started_at: UnixSeconds(
                timestamp(
                    NaiveDate::from_ymd_opt(2026, 9, 16)
                        .unwrap()
                        .and_hms_opt(9, 15, 17)
                        .unwrap(),
                )
                .unwrap(),
            ),
            stopped_at: UnixSeconds(
                timestamp(
                    NaiveDate::from_ymd_opt(2026, 9, 16)
                        .unwrap()
                        .and_hms_opt(10, 30, 43)
                        .unwrap(),
                )
                .unwrap(),
            ),
        };

        let date = NaiveDate::from_ymd_opt(2026, 8, 2).unwrap();
        let edited = edit_date(original, date).unwrap();

        assert_eq!(datetime(edited.started_at.0).date_naive(), date);
        assert_eq!(
            datetime(edited.started_at.0).time(),
            datetime(original.started_at.0).time()
        );
        assert_eq!(
            datetime(edited.stopped_at.0).time(),
            datetime(original.stopped_at.0).time()
        );
    }

    #[test]
    fn editing_date_preserves_an_overnight_stop_date() {
        let original = TimeRange {
            started_at: UnixSeconds(
                timestamp(
                    NaiveDate::from_ymd_opt(2026, 9, 16)
                        .unwrap()
                        .and_hms_opt(23, 15, 0)
                        .unwrap(),
                )
                .unwrap(),
            ),
            stopped_at: UnixSeconds(
                timestamp(
                    NaiveDate::from_ymd_opt(2026, 9, 17)
                        .unwrap()
                        .and_hms_opt(1, 30, 0)
                        .unwrap(),
                )
                .unwrap(),
            ),
        };

        let date = NaiveDate::from_ymd_opt(2026, 8, 2).unwrap();
        let edited = edit_date(original, date).unwrap();

        assert_eq!(datetime(edited.started_at.0).date_naive(), date);
        assert_eq!(
            datetime(edited.stopped_at.0).date_naive(),
            date.succ_opt().unwrap()
        );
    }
}
