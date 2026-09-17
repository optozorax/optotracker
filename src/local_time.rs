//! Local-calendar policy for the application.
//!
//! Stored timestamps are UTC Unix seconds, while all dates shown to the user
//! and all day boundaries follow the computer's current local timezone.

use std::collections::BTreeMap;

use chrono::{DateTime, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone as _};

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

        assert_eq!(
            split_interval_by_day(start, stop).values().sum::<u64>(),
            (stop - start) as u64
        );
    }
}
