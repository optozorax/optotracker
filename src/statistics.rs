//! The Statistics view: what a period of time went to and the habits behind
//! it. The comparison of projects that the Projects view shows is drawn
//! with the same parts and lives here too.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use chrono::{Datelike as _, Duration, Local, Months, NaiveDate};
use eframe::egui::{
    self, Align, Align2, Button, Color32, CornerRadius, FontId, Layout, Rangef, Rect, RichText,
    ScrollArea, Sense, Stroke, StrokeKind,
};

use crate::date_picker::DatePickerButton;
use crate::local_time;
use crate::model::{ProjectStatus, Store};
use crate::reports::{
    activity_totals, buckets, busiest_day, current_streak, days_in, hour_totals, longest_streak,
    month_end, month_start, project_mixes, range_totals, session_lengths, totals_by_local_day,
    week_start, weekday_totals, worked_days, Bucket, BucketSize, DayTotals, ProjectMix,
};
use crate::{
    card, color, format_hours_minutes, format_share, frameless_until_hovered, paint_share_bar,
    right_aligned, row, section_title, semibold_font, swatch_legend, Swatch, BORDER, CAPTION,
    HAIRLINE, HOVER, NO_PROJECT_COLOR, ROW_HEIGHT, SHARE_BAR_SIZE, SHARE_WIDTH, TEXT,
};

const MEMORY_ID: &str = "opto_tracker_statistics_v1";
/// The green of the calendar, for bars that stand for time as such.
const TIME_COLOR: Color32 = Color32::from_rgb(48, 161, 78);
const CHART_HEIGHT: f32 = 140.0;
const SMALL_CHART_HEIGHT: f32 = 84.0;
/// Room for the hours to the left of a chart and for the labels below it.
const AXIS_WIDTH: f32 = 34.0;
const AXIS_HEIGHT: f32 = 18.0;
const WIDEST_BAR: f32 = 24.0;
/// A list shows this many rows until it is asked for all of them.
const LIST_ROWS: usize = 8;
const TOOLTIP_PARTS: usize = 8;
const TOOLTIP_WIDTH: f32 = 230.0;
const LIST_ROW_HEIGHT: f32 = 20.0;
const TIME_WIDTH: f32 = 70.0;
/// The times of the project comparison are set in a wider font.
const MIX_TIME_WIDTH: f32 = 86.0;
const MIX_BAR_HEIGHT: f32 = 12.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
enum Span {
    Week,
    #[default]
    Month,
    Year,
    All,
    Custom,
}

/// What the bars of the time chart are divided into.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
enum Split {
    #[default]
    Activities,
    Projects,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct Remembered {
    span: Span,
    split: Split,
    /// The ends of the custom range as days of the common era.
    custom: Option<(i32, i32)>,
}

pub struct StatisticsView {
    span: Span,
    /// How many whole weeks, months or years the shown one lies back.
    steps_back: u32,
    custom: Option<(NaiveDate, NaiveDate)>,
    split: Split,
    /// Whether the lists of activities and of projects show every row.
    full_lists: [bool; 2],
}

/// What every part of the view reads.
struct Data<'a> {
    store: &'a Store,
    now: i64,
    today: NaiveDate,
    /// The first day with tracked time.
    first_day: NaiveDate,
    day_totals: &'a BTreeMap<NaiveDate, DayTotals>,
    /// The place of each activity in a stack of them.
    activity_ranks: Vec<usize>,
}

/// A colored part of a total.
struct Part<'a> {
    swatch: Swatch,
    name: &'a str,
    seconds: u64,
    /// Its place in a stack of parts.
    rank: usize,
}

impl Data<'_> {
    /// The parts of some totals, largest first.
    fn parts(&self, split: Split, totals: &DayTotals) -> Vec<Part<'_>> {
        match split {
            Split::Activities => {
                activity_parts(self.store, &self.activity_ranks, &totals.by_activity)
            }
            Split::Projects => {
                let mut parts: Vec<Part<'_>> = totals
                    .by_project
                    .iter()
                    .filter(|(_, &seconds)| seconds > 0)
                    .map(|(&project, &seconds)| {
                        match project.and_then(|index| self.store.projects.get(index)) {
                            Some(found) => Part {
                                swatch: Swatch::Project(color(found.color)),
                                name: &found.name,
                                seconds,
                                rank: project.unwrap_or(usize::MAX),
                            },
                            None => Part {
                                swatch: Swatch::Project(NO_PROJECT_COLOR),
                                name: "No project",
                                seconds,
                                rank: usize::MAX,
                            },
                        }
                    })
                    .collect();
                parts.sort_by_key(|part| std::cmp::Reverse(part.seconds));
                parts
            }
        }
    }

    /// The days of a range that have already come, since tracking began.
    fn elapsed(&self, range: &RangeInclusive<NaiveDate>) -> RangeInclusive<NaiveDate> {
        (*range.start()).max(self.first_day)..=(*range.end()).min(self.today)
    }
}

/// The activities that some totals are made of, largest first.
fn activity_parts<'a>(
    store: &'a Store,
    ranks: &[usize],
    totals: &BTreeMap<usize, u64>,
) -> Vec<Part<'a>> {
    let mut parts: Vec<Part<'a>> = totals
        .iter()
        .filter(|(_, &seconds)| seconds > 0)
        .filter_map(|(&index, &seconds)| {
            let activity = store.activities.get(index)?;
            Some(Part {
                swatch: Swatch::Activity(color(activity.color)),
                name: &activity.name,
                seconds,
                rank: ranks[index],
            })
        })
        .collect();
    parts.sort_by_key(|part| std::cmp::Reverse(part.seconds));
    parts
}

impl StatisticsView {
    pub fn load(ctx: &egui::Context) -> Self {
        let remembered = ctx
            .data_mut(|data| data.get_persisted::<Remembered>(egui::Id::new(MEMORY_ID)))
            .unwrap_or_default();
        let date = NaiveDate::from_num_days_from_ce_opt;
        Self {
            span: remembered.span,
            steps_back: 0,
            custom: remembered
                .custom
                .and_then(|(from, to)| Some((date(from)?, date(to)?))),
            split: remembered.split,
            full_lists: [false; 2],
        }
    }

