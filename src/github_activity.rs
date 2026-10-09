use std::ops::RangeInclusive;

use chrono::{Datelike, Duration, NaiveDate};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Rangef, Rect, Sense, Stroke, StrokeKind, Vec2,
};

/// A whole calendar year always fits; the cells shrink to make room for it.
const MIN_WEEKS: usize = 53;
const DAYS: usize = 7;
const LABEL_WIDTH: f32 = 34.0;
const LABEL_HEIGHT: f32 = 18.0;
/// Cells stop growing at this pitch. A wider window shows more weeks instead.
const MAX_STEP: f32 = 15.0;
const MIN_STEP: f32 = 5.0;
const MAX_WEEK_OFFSET: i32 = 5_200;

#[derive(Clone, Copy)]
pub struct Segment {
    pub color: Color32,
    pub weight: f32,
}

pub enum CellFill<'a> {
    Empty,
    Solid(Color32),
    Segmented(&'a [Segment]),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionChange {
    #[default]
    Unchanged,
    Select(NaiveDate),
    Clear,
}

/// Reusable GitHub-style calendar heatmap state.
///
/// Its last 53 columns hold the current calendar year. A window wide enough
/// for more columns shows the preceding weeks to their left, and the whole
/// calendar can be dragged or scrolled in either direction. The caller
/// supplies cell presentation and tooltip content, so the widget does not
/// know what the dates or colors mean.
pub struct CalendarHeatmap {
    /// Whole weeks scrolled away from the current year, positive into the past.
    week_offset: i32,
    drag_remainder: f32,
    /// Columns drawn by the last [`Self::ui`] call.
    weeks: usize,
    cells_x: Rangef,
}

impl Default for CalendarHeatmap {
    fn default() -> Self {
        Self {
            week_offset: 0,
            drag_remainder: 0.0,
            weeks: MIN_WEEKS,
            cells_x: Rangef::NOTHING,
        }
    }
}

impl CalendarHeatmap {
    /// Scrolls by whole weeks. Positive values move back in time.
    pub fn scroll_weeks(&mut self, weeks: i32) {
        self.week_offset = (self.week_offset + weeks).clamp(-MAX_WEEK_OFFSET, MAX_WEEK_OFFSET);
        self.drag_remainder = 0.0;
    }

    /// Whether the calendar has been moved away from the current year.
    pub fn is_scrolled(&self) -> bool {
        self.week_offset != 0
    }

    pub fn reset_scroll(&mut self) {
        self.week_offset = 0;
        self.drag_remainder = 0.0;
    }

    /// The days shown by the last [`Self::ui`] call.
    pub fn visible_days(&self, today: NaiveDate) -> RangeInclusive<NaiveDate> {
        let first_day = self.first_day(today);
        first_day..=first_day + Duration::days((self.weeks * DAYS) as i64 - 1)
    }

    /// The horizontal extent of the cells drawn by the last [`Self::ui`]
    /// call. Callers use it to line their own content up with the centered
    /// grid.
    pub fn cells_x_range(&self) -> Rangef {
        self.cells_x
    }

    fn first_day(&self, today: NaiveDate) -> NaiveDate {
        let year_start = NaiveDate::from_ymd_opt(today.year(), 1, 1).expect("valid year start");
        let year_start_monday =
            year_start - Duration::days(year_start.weekday().num_days_from_monday() as i64);
        let extra_weeks = self.weeks.saturating_sub(MIN_WEEKS) as i64;
        year_start_monday - Duration::weeks(extra_weeks + self.week_offset as i64)
    }

