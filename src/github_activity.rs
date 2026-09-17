use chrono::{Datelike, Duration, NaiveDate};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, StrokeKind, Vec2,
};

const WEEKS: usize = 53;
const DAYS: usize = 7;
const LABEL_WIDTH: f32 = 34.0;
const LABEL_HEIGHT: f32 = 18.0;

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
/// The caller supplies cell presentation and tooltip content, so the widget
/// does not know what the dates or colors mean.
#[derive(Default)]
pub struct CalendarHeatmap {
    week_offset: i32,
    drag_remainder: f32,
}

impl CalendarHeatmap {
    pub fn ui<'a>(
        &mut self,
        ui: &mut egui::Ui,
        today: NaiveDate,
        selected: Option<NaiveDate>,
        fill: impl Fn(NaiveDate) -> CellFill<'a>,
        tooltip: impl Fn(&mut egui::Ui, NaiveDate),
    ) -> SelectionChange {
        let available = ui.available_width().max(420.0);
        let step = ((available - LABEL_WIDTH) / WEEKS as f32).max(7.0);
        let gap = (step * 0.13).clamp(2.0, 4.0);
        let square = (step - gap).max(5.0);
        let desired = Vec2::new(available, LABEL_HEIGHT * 2.0 + DAYS as f32 * step);
        let (outer, response) = ui.allocate_exact_size(desired, Sense::click_and_drag());

        if response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            self.drag_remainder += response.drag_delta().x;
            let weeks = (self.drag_remainder / step).trunc() as i32;
            if weeks != 0 {
                self.week_offset = (self.week_offset + weeks).clamp(-5_200, 5_200);
                self.drag_remainder -= weeks as f32 * step;
            }
        }

        let year_start = NaiveDate::from_ymd_opt(today.year(), 1, 1).expect("valid year start");
        let year_start_monday =
            year_start - Duration::days(year_start.weekday().num_days_from_monday() as i64);
        let first_day = year_start_monday - Duration::weeks(self.week_offset as i64);
        let origin = outer.min + egui::vec2(LABEL_WIDTH, LABEL_HEIGHT);
        let separator = Color32::from_rgb(31, 35, 40);
        let month_stroke = Stroke::new(1.0, separator);
        let year_stroke = Stroke::new(2.0, separator);
        let selection_color = ui.visuals().selection.bg_fill;
        let pointer_position = ui.ctx().pointer_hover_pos();
        let mut hovered_date = None;

        for week in 0..WEEKS {
            for weekday in 0..DAYS {
                let date = first_day + Duration::days((week * DAYS + weekday) as i64);
                let min = origin + egui::vec2(week as f32 * step, weekday as f32 * step);
                let cell = Rect::from_min_size(min, Vec2::splat(square));
                if date > today {
                    ui.painter().rect_filled(cell, 2.0, Color32::from_gray(246));
                } else {
                    match fill(date) {
                        CellFill::Empty => {
                            ui.painter()
                                .rect_filled(cell, 2.0, Color32::from_rgb(235, 237, 240));
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
                        Stroke::new(1.5, separator),
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

        self.paint_labels(
            ui,
            first_day,
            origin,
            step,
            (self.week_offset == 0).then_some(today.year()),
        );

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

        if week + 1 < WEEKS {
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
        &self,
        ui: &egui::Ui,
        first_day: NaiveDate,
        origin: egui::Pos2,
        step: f32,
        left_year: Option<i32>,
    ) {
        let font = FontId::proportional((step * 0.52).clamp(12.0, 15.0));
        let color = Color32::from_rgb(87, 96, 106);

        for (weekday, label) in [(0, "Mon"), (6, "Sun")] {
            ui.painter().text(
                origin + egui::vec2(-LABEL_WIDTH, weekday as f32 * step + step * 0.4),
                Align2::LEFT_CENTER,
                label,
                font.clone(),
                color,
            );
        }

        let mut last_month = left_year.map(|year| (year, 1));
        let mut last_year = left_year;
        if let Some(year) = left_year {
            ui.painter().text(
                origin + egui::vec2(0.0, DAYS as f32 * step + 2.0),
                Align2::LEFT_TOP,
                "Jan",
                font.clone(),
                color,
            );
            ui.painter().text(
                origin + egui::vec2(0.0, -LABEL_HEIGHT),
                Align2::LEFT_TOP,
                year.to_string(),
                font.clone(),
                Color32::from_rgb(207, 34, 46),
            );
        }
        for week in 0..WEEKS {
            let date = first_day + Duration::weeks(week as i64);
            if left_year.is_some_and(|year| date.year() < year) {
                continue;
            }
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
                    Color32::from_rgb(207, 34, 46),
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
            .rect_filled(cell, 2.0, Color32::from_rgb(235, 237, 240));
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

pub fn heat_color(value: f32) -> Color32 {
    if value > 1.0 {
        return Color32::from_rgb(31, 35, 40);
    }
    let value = value.clamp(0.0, 1.0);
    if value <= 0.0 {
        Color32::from_rgb(235, 237, 240)
    } else if value < 0.25 {
        Color32::from_rgb(155, 233, 168)
    } else if value < 0.5 {
        Color32::from_rgb(64, 196, 99)
    } else if value < 0.75 {
        Color32::from_rgb(48, 161, 78)
    } else {
        Color32::from_rgb(33, 110, 57)
    }
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