    fn remember(&self, ctx: &egui::Context) {
        let id = egui::Id::new(MEMORY_ID);
        let remembered = Remembered {
            span: self.span,
            split: self.split,
            custom: self
                .custom
                .map(|(from, to)| (from.num_days_from_ce(), to.num_days_from_ce())),
        };
        ctx.data_mut(|data| {
            if data.get_persisted::<Remembered>(id) != Some(remembered) {
                data.insert_persisted(id, remembered);
            }
        });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, store: &Store, now: i64) {
        let today = Local::now().date_naive();
        let day_totals = totals_by_local_day(store, now);
        let Some(first_day) = day_totals.keys().next().map(|&first| first.min(today)) else {
            card(ui, |ui| {
                ui.label(RichText::new("Nothing has been tracked yet.").weak());
            });
            return;
        };
        let data = Data {
            store,
            now,
            today,
            first_day,
            day_totals: &day_totals,
            activity_ranks: activity_ranks(store, &activity_totals(store, now)),
        };
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .scroll_bar_visibility(
                egui::containers::scroll_area::ScrollBarVisibility::AlwaysVisible,
            )
            .show(ui, |ui| {
                self.period_controls(ui, today, first_day);
                let range = self.range(today, first_day);
                let totals = range_totals(&day_totals, &range);
                ui.add_space(8.0);
                card(ui, |ui| tiles(ui, &self.summary(&data, &range, &totals)));
                ui.add_space(8.0);
                if let Some((span, day)) = self.time_chart(ui, &data, &range, &totals) {
                    self.show_period(span, day, today);
                }
                if totals.seconds > 0 {
                    ui.add_space(8.0);
                    self.lists(ui, &data, &totals);
                    ui.add_space(8.0);
                    habits(ui, &data, &range);
                }
            });
        self.remember(ui.ctx());
    }

    /// The days that the period covers.
    fn range(&self, today: NaiveDate, first_day: NaiveDate) -> RangeInclusive<NaiveDate> {
        match self.span {
            Span::Week => {
                let first = week_start(today) - Duration::weeks(i64::from(self.steps_back));
                first..=first + Duration::days(6)
            }
            Span::Month => {
                let this_month = month_start(today);
                let first = this_month
                    .checked_sub_months(Months::new(self.steps_back))
                    .unwrap_or(this_month);
                first..=month_end(first)
            }
            Span::Year => {
                let year = today.year() - self.steps_back as i32;
                let day = |month, day| NaiveDate::from_ymd_opt(year, month, day).unwrap_or(today);
                day(1, 1)..=day(12, 31)
            }
            Span::All => first_day..=today,
            Span::Custom => {
                let (from, to) = self.custom.unwrap_or((month_start(today), today));
                from..=to.max(from)
            }
        }
    }

    /// The days to compare a period with: the week, month or year before it,
    /// or as many days before a custom range. A period that is still going
    /// on is compared with as much of the one before as has passed of it.
    fn earlier_range(
        &self,
        range: &RangeInclusive<NaiveDate>,
        today: NaiveDate,
    ) -> Option<RangeInclusive<NaiveDate>> {
        let start = *range.start();
        let passed = (*range.end()).min(today) - start;
        if passed < Duration::zero() {
            return None;
        }
        let first = match self.span {
            Span::Week => start - Duration::weeks(1),
            Span::Month => start.checked_sub_months(Months::new(1))?,
            Span::Year => start.with_year(start.year() - 1)?,
            Span::All => return None,
            Span::Custom => start - passed - Duration::days(1),
        };
        Some(first..=(first + passed).min(start.pred_opt()?))
    }

    /// Switches to the week or month that holds a day.
    fn show_period(&mut self, span: Span, day: NaiveDate, today: NaiveDate) {
        let steps_back = match span {
            Span::Week => (week_start(today) - week_start(day)).num_weeks(),
            Span::Month => {
                i64::from(today.year() - day.year()) * 12 + i64::from(today.month())
                    - i64::from(day.month())
            }
            Span::Year | Span::All | Span::Custom => 0,
        };
        self.span = span;
        self.steps_back = steps_back.max(0) as u32;
    }

    fn period_controls(&mut self, ui: &mut egui::Ui, today: NaiveDate, first_day: NaiveDate) {
        let spans = [
            (Span::Week, "Week"),
            (Span::Month, "Month"),
            (Span::Year, "Year"),
            (Span::All, "All time"),
            (Span::Custom, "Custom"),
        ];
        let shown = self.range(today, first_day);
        let before = self.span;
        if ui.available_width() >= 560.0 {
            row(ui, ROW_HEIGHT, |ui| {
                self.period_stepper(ui, today, first_day);
                right_aligned(ui, |ui| {
                    frameless_until_hovered(ui, |ui| {
                        for (span, name) in spans.into_iter().rev() {
                            ui.selectable_value(&mut self.span, span, name);
                        }
                    });
                });
            });
        } else {
            row(ui, ROW_HEIGHT, |ui| {
                frameless_until_hovered(ui, |ui| {
                    for (span, name) in spans {
                        ui.selectable_value(&mut self.span, span, name);
                    }
                });
            });
            row(ui, ROW_HEIGHT, |ui| {
                self.period_stepper(ui, today, first_day)
            });
        }
        if self.span != before {
            self.steps_back = 0;
            if self.span == Span::Custom {
                // A custom range starts out as the days that were on screen.
                self.custom = Some((*shown.start(), (*shown.end()).min(today)));
            }
        }
    }

    /// The name of the period, with the buttons that move it.
    fn period_stepper(&mut self, ui: &mut egui::Ui, today: NaiveDate, first_day: NaiveDate) {
        let range = self.range(today, first_day);
        let title = |ui: &mut egui::Ui, text: &str| section_title(ui, text);
        let detail = |ui: &mut egui::Ui, text: String| {
            ui.label(RichText::new(text).weak());
        };
        match self.span {
            Span::Week | Span::Month | Span::Year => {
                let earlier = ui.add_enabled(*range.start() > first_day, Button::new("⏴"));
                if earlier.on_hover_text("Earlier").clicked() {
                    self.steps_back += 1;
                }
                let later = ui.add_enabled(self.steps_back > 0, Button::new("⏵"));
                if later.on_hover_text("Later").clicked() {
                    self.steps_back -= 1;
                }
                ui.add_space(4.0);
                match (self.span, self.steps_back) {
                    (Span::Week, 0) => {
                        title(ui, "This week");
                        detail(ui, format_days(&range));
                    }
                    (Span::Week, 1) => {
                        title(ui, "Last week");
                        detail(ui, format_days(&range));
                    }
                    (Span::Week, _) => title(ui, &format_days(&range)),
                    (Span::Month, _) => title(ui, &range.start().format("%B %Y").to_string()),
                    _ => title(ui, &range.start().year().to_string()),
                }
            }
            Span::All => {
                title(ui, "All time");
                detail(ui, format_days(&range));
            }
            Span::Custom => {
                let (mut from, mut to) = (*range.start(), *range.end());
                let years = first_day.year()..=today.year();
                ui.label(local_time::format_date(from));
                let from_picker =
                    DatePickerButton::new(&mut from, "statistics-from").year_range(years.clone());
                let from_changed = ui.add(from_picker).changed();
                ui.label("–");
                ui.label(local_time::format_date(to));
                let to_picker = DatePickerButton::new(&mut to, "statistics-to").year_range(years);
                let to_changed = ui.add(to_picker).changed();
                // Whichever end was moved past the other takes it along.
                if from_changed && from > to {
                    to = from;
                }
                if to_changed && to < from {
                    from = to;
                }
                self.custom = Some((from, to));
            }
        }
    }