    pub fn ui<'a>(
        &mut self,
        ui: &mut egui::Ui,
        today: NaiveDate,
        selected: Option<NaiveDate>,
        fill: impl Fn(NaiveDate) -> CellFill<'a>,
        tooltip: impl Fn(&mut egui::Ui, NaiveDate),
    ) -> SelectionChange {
        let available = ui
            .available_width()
            .max(LABEL_WIDTH + MIN_WEEKS as f32 * MIN_STEP);
        let grid_width = available - LABEL_WIDTH;
        let step = (grid_width / MIN_WEEKS as f32).clamp(MIN_STEP, MAX_STEP);
        let weeks = ((grid_width / step) as usize).max(MIN_WEEKS);
        self.weeks = weeks;
        let gap = (step * 0.13).clamp(1.5, 4.0);
        let square = step - gap;
        let desired = Vec2::new(available, LABEL_HEIGHT * 2.0 + DAYS as f32 * step);
        let (outer, response) = ui.allocate_exact_size(desired, Sense::click_and_drag());
        // The columns rarely add up to the exact width; the rest is split
        // between both sides.
        let cells_width = weeks as f32 * step;
        let cells_left = outer.left() + LABEL_WIDTH + ((grid_width - cells_width) * 0.5).max(0.0);
        self.cells_x = Rangef::new(cells_left, cells_left + cells_width - gap);

        if response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            self.drag_remainder += response.drag_delta().x;
            let dragged_weeks = (self.drag_remainder / step).trunc() as i32;
            if dragged_weeks != 0 {
                self.week_offset =
                    (self.week_offset + dragged_weeks).clamp(-MAX_WEEK_OFFSET, MAX_WEEK_OFFSET);
                self.drag_remainder -= dragged_weeks as f32 * step;
            }
        }

        let first_day = self.first_day(today);
        let origin = egui::pos2(cells_left, outer.top() + LABEL_HEIGHT);
        // The boundaries stay lighter than the cells so that the data, not
        // the grid, is what stands out.
        let noninteractive = &ui.visuals().widgets.noninteractive;
        let month_stroke = Stroke::new(1.0, noninteractive.bg_stroke.color);
        let year_stroke = Stroke::new(1.5, noninteractive.fg_stroke.color);
        let hover_stroke = Stroke::new(1.5, ui.visuals().strong_text_color());
        let selection_color = ui.visuals().selection.stroke.color;
        let dark_mode = ui.visuals().dark_mode;
        let pointer_position = ui.ctx().pointer_hover_pos();
        let mut hovered_date = None;

        for week in 0..weeks {
            for weekday in 0..DAYS {
                let date = first_day + Duration::days((week * DAYS + weekday) as i64);
                let min = origin + egui::vec2(week as f32 * step, weekday as f32 * step);
                let cell = Rect::from_min_size(min, Vec2::splat(square));
                if date > today {
                    ui.painter().rect_filled(cell, 2.0, future_color(dark_mode));
                } else {
                    match fill(date) {
                        CellFill::Empty => {
                            ui.painter().rect_filled(cell, 2.0, empty_color(dark_mode));
                        }
                        CellFill::Solid(color) => {
                            ui.painter().rect_filled(cell, 2.0, color);
                        }
                        CellFill::Segmented(segments) => paint_segments(ui, cell, segments),
                    }
                }
                Self::paint_calendar_boundaries(
                    ui,
                    date,
                    cell,
                    (week, weekday),
                    week + 1 == weeks,
                    gap,
                    (month_stroke, year_stroke),
                );

                let cell_response = ui.interact(
                    cell.expand(gap * 0.5),
                    response.id.with(date),
                    Sense::hover(),
                );
                let date_is_hovered = date <= today
                    && pointer_position
                        .is_some_and(|position| cell.expand(gap * 0.5).contains(position));
                if date_is_hovered {
                    hovered_date = Some(date);
                }
                if cell_response.hovered() && date <= today && !response.dragged() {
                    ui.painter().rect_stroke(
                        cell.expand(1.0),
                        2.0,
                        hover_stroke,
                        StrokeKind::Outside,
                    );
                    cell_response.on_hover_ui_at_pointer(|ui| tooltip(ui, date));
                }
                if selected == Some(date) {
                    ui.painter().rect_stroke(
                        cell.expand(1.0),
                        2.0,
                        Stroke::new(2.0, selection_color),
                        StrokeKind::Outside,
                    );
                }
            }
        }

        Self::paint_labels(ui, first_day, weeks, origin, step);

        if selected.is_some() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            SelectionChange::Clear
        } else if response.clicked() {
            hovered_date
                .map(SelectionChange::Select)
                .unwrap_or(SelectionChange::Clear)
        } else {
            SelectionChange::Unchanged
        }
    }

    fn paint_calendar_boundaries(
        ui: &egui::Ui,
        date: NaiveDate,
        cell: Rect,
        grid_position: (usize, usize),
        last_week: bool,
        gap: f32,
        strokes: (Stroke, Stroke),
    ) {
        let (week, weekday) = grid_position;
        let (month_stroke, year_stroke) = strokes;
        let half_gap = gap * 0.5;
        let left = cell.left() - half_gap;
        let right = cell.right() + half_gap;
        let top = cell.top() - half_gap;
        let bottom = cell.bottom() + half_gap;

        if weekday == 0 {
            ui.painter().line_segment(
                [egui::pos2(left, top), egui::pos2(right, top)],
                month_stroke,
            );
        }
        if weekday + 1 == DAYS {
            ui.painter().line_segment(
                [egui::pos2(left, bottom), egui::pos2(right, bottom)],
                month_stroke,
            );
        }
        if week == 0 {
            ui.painter().line_segment(
                [egui::pos2(left, top), egui::pos2(left, bottom)],
                month_stroke,
            );
        }

        if last_week {
            ui.painter().line_segment(
                [egui::pos2(right, top), egui::pos2(right, bottom)],
                month_stroke,
            );
        } else {
            let right_date = date + Duration::weeks(1);
            if right_date.year() != date.year() {
                ui.painter().line_segment(
                    [egui::pos2(right, top), egui::pos2(right, bottom)],
                    year_stroke,
                );
            } else if right_date.month() != date.month() {
                ui.painter().line_segment(
                    [egui::pos2(right, top), egui::pos2(right, bottom)],
                    month_stroke,
                );
            }
        }

        if weekday + 1 < DAYS {
            let down_date = date + Duration::days(1);
            if down_date.year() != date.year() {
                ui.painter().line_segment(
                    [egui::pos2(left, bottom), egui::pos2(right, bottom)],
                    year_stroke,
                );
            } else if down_date.month() != date.month() {
                ui.painter().line_segment(
                    [egui::pos2(left, bottom), egui::pos2(right, bottom)],
                    month_stroke,
                );
            }
        }
    }

    fn paint_labels(
        ui: &egui::Ui,
        first_day: NaiveDate,
        weeks: usize,
        origin: egui::Pos2,
        step: f32,
    ) {
        let font = FontId::proportional(if step < 7.5 { 10.0 } else { 12.0 });
        let color = if ui.visuals().dark_mode {
            Color32::from_rgb(139, 148, 158)
        } else {
            Color32::from_rgb(87, 96, 106)
        };

        for (weekday, label) in [(0, "Mon"), (6, "Sun")] {
            ui.painter().text(
                origin + egui::vec2(-LABEL_WIDTH, weekday as f32 * step + step * 0.4),
                Align2::LEFT_CENTER,
                label,
                font.clone(),
                color,
            );
        }

        let column = |week: usize| first_day + Duration::weeks(week as i64);
        let first = column(0);
        // The first column is usually the tail of a month or year. Its label
        // is dropped when the next one would be drawn on top of it.
        let fits =
            |next: Option<usize>, width: f32| next.is_none_or(|week| week as f32 * step >= width);
        let next_month = (1..weeks).find(|&week| {
            (column(week).year(), column(week).month()) != (first.year(), first.month())
        });
        let next_year = (1..weeks).find(|&week| column(week).year() != first.year());
        let mut last_month = (!fits(next_month, 28.0)).then_some((first.year(), first.month()));
        let mut last_year = (!fits(next_year, 34.0)).then_some(first.year());
        for week in 0..weeks {
            let date = column(week);
            if last_month != Some((date.year(), date.month())) {
                ui.painter().text(
                    origin + egui::vec2(week as f32 * step, DAYS as f32 * step + 2.0),
                    Align2::LEFT_TOP,
                    month_name(date.month()),
                    font.clone(),
                    color,
                );
                last_month = Some((date.year(), date.month()));
            }
            if last_year != Some(date.year()) {
                ui.painter().text(
                    origin + egui::vec2(week as f32 * step, -LABEL_HEIGHT),
                    Align2::LEFT_TOP,
                    date.year().to_string(),
                    font.clone(),
                    ui.visuals().strong_text_color(),
                );
                last_year = Some(date.year());
            }
        }
    }
}