    /// The numbers that sum the period up.
    fn summary(
        &self,
        data: &Data<'_>,
        range: &RangeInclusive<NaiveDate>,
        totals: &DayTotals,
    ) -> Vec<Tile> {
        let worked = worked_days(data.day_totals, range) as u64;
        let days = u64::from(days_in(&data.elapsed(range)));
        let per_worked_day = totals.seconds.checked_div(worked).unwrap_or(0);
        let per_day = totals.seconds.checked_div(days).unwrap_or(0);
        let before = match self.span {
            Span::Week => "the week before",
            Span::Month => "the month before",
            Span::Year => "the year before",
            Span::All | Span::Custom => "the days before",
        };
        let (change, change_hint) = self
            .earlier_range(range, data.today)
            .map(|earlier| {
                let earlier_seconds = range_totals(data.day_totals, &earlier).seconds;
                let change = match (totals.seconds, earlier_seconds) {
                    (0, 0) => String::new(),
                    (_, 0) => format!("nothing {before}"),
                    (seconds, earlier) => {
                        let percent = (seconds as f64 / earlier as f64 - 1.0) * 100.0;
                        match percent.round() as i64 {
                            0 => format!("same as {before}"),
                            percent if percent > 0 => format!("+{percent}% vs {before}"),
                            percent => format!("−{}% vs {before}", -percent),
                        }
                    }
                };
                let hint = format!(
                    "{} in {}",
                    format_hours_minutes(earlier_seconds),
                    format_days(&earlier)
                );
                (change, hint)
            })
            .unwrap_or_default();
        let busiest = busiest_day(data.day_totals, range);
        vec![
            Tile {
                caption: "Tracked",
                value: format_hours_minutes(totals.seconds),
                note: change,
                hint: change_hint,
            },
            Tile {
                caption: "Days worked",
                value: worked.to_string(),
                note: format!("of {days} {}", if days == 1 { "day" } else { "days" }),
                hint: String::new(),
            },
            Tile {
                caption: "Per worked day",
                value: format_hours_minutes(per_worked_day),
                note: format!("{} over all days", format_hours_minutes(per_day)),
                hint: String::new(),
            },
            Tile {
                caption: "Busiest day",
                value: busiest
                    .map(|(_, seconds)| format_hours_minutes(seconds))
                    .unwrap_or_else(|| "–".to_owned()),
                note: busiest
                    .map(|(date, _)| date.format("%a, %-d %b %Y").to_string())
                    .unwrap_or_default(),
                hint: String::new(),
            },
        ]
    }

    /// The time of the period as one bar per day, week or month. Returns the
    /// week or month whose bar was clicked.
    fn time_chart(
        &mut self,
        ui: &mut egui::Ui,
        data: &Data<'_>,
        range: &RangeInclusive<NaiveDate>,
        totals: &DayTotals,
    ) -> Option<(Span, NaiveDate)> {
        let size = bucket_size(range);
        let buckets = buckets(data.day_totals, range, size);
        card(ui, |ui| {
            row(ui, ROW_HEIGHT, |ui| {
                section_title(
                    ui,
                    match size {
                        BucketSize::Day => "Time per day",
                        BucketSize::Week => "Time per week",
                        BucketSize::Month => "Time per month",
                    },
                );
                right_aligned(ui, |ui| {
                    frameless_until_hovered(ui, |ui| {
                        ui.selectable_value(&mut self.split, Split::Projects, "Projects");
                        ui.selectable_value(&mut self.split, Split::Activities, "Activities");
                    });
                });
            });
            let split = self.split;
            let bars: Vec<Bar> = buckets
                .iter()
                .enumerate()
                .map(|(index, bucket)| Bar {
                    segments: stack(data.parts(split, &bucket.totals)),
                    label: bucket_label(size, &buckets, index),
                })
                .collect();
            let drill_down = match size {
                BucketSize::Day => None,
                BucketSize::Week => Some(Span::Week),
                BucketSize::Month => Some(Span::Month),
            };
            let clicked = bar_chart(
                ui,
                "time",
                &bars,
                CHART_HEIGHT,
                drill_down.is_some(),
                |ui, index| {
                    let bucket = &buckets[index];
                    let parts = data.parts(split, &bucket.totals);
                    parts_tooltip(
                        ui,
                        &bucket_title(size, bucket),
                        bucket.totals.seconds,
                        &parts,
                    );
                },
            );
            ui.add_space(2.0);
            swatch_legend(ui, &legend(&data.parts(split, totals)));
            drill_down.zip(clicked.map(|index| buckets[index].first))
        })
    }

    /// What the period went to: its activities and its projects.
    fn lists(&mut self, ui: &mut egui::Ui, data: &Data<'_>, totals: &DayTotals) {
        let activities = data.parts(Split::Activities, totals);
        let projects = data.parts(Split::Projects, totals);
        let [all_activities, all_projects] = &mut self.full_lists;
        if ui.available_width() >= 760.0 {
            ui.columns(2, |columns| {
                parts_list(&mut columns[0], "Activities", &activities, all_activities);
                parts_list(&mut columns[1], "Projects", &projects, all_projects);
            });
        } else {
            parts_list(ui, "Activities", &activities, all_activities);
            ui.add_space(8.0);
            parts_list(ui, "Projects", &projects, all_projects);
        }
    }
}

/// The place of each activity in a stack of them. Activities of a similar
/// color are neighbors, so that the shades of one color read as one block,
/// and the color of the most used activity comes first.
fn activity_ranks(store: &Store, totals: &[u64]) -> Vec<usize> {
    let hues: Vec<Option<f32>> = store
        .activities
        .iter()
        .map(|activity| hue(color(activity.color)))
        .collect();
    let first_hue = totals
        .iter()
        .zip(&hues)
        .filter_map(|(&seconds, &hue)| Some((seconds, hue?)))
        .max_by_key(|&(seconds, _)| seconds)
        // A little before it, for the shades on either side of it.
        .map_or(0.0, |(_, hue)| hue - 0.02);
    // Grays have no place on the color wheel and follow everything else.
    let position = |index: usize| hues[index].map_or(2.0, |hue| (hue - first_hue).rem_euclid(1.0));
    let mut order: Vec<usize> = (0..hues.len()).collect();
    order.sort_by(|&left, &right| {
        position(left)
            .total_cmp(&position(right))
            .then(left.cmp(&right))
    });
    let mut ranks = vec![0; hues.len()];
    for (rank, index) in order.into_iter().enumerate() {
        ranks[index] = rank;
    }
    ranks
}

/// Where a color lies on the color wheel, as a part of the way around it.
/// A gray lies nowhere on it.
fn hue(color: Color32) -> Option<f32> {
    let [red, green, blue] = [color.r(), color.g(), color.b()].map(f32::from);
    let strongest = red.max(green).max(blue);
    let spread = strongest - red.min(green).min(blue);
    if spread < 38.0 {
        return None;
    }
    let sixth = if strongest == red {
        ((green - blue) / spread).rem_euclid(6.0)
    } else if strongest == green {
        (blue - red) / spread + 2.0
    } else {
        (red - green) / spread + 4.0
    };
    Some(sixth / 6.0)
}

/// The colors of some parts in the order they are stacked in.
fn stack(mut parts: Vec<Part<'_>>) -> Vec<(Color32, u64)> {
    parts.sort_by_key(|part| part.rank);
    parts
        .iter()
        .map(|part| (part.swatch.color(), part.seconds))
        .collect()
}

fn legend(parts: &[Part<'_>]) -> Vec<(Swatch, String)> {
    parts
        .iter()
        .map(|part| (part.swatch, part.name.to_owned()))
        .collect()
}

/// Bars for days suit a month and a half, bars for weeks a year.
fn bucket_size(range: &RangeInclusive<NaiveDate>) -> BucketSize {
    match days_in(range) {
        0..=45 => BucketSize::Day,
        46..=400 => BucketSize::Week,
        _ => BucketSize::Month,
    }
}

/// What the axis says below a bar. Most bars go without: a few days are
/// labeled in full, longer runs at every fifth day and where a month or a
/// year begins.
fn bucket_label(size: BucketSize, buckets: &[Bucket], index: usize) -> Option<AxisLabel> {
    let day = buckets[index].first;
    let previous = index.checked_sub(1).map(|index| buckets[index].first);
    let new_year = previous.is_some_and(|previous| previous.year() != day.year());
    let new_month = previous.is_none_or(|previous| previous.month() != day.month());
    let plain = |text: String| {
        Some(AxisLabel {
            text,
            strong: false,
        })
    };
    let strong = |text: String| Some(AxisLabel { text, strong: true });
    if new_year {
        return strong(day.year().to_string());
    }
    match size {
        BucketSize::Day if buckets.len() <= 7 => plain(day.format("%a").to_string()),
        BucketSize::Day if new_month && previous.is_some() => strong(day.format("%b").to_string()),
        BucketSize::Day if buckets.len() <= 16 || day.day() == 1 || day.day().is_multiple_of(5) => {
            plain(day.day().to_string())
        }
        BucketSize::Week | BucketSize::Month if new_month => plain(day.format("%b").to_string()),
        _ => None,
    }
}

fn bucket_title(size: BucketSize, bucket: &Bucket) -> String {
    let whole_month =
        bucket.first == month_start(bucket.first) && bucket.last == month_end(bucket.first);
    match size {
        BucketSize::Day => local_time::format_date_with_weekday(bucket.first),
        BucketSize::Month if whole_month => bucket.first.format("%B %Y").to_string(),
        BucketSize::Week | BucketSize::Month => format_days(&(bucket.first..=bucket.last)),
    }
}

/// A range of days without repeating the month or year that its ends share.
fn format_days(range: &RangeInclusive<NaiveDate>) -> String {
    let (first, last) = (*range.start(), *range.end());
    let last_text = local_time::format_date(last);
    if first == last {
        last_text
    } else if (first.year(), first.month()) == (last.year(), last.month()) {
        format!("{} – {last_text}", first.day())
    } else if first.year() == last.year() {
        format!("{} – {last_text}", first.format("%-d %b"))
    } else {
        format!("{} – {last_text}", local_time::format_date(first))
    }
}

fn format_day_count(days: u32) -> String {
    format!("{days} {}", if days == 1 { "day" } else { "days" })
}

/// A number with a caption above it and a remark below it.
#[derive(Default)]
struct Tile {
    caption: &'static str,
    value: String,
    note: String,
    /// Shown when the pointer rests on the tile.
    hint: String,
}

/// Lays tiles out in a row, or in two rows of a narrow window.
fn tiles(ui: &mut egui::Ui, tiles: &[Tile]) {
    let per_row = if ui.available_width() >= 560.0 { 4 } else { 2 };
    let caption_font = FontId::proportional(11.0);
    let value_font = semibold_font(18.0);
    let (caption_height, value_height) = ui.fonts_mut(|fonts| {
        (
            fonts.row_height(&caption_font),
            fonts.row_height(&value_font),
        )
    });
    let height = 2.0 * caption_height + value_height + 2.0;
    for (index, tiles) in tiles.chunks(per_row).enumerate() {
        if index > 0 {
            ui.add_space(8.0);
        }
        ui.columns(per_row, |columns| {
            for (tile, ui) in tiles.iter().zip(columns) {
                let size = egui::vec2(ui.available_width(), height);
                let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
                let line = |top: f32, text: &str, font: &FontId, color: Color32| {
                    let left_top = egui::pos2(rect.left(), rect.top() + top);
                    paint_line(
                        ui,
                        left_top,
                        Align2::LEFT_TOP,
                        text,
                        font,
                        color,
                        rect.width(),
                    );
                };
                line(0.0, tile.caption, &caption_font, CAPTION);
                line(caption_height + 1.0, &tile.value, &value_font, TEXT);
                let note_top = caption_height + value_height + 2.0;
                line(note_top, &tile.note, &caption_font, CAPTION);
                if !tile.hint.is_empty() {
                    response.on_hover_text(&tile.hint);
                }
            }
        });
    }
}

/// Paints one line of text that is cut off at `width`, and returns whether
/// it had to be.
fn paint_line(
    ui: &egui::Ui,
    anchor_position: egui::Pos2,
    anchor: Align2,
    text: &str,
    font: &FontId,
    color: Color32,
    width: f32,
) -> bool {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font.clone(), color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(0.0));
    let galley = ui.painter().layout_job(job);
    let elided = galley.elided;
    let rect = anchor.anchor_size(anchor_position, galley.size());
    ui.painter().galley(rect.min, galley, color);
    elided
}