fn paint_segments(ui: &egui::Ui, cell: Rect, segments: &[Segment]) {
    let total: f32 = segments.iter().map(|segment| segment.weight.max(0.0)).sum();
    if total <= 0.0 {
        ui.painter()
            .rect_filled(cell, 2.0, empty_color(ui.visuals().dark_mode));
        return;
    }

    let radius = 2;
    let mut top = cell.top();
    for (index, segment) in segments.iter().enumerate() {
        let last = index + 1 == segments.len();
        let bottom = if last {
            cell.bottom()
        } else {
            (top + cell.height() * segment.weight.max(0.0) / total).min(cell.bottom())
        };
        let stripe = Rect::from_min_max(
            egui::pos2(cell.left(), top),
            egui::pos2(cell.right(), bottom),
        );
        let corners = CornerRadius {
            nw: if index == 0 { radius } else { 0 },
            ne: if index == 0 { radius } else { 0 },
            sw: if last { radius } else { 0 },
            se: if last { radius } else { 0 },
        };
        ui.painter().rect_filled(stripe, corners, segment.color);
        top = bottom;
    }
}

/// A day that has not come yet.
fn future_color(dark_mode: bool) -> Color32 {
    if dark_mode {
        Color32::from_rgb(28, 32, 38)
    } else {
        Color32::from_gray(246)
    }
}

/// A day without anything to show.
fn empty_color(dark_mode: bool) -> Color32 {
    if dark_mode {
        Color32::from_rgb(38, 44, 52)
    } else {
        Color32::from_rgb(235, 237, 240)
    }
}

/// The green for a value between nothing (0) and a full cell (1), with one
/// more shade for everything beyond that. More is darker on a light
/// background and brighter on a dark one.
pub fn heat_color(value: f32, dark_mode: bool) -> Color32 {
    let shades: [[u8; 3]; 5] = if dark_mode {
        [
            [14, 68, 41],
            [0, 109, 50],
            [38, 166, 65],
            [57, 211, 83],
            [134, 239, 144],
        ]
    } else {
        [
            [155, 233, 168],
            [64, 196, 99],
            [48, 161, 78],
            [33, 110, 57],
            [14, 68, 41],
        ]
    };
    let [red, green, blue] = if value > 1.0 {
        shades[4]
    } else if value <= 0.0 {
        return empty_color(dark_mode);
    } else if value < 0.25 {
        shades[0]
    } else if value < 0.5 {
        shades[1]
    } else if value < 0.75 {
        shades[2]
    } else {
        shades[3]
    };
    Color32::from_rgb(red, green, blue)
}

fn month_name(month: u32) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "",
    }
}