struct AxisLabel {
    text: String,
    /// A strong label is darker and keeps its place when labels collide.
    strong: bool,
}

struct Bar {
    /// Colors and seconds, from the bottom up.
    segments: Vec<(Color32, u64)>,
    label: Option<AxisLabel>,
}

impl Bar {
    fn seconds(&self) -> u64 {
        self.segments.iter().map(|&(_, seconds)| seconds).sum()
    }
}

/// How far apart the lines of a time axis are that reaches `largest`
/// seconds: a round number of minutes or hours that needs at most four.
fn axis_step(largest: u64) -> u64 {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const STEPS: [u64; 16] = [
        5 * MINUTE,
        15 * MINUTE,
        30 * MINUTE,
        HOUR,
        2 * HOUR,
        4 * HOUR,
        5 * HOUR,
        10 * HOUR,
        20 * HOUR,
        25 * HOUR,
        50 * HOUR,
        100 * HOUR,
        200 * HOUR,
        250 * HOUR,
        500 * HOUR,
        1000 * HOUR,
    ];
    STEPS
        .into_iter()
        .find(|step| largest <= step * 4)
        .unwrap_or_else(|| largest.div_ceil(4 * 1000 * HOUR) * 1000 * HOUR)
}

fn axis_text(seconds: u64) -> String {
    if seconds == 0 {
        "0".to_owned()
    } else if seconds.is_multiple_of(3600) {
        format!("{}h", seconds / 3600)
    } else {
        format!("{}m", seconds / 60)
    }
}

/// Bars over lines that mark the time. The bar under the pointer shows the
/// tooltip of its index. Returns the bar that was clicked.
fn bar_chart(
    ui: &mut egui::Ui,
    id_salt: &str,
    bars: &[Bar],
    height: f32,
    clickable: bool,
    tooltip: impl Fn(&mut egui::Ui, usize),
) -> Option<usize> {
    let font = FontId::proportional(11.0);
    let size = egui::vec2(ui.available_width(), height + AXIS_HEIGHT);
    let (outer, _) = ui.allocate_exact_size(size, Sense::hover());
    let plot = Rect::from_min_max(
        egui::pos2(outer.left() + AXIS_WIDTH, outer.top() + 7.0),
        egui::pos2(outer.right(), outer.top() + height),
    );

    let largest = bars.iter().map(Bar::seconds).max().unwrap_or(0);
    let step = axis_step(largest);
    let lines = largest.div_ceil(step).max(1);
    let top_seconds = (lines * step) as f32;
    for line in 0..=lines {
        let y = plot.bottom() - plot.height() * line as f32 / lines as f32;
        // The line that the bars stand on is the only one that stands out.
        let line_color = if line == 0 { BORDER } else { HAIRLINE };
        ui.painter()
            .hline(plot.x_range(), y, Stroke::new(1.0, line_color));
        ui.painter().text(
            egui::pos2(plot.left() - 6.0, y),
            Align2::RIGHT_CENTER,
            axis_text(line * step),
            font.clone(),
            CAPTION,
        );
    }
    if bars.is_empty() {
        return None;
    }

    let slot = plot.width() / bars.len() as f32;
    let width = (slot * 0.7).clamp(1.0, WIDEST_BAR);
    let id = ui.id().with(id_salt);
    let pixels_per_point = ui.pixels_per_point();
    let to_pixel = |value: f32| (value * pixels_per_point).round() / pixels_per_point;
    let mut clicked = None;
    for (index, bar) in bars.iter().enumerate() {
        let center = plot.left() + slot * (index as f32 + 0.5);
        let slot_rect = Rect::from_x_y_ranges(
            Rangef::new(center - slot * 0.5, center + slot * 0.5),
            plot.y_range(),
        );
        let sense = if clickable {
            Sense::click()
        } else {
            Sense::hover()
        };
        let response = ui.interact(slot_rect, id.with(index), sense);
        if response.hovered() {
            ui.painter()
                .rect_filled(slot_rect.shrink2(egui::vec2(0.0, 0.5)), 2.0, HOVER);
        }

        let x_range = Rangef::new(
            to_pixel(center - width * 0.5),
            to_pixel(center + width * 0.5),
        );
        let mut bottom = plot.bottom();
        let last = bar.segments.len().saturating_sub(1);
        for (position, &(segment_color, seconds)) in bar.segments.iter().enumerate() {
            if seconds == 0 {
                continue;
            }
            // A sliver of time stays visible.
            let segment_height = (plot.height() * seconds as f32 / top_seconds).max(1.0);
            let top = (bottom - segment_height).max(plot.top());
            let segment = Rect::from_x_y_ranges(x_range, to_pixel(top)..=to_pixel(bottom));
            let radius = if position == last && width >= 5.0 {
                2
            } else {
                0
            };
            let corners = CornerRadius {
                nw: radius,
                ne: radius,
                sw: 0,
                se: 0,
            };
            ui.painter().rect_filled(segment, corners, segment_color);
            // A white line keeps neighboring colors apart.
            if position > 0 && segment.height() >= 3.0 {
                ui.painter()
                    .hline(x_range, segment.bottom(), Stroke::new(1.0, Color32::WHITE));
            }
            bottom = top;
        }

        if response.clicked() {
            clicked = Some(index);
        }
        let response = if clickable {
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            response
        };
        response.on_hover_ui_at_pointer(|ui| tooltip(ui, index));
    }

    // The strong labels are placed first, the others where they leave room.
    let mut taken: Vec<Rangef> = Vec::new();
    for strong in [true, false] {
        for (index, bar) in bars.iter().enumerate() {
            let Some(label) = bar.label.as_ref().filter(|label| label.strong == strong) else {
                continue;
            };
            let label_color = if strong { TEXT } else { CAPTION };
            let galley = ui
                .painter()
                .layout_no_wrap(label.text.clone(), font.clone(), label_color);
            let center = plot.left() + slot * (index as f32 + 0.5);
            let rightmost = (outer.right() - galley.size().x).max(plot.left());
            let left = (center - galley.size().x * 0.5).clamp(plot.left(), rightmost);
            let span = Rangef::new(left - 3.0, left + galley.size().x + 3.0);
            if taken.iter().any(|other| other.intersects(span)) {
                continue;
            }
            taken.push(span);
            ui.painter()
                .galley(egui::pos2(left, plot.bottom() + 3.0), galley, label_color);
        }
    }
    clicked
}

/// A heading, a time and the largest parts that the time is made of.
fn parts_tooltip(ui: &mut egui::Ui, heading: &str, seconds: u64, parts: &[Part<'_>]) {
    // The times line up at the right edge, which needs a fixed width.
    ui.set_width(TOOLTIP_WIDTH);
    ui.label(heading);
    if seconds == 0 {
        ui.label("No time tracked");
        return;
    }
    ui.label(format_hours_minutes(seconds));
    if parts.is_empty() {
        return;
    }
    ui.separator();
    for part in parts.iter().take(TOOLTIP_PARTS) {
        ui.horizontal(|ui| {
            // The time is placed first, so that a long name is cut short of it.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(format_hours_minutes(part.seconds));
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let size = egui::Vec2::splat(Swatch::SIZE);
                    let (swatch, _) = ui.allocate_exact_size(size, Sense::hover());
                    part.swatch.paint(ui.painter(), swatch);
                    ui.add(egui::Label::new(part.name).truncate());
                });
            });
        });
    }
    if parts.len() > TOOLTIP_PARTS {
        let more = format!("+{} more", parts.len() - TOOLTIP_PARTS);
        ui.label(RichText::new(more).weak());
    }
}

/// A card that lists parts with their time and their share of the whole.
fn parts_list(ui: &mut egui::Ui, title: &str, parts: &[Part<'_>], show_all: &mut bool) {
    card(ui, |ui| {
        section_title(ui, title);
        ui.add_space(2.0);
        let whole: u64 = parts.iter().map(|part| part.seconds).sum();
        let largest = parts.first().map_or(0, |part| part.seconds);
        let shown = if *show_all { parts.len() } else { LIST_ROWS };
        ui.spacing_mut().item_spacing.y = 0.0;
        for part in parts.iter().take(shown) {
            part_row(ui, part, whole, largest);
        }
        if parts.len() > LIST_ROWS {
            ui.add_space(4.0);
            let text = if *show_all {
                "Show fewer".to_owned()
            } else {
                format!("Show all {}", parts.len())
            };
            if ui.link(text).clicked() {
                *show_all = !*show_all;
            }
        }
    });
}

/// From the right: the share, the time and a bar, with the name in the
/// room that is left.
fn part_row(ui: &mut egui::Ui, part: &Part<'_>, whole: u64, largest: u64) {
    let size = egui::vec2(ui.available_width(), LIST_ROW_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let font = FontId::proportional(14.0);
    let middle = rect.center().y;
    let share = format_share(part.seconds, whole);
    let share_at = egui::pos2(rect.right(), middle);
    let no_limit = f32::INFINITY;
    paint_line(
        ui,
        share_at,
        Align2::RIGHT_CENTER,
        &share,
        &font,
        CAPTION,
        no_limit,
    );
    let time = format_hours_minutes(part.seconds);
    let time_at = egui::pos2(rect.right() - SHARE_WIDTH - 8.0, middle);
    paint_line(
        ui,
        time_at,
        Align2::RIGHT_CENTER,
        &time,
        &font,
        TEXT,
        no_limit,
    );
    let mut name_right = time_at.x - TIME_WIDTH - 8.0;
    // A narrow list does without the bars.
    if rect.width() >= 300.0 {
        let bar_center = egui::pos2(name_right - SHARE_BAR_SIZE.x * 0.5, middle);
        let track = Rect::from_center_size(bar_center, SHARE_BAR_SIZE);
        let share = part.seconds as f32 / largest.max(1) as f32;
        paint_share_bar(ui.painter(), track, share, part.swatch.color());
        name_right = track.left() - 10.0;
    }
    let swatch = Rect::from_center_size(
        egui::pos2(rect.left() + Swatch::SIZE * 0.5, middle),
        egui::Vec2::splat(Swatch::SIZE),
    );
    part.swatch.paint(ui.painter(), swatch);
    let name_at = egui::pos2(swatch.right() + 6.0, middle);
    let room = name_right - name_at.x;
    if paint_line(
        ui,
        name_at,
        Align2::LEFT_CENTER,
        part.name,
        &font,
        TEXT,
        room,
    ) {
        response.on_hover_text(part.name);
    }
}

/// When the work happens: the weekdays and hours it falls on, how long it
/// goes on without a break and how many days in a row.
fn habits(ui: &mut egui::Ui, data: &Data<'_>, range: &RangeInclusive<NaiveDate>) {
    let weekdays = weekday_totals(data.day_totals, &data.elapsed(range));
    let hours = hour_totals(data.store, data.now, range);
    let all_hours: u64 = hours.iter().sum();
    let sessions = session_lengths(data.store, data.now, range);
    let streak = longest_streak(data.day_totals, range);
    let caption = |ui: &mut egui::Ui, text: &str| {
        ui.label(
            RichText::new(text)
                .font(FontId::proportional(11.0))
                .color(CAPTION),
        );
    };
    let bar = |seconds: u64, label: Option<String>| Bar {
        segments: vec![(TIME_COLOR, seconds)],
        label: label.map(|text| AxisLabel {
            text,
            strong: false,
        }),
    };

    let weekday_chart = |ui: &mut egui::Ui| {
        caption(ui, "An average day of the week");
        let names = [
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
            "Sunday",
        ];
        let average = |index: usize| {
            let weekday = weekdays[index];
            weekday
                .seconds
                .checked_div(u64::from(weekday.days))
                .unwrap_or(0)
        };
        let bars: Vec<Bar> = (0..7)
            .map(|index| bar(average(index), Some(names[index][..3].to_owned())))
            .collect();
        bar_chart(
            ui,
            "weekdays",
            &bars,
            SMALL_CHART_HEIGHT,
            false,
            |ui, index| {
                let weekday = weekdays[index];
                ui.label(names[index]);
                ui.label(format!(
                    "{} on average",
                    format_hours_minutes(average(index))
                ));
                ui.label(
                    RichText::new(format!(
                        "worked on {} of {}",
                        weekday.worked_days, weekday.days
                    ))
                    .weak(),
                );
            },
        );
    };
    let hour_chart = |ui: &mut egui::Ui| {
        caption(ui, "Time by hour of the day");
        let bars: Vec<Bar> = (0..24)
            .map(|hour| bar(hours[hour], (hour % 3 == 0).then(|| hour.to_string())))
            .collect();
        bar_chart(ui, "hours", &bars, SMALL_CHART_HEIGHT, false, |ui, hour| {
            ui.label(format!("{hour:02}:00 – {:02}:00", (hour + 1) % 24));
            ui.label(format_hours_minutes(hours[hour]));
            let share = format_share(hours[hour], all_hours);
            ui.label(RichText::new(format!("{share} of the time")).weak());
        });
    };

    card(ui, |ui| {
        section_title(ui, "Habits");
        ui.add_space(4.0);
        if ui.available_width() >= 560.0 {
            ui.columns(2, |columns| {
                weekday_chart(&mut columns[0]);
                hour_chart(&mut columns[1]);
            });
        } else {
            weekday_chart(ui);
            ui.add_space(6.0);
            hour_chart(ui);
        }
        ui.add_space(10.0);
        let current = current_streak(data.day_totals, data.today);
        tiles(
            ui,
            &[
                Tile {
                    caption: "Typical session",
                    // The one in the middle: half of them are longer.
                    value: sessions
                        .get(sessions.len() / 2)
                        .map(|&seconds| format_hours_minutes(seconds))
                        .unwrap_or_else(|| "–".to_owned()),
                    note: format!(
                        "of {} {}",
                        sessions.len(),
                        if sessions.len() == 1 {
                            "session"
                        } else {
                            "sessions"
                        }
                    ),
                    hint: "Half of the sessions are longer than this, half are shorter. \
                           Entries less than five minutes apart are one session."
                        .to_owned(),
                },
                Tile {
                    caption: "Longest session",
                    value: sessions
                        .last()
                        .map(|&seconds| format_hours_minutes(seconds))
                        .unwrap_or_else(|| "–".to_owned()),
                    note: "without a break".to_owned(),
                    hint: String::new(),
                },
                Tile {
                    caption: "Longest streak",
                    value: format_day_count(streak.map_or(0, |(_, days)| days)),
                    note: streak
                        .map(|(first, days)| {
                            format_days(&(first..=first + Duration::days(i64::from(days) - 1)))
                        })
                        .unwrap_or_default(),
                    hint: "The most days in a row with tracked time in this period.".to_owned(),
                },
                Tile {
                    caption: "Current streak",
                    value: format_day_count(current),
                    note: "in a row until now".to_owned(),
                    hint: "Days in a row with tracked time, up to today.".to_owned(),
                },
            ],
        );
    });
}

/// One bar per project that shows what its time is made of, under one for
/// all projects together.
pub fn project_comparison(ui: &mut egui::Ui, store: &Store, now: i64) {
    let ranks = activity_ranks(store, &activity_totals(store, now));
    let mixes = project_mixes(store, now);
    let mut all = ProjectMix::default();
    for mix in &mixes {
        all.seconds += mix.seconds;
        for (&activity, &seconds) in &mix.by_activity {
            *all.by_activity.entry(activity).or_default() += seconds;
        }
        all.first_used = all.first_used.into_iter().chain(mix.first_used).min();
        all.last_used = all.last_used.into_iter().chain(mix.last_used).max();
    }
    if all.seconds == 0 {
        return;
    }
    card(ui, |ui| {
        row(ui, ROW_HEIGHT, |ui| {
            section_title(ui, "Projects compared");
            ui.label(RichText::new("by activity").weak());
        });
        ui.spacing_mut().item_spacing.y = 0.0;
        mix_row(ui, store, &ranks, None, "All projects", TEXT, &all);
        for (index, mix) in mixes.iter().enumerate().rev() {
            if mix.seconds == 0 {
                continue;
            }
            let project = &store.projects[index];
            let name_color = match project.status {
                ProjectStatus::Active => TEXT,
                ProjectStatus::Finished => ui.visuals().weak_text_color(),
            };
            let swatch = Swatch::Project(color(project.color));
            mix_row(
                ui,
                store,
                &ranks,
                Some(swatch),
                &project.name,
                name_color,
                mix,
            );
        }
        ui.add_space(8.0);
        swatch_legend(
            ui,
            &legend(&activity_parts(store, &ranks, &all.by_activity)),
        );
    });
}

/// A name with its time and the days it spans, over a bar that is divided
/// like the time is among activities.
fn mix_row(
    ui: &mut egui::Ui,
    store: &Store,
    ranks: &[usize],
    swatch: Option<Swatch>,
    name: &str,
    name_color: Color32,
    mix: &ProjectMix,
) {
    ui.add_space(6.0);
    let width = ui.available_width();
    let (line, line_response) =
        ui.allocate_exact_size(egui::vec2(width, LIST_ROW_HEIGHT), Sense::hover());
    let middle = line.center().y;
    let name_font = semibold_font(14.0);
    let small = FontId::proportional(11.0);
    let no_limit = f32::INFINITY;
    let time = format_hours_minutes(mix.seconds);
    let time_at = egui::pos2(line.right(), middle);
    paint_line(
        ui,
        time_at,
        Align2::RIGHT_CENTER,
        &time,
        &name_font,
        TEXT,
        no_limit,
    );
    let mut name_right = line.right() - MIX_TIME_WIDTH;
    let day = |seconds: i64| local_time::datetime(seconds).date_naive();
    if let (Some(first), Some(last)) = (mix.first_used.map(day), mix.last_used.map(day)) {
        let days = first..=last;
        // A narrow window has room for the dates alone.
        let span = if width >= 480.0 {
            format!(
                "{} · {}",
                format_days(&days),
                format_day_count(days_in(&days))
            )
        } else {
            format_days(&days)
        };
        let span_at = egui::pos2(name_right, middle);
        let room = width * 0.55;
        paint_line(
            ui,
            span_at,
            Align2::RIGHT_CENTER,
            &span,
            &small,
            CAPTION,
            room,
        );
        name_right -= room.min(span.chars().count() as f32 * 6.0) + 8.0;
    }
    let mut name_left = line.left();
    if let Some(swatch) = swatch {
        let rect = Rect::from_center_size(
            egui::pos2(line.left() + Swatch::SIZE * 0.5, middle),
            egui::Vec2::splat(Swatch::SIZE),
        );
        swatch.paint(ui.painter(), rect);
        name_left = rect.right() + 6.0;
    }
    let name_at = egui::pos2(name_left, middle);
    let room = name_right - name_left;
    if paint_line(
        ui,
        name_at,
        Align2::LEFT_CENTER,
        name,
        &name_font,
        name_color,
        room,
    ) {
        line_response.on_hover_text(name);
    }

    let (bar, response) = ui.allocate_exact_size(egui::vec2(width, MIX_BAR_HEIGHT), Sense::hover());
    let mut parts = activity_parts(store, ranks, &mix.by_activity);
    parts.sort_by_key(|part| part.rank);
    let pointer = response.hover_pos();
    let mut left = bar.left();
    let last = parts.len().saturating_sub(1);
    let mut hovered = None;
    for (position, part) in parts.iter().enumerate() {
        let right = if position == last {
            bar.right()
        } else {
            left + bar.width() * part.seconds as f32 / mix.seconds as f32
        };
        let segment = Rect::from_x_y_ranges(left..=right, bar.y_range());
        let corners = CornerRadius {
            nw: if position == 0 { 3 } else { 0 },
            sw: if position == 0 { 3 } else { 0 },
            ne: if position == last { 3 } else { 0 },
            se: if position == last { 3 } else { 0 },
        };
        ui.painter()
            .rect_filled(segment, corners, part.swatch.color());
        if position > 0 && segment.width() >= 3.0 {
            ui.painter()
                .vline(left, bar.y_range(), Stroke::new(1.0, Color32::WHITE));
        }
        if pointer.is_some_and(|pointer| segment.contains(pointer)) {
            hovered = Some((segment, position));
        }
        left = right;
    }
    if let Some((segment, position)) = hovered {
        ui.painter().rect_stroke(
            segment,
            0.0,
            Stroke::new(1.5, ui.visuals().strong_text_color()),
            StrokeKind::Outside,
        );
        let part = &parts[position];
        response.on_hover_ui_at_pointer(|ui| {
            ui.horizontal(|ui| {
                let (swatch, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(Swatch::SIZE), Sense::hover());
                part.swatch.paint(ui.painter(), swatch);
                ui.label(part.name);
            });
            ui.label(format!(
                "{} · {}",
                format_hours_minutes(part.seconds),
                format_share(part.seconds, mix.seconds)
            ));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    fn view(span: Span, steps_back: u32) -> StatisticsView {
        StatisticsView {
            span,
            steps_back,
            custom: None,
            split: Split::default(),
            full_lists: [false; 2],
        }
    }

    #[test]
    fn periods_are_whole_weeks_months_and_years() {
        // A Friday.
        let today = date(2026, 10, 9);
        let first_day = date(2025, 4, 12);
        let range = |span, steps_back| view(span, steps_back).range(today, first_day);
        assert_eq!(range(Span::Week, 0), date(2026, 10, 5)..=date(2026, 10, 11));
        assert_eq!(range(Span::Week, 2), date(2026, 9, 21)..=date(2026, 9, 27));
        assert_eq!(
            range(Span::Month, 0),
            date(2026, 10, 1)..=date(2026, 10, 31)
        );
        assert_eq!(range(Span::Month, 8), date(2026, 2, 1)..=date(2026, 2, 28));
        assert_eq!(range(Span::Year, 1), date(2025, 1, 1)..=date(2025, 12, 31));
        assert_eq!(range(Span::All, 0), first_day..=today);
        assert_eq!(range(Span::Custom, 0), date(2026, 10, 1)..=today);
    }

    #[test]
    fn a_running_period_is_compared_with_as_much_of_the_one_before() {
        let today = date(2026, 10, 9);
        let first_day = date(2025, 4, 12);
        let earlier = |span, steps_back| {
            let view = view(span, steps_back);
            view.earlier_range(&view.range(today, first_day), today)
        };
        assert_eq!(
            earlier(Span::Week, 0),
            Some(date(2026, 9, 28)..=date(2026, 10, 2))
        );
        assert_eq!(
            earlier(Span::Week, 1),
            Some(date(2026, 9, 21)..=date(2026, 9, 27))
        );
        assert_eq!(
            earlier(Span::Month, 0),
            Some(date(2026, 9, 1)..=date(2026, 9, 9))
        );
        // March is longer than the February it is compared with.
        assert_eq!(
            earlier(Span::Month, 7),
            Some(date(2026, 2, 1)..=date(2026, 2, 28))
        );
        assert_eq!(
            earlier(Span::Year, 0),
            Some(date(2025, 1, 1)..=date(2025, 10, 9))
        );
        assert_eq!(earlier(Span::All, 0), None);
        assert_eq!(
            earlier(Span::Custom, 0),
            Some(date(2026, 9, 22)..=date(2026, 9, 30))
        );
    }

    #[test]
    fn a_clicked_bar_opens_its_week_or_month() {
        let today = date(2026, 10, 9);
        let first_day = date(2025, 4, 12);
        let mut view = view(Span::Year, 0);
        view.show_period(Span::Week, date(2026, 9, 24), today);
        assert_eq!(
            view.range(today, first_day),
            date(2026, 9, 21)..=date(2026, 9, 27)
        );
        view.show_period(Span::Month, date(2025, 12, 1), today);
        assert_eq!(
            view.range(today, first_day),
            date(2025, 12, 1)..=date(2025, 12, 31)
        );
    }

    #[test]
    fn bars_are_days_then_weeks_then_months() {
        let from = date(2026, 1, 1);
        assert_eq!(bucket_size(&(from..=date(2026, 1, 31))), BucketSize::Day);
        assert_eq!(bucket_size(&(from..=date(2026, 12, 31))), BucketSize::Week);
        assert_eq!(bucket_size(&(from..=date(2027, 6, 30))), BucketSize::Month);
    }

    #[test]
    fn a_time_axis_has_round_steps_and_at_most_four_of_them() {
        let hours = |hours: u64| hours * 3600;
        assert_eq!(axis_step(0), 5 * 60);
        assert_eq!(axis_step(50 * 60), 15 * 60);
        assert_eq!(axis_step(hours(11)), hours(4));
        assert_eq!(axis_step(hours(63)), hours(20));
        assert_eq!(axis_step(hours(260)), hours(100));
        assert_eq!(axis_step(hours(9000)), hours(3000));
        assert_eq!(axis_text(hours(8)), "8h");
        assert_eq!(axis_text(45 * 60), "45m");
    }

    #[test]
    fn day_ranges_do_not_repeat_what_their_ends_share() {
        let text = |from, to| format_days(&(from..=to));
        assert_eq!(text(date(2026, 10, 5), date(2026, 10, 5)), "5 Oct 2026");
        assert_eq!(
            text(date(2026, 10, 5), date(2026, 10, 11)),
            "5 – 11 Oct 2026"
        );
        assert_eq!(
            text(date(2026, 9, 28), date(2026, 10, 4)),
            "28 Sep – 4 Oct 2026"
        );
        assert_eq!(
            text(date(2025, 12, 29), date(2026, 1, 4)),
            "29 Dec 2025 – 4 Jan 2026"
        );
    }

    #[test]
    fn similar_colors_are_stacked_next_to_each_other() {
        let rgb = |red, green, blue| hue(Color32::from_rgb(red, green, blue));
        assert_eq!(hue(Color32::from_gray(128)), None);
        // A slate gray is too pale to count as a blue.
        assert_eq!(hue(Color32::from_rgb(78, 92, 107)), None);
        let blue = rgb(42, 120, 214).unwrap();
        let light_blue = rgb(72, 141, 229).unwrap();
        let orange = rgb(221, 99, 1).unwrap();
        assert!((blue - light_blue).abs() < 0.01);
        assert!((blue - orange).abs() > 0.3);
    }
}
