//! Small self-contained egui date picker.
//!
//! The widget owns only transient UI state and edits a caller-owned
//! `NaiveDate`. It has no knowledge of tracker entries or persistence.

use std::{fmt::Debug, hash::Hash, ops::RangeInclusive};

use chrono::{Datelike as _, Duration, Local, NaiveDate};
use eframe::egui::{
    self, Align, Align2, Area, Button, Color32, FontId, Frame, Grid, Id, Key, Layout, Order,
    RichText, Stroke, StrokeKind, Ui, Widget,
};

const BUTTON_SIZE: egui::Vec2 = egui::vec2(30.0, 26.0);
const POPUP_WIDTH: f32 = 292.0;
const POPUP_ESTIMATED_HEIGHT: f32 = 292.0;

#[derive(Clone)]
struct PickerState {
    open: bool,
    draft: NaiveDate,
}

pub struct DatePickerButton<'a> {
    selection: &'a mut NaiveDate,
    id_salt: Id,
    year_range: RangeInclusive<i32>,
}

impl<'a> DatePickerButton<'a> {
    pub fn new(selection: &'a mut NaiveDate, id_salt: impl Hash + Debug) -> Self {
        Self {
            selection,
            id_salt: Id::new(id_salt),
            year_range: NaiveDate::MIN.year()..=NaiveDate::MAX.year(),
        }
    }

    pub fn year_range(mut self, range: RangeInclusive<i32>) -> Self {
        self.year_range = range;
        self
    }
}

impl Widget for DatePickerButton<'_> {
    fn ui(self, ui: &mut Ui) -> egui::Response {
        let id = ui.make_persistent_id(self.id_salt);
        let mut state = ui
            .data(|data| data.get_temp::<PickerState>(id))
            .unwrap_or(PickerState {
                open: false,
                draft: *self.selection,
            });

        let mut response = ui.add(Button::new("").frame(false).min_size(BUTTON_SIZE));
        if response.clicked() {
            state.open = !state.open;
            state.draft = *self.selection;
        }

        let fill = if response.is_pointer_button_down_on() {
            Color32::from_rgb(205, 222, 232)
        } else if response.hovered() || state.open {
            Color32::from_rgb(226, 238, 244)
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(response.rect, 3.0, fill);
        ui.painter().text(
            response.rect.center(),
            Align2::CENTER_CENTER,
            "📆",
            FontId::proportional(17.0),
            ui.visuals().text_color(),
        );

        if state.open {
            let screen = ui.ctx().content_rect();
            let mut position = response.rect.left_bottom() + egui::vec2(0.0, 3.0);
            if position.x + POPUP_WIDTH > screen.right() {
                position.x = response.rect.right() - POPUP_WIDTH;
            }
            position.x = position.x.max(screen.left() + 4.0);
            if position.y + POPUP_ESTIMATED_HEIGHT > screen.bottom() {
                position.y = response.rect.top() - POPUP_ESTIMATED_HEIGHT;
            }
            position.y = position.y.max(screen.top() + 4.0);

            let popup = Area::new(id.with("popup"))
                .kind(egui::UiKind::Picker)
                .order(Order::Foreground)
                .fixed_pos(position)
                .show(ui.ctx(), |ui| {
                    Frame::popup(ui.style())
                        .show(ui, |ui| {
                            ui.set_width(POPUP_WIDTH);
                            DatePickerPopup {
                                draft: &mut state.draft,
                                id,
                                year_range: &self.year_range,
                            }
                            .draw(ui)
                        })
                        .inner
                });

            match popup.inner {
                PopupAction::Save => {
                    *self.selection = state.draft;
                    state.open = false;
                    response.mark_changed();
                }
                PopupAction::Cancel => state.open = false,
                PopupAction::None => {
                    if !response.clicked()
                        && (ui.input(|input| input.key_pressed(Key::Escape))
                            || popup.response.clicked_elsewhere())
                    {
                        state.open = false;
                    }
                }
            }
        }

        ui.data_mut(|data| data.insert_temp(id, state));
        response
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum PopupAction {
    #[default]
    None,
    Save,
    Cancel,
}

struct DatePickerPopup<'a> {
    draft: &'a mut NaiveDate,
    id: Id,
    year_range: &'a RangeInclusive<i32>,
}

impl DatePickerPopup<'_> {
    fn draw(&mut self, ui: &mut Ui) -> PopupAction {
        let mut action = PopupAction::None;
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);

        ui.horizontal(|ui| {
            let previous_year = shift_year(*self.draft, -1);
            if navigation_button(
                ui,
                self.year_range.contains(&previous_year.year()),
                "«",
                "Previous year",
            ) {
                *self.draft = previous_year;
            }
            let previous_month = shift_month(*self.draft, -1);
            if navigation_button(
                ui,
                self.year_range.contains(&previous_month.year()),
                "‹",
                "Previous month",
            ) {
                *self.draft = previous_month;
            }
            ui.allocate_ui_with_layout(
                egui::vec2(156.0, 26.0),
                Layout::centered_and_justified(egui::Direction::TopDown),
                |ui| {
                    ui.label(RichText::new(self.draft.format("%b %Y").to_string()).strong());
                },
            );
            let next_month = shift_month(*self.draft, 1);
            if navigation_button(
                ui,
                self.year_range.contains(&next_month.year()),
                "›",
                "Next month",
            ) {
                *self.draft = next_month;
            }
            let next_year = shift_year(*self.draft, 1);
            if navigation_button(
                ui,
                self.year_range.contains(&next_year.year()),
                "»",
                "Next year",
            ) {
                *self.draft = next_year;
            }
        });

        ui.add_space(2.0);
        Grid::new(self.id.with("calendar"))
            .num_columns(7)
            .spacing(egui::vec2(4.0, 3.0))
            .show(ui, |ui| {
                for weekday in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
                    ui.allocate_ui_with_layout(
                        egui::vec2(36.0, 20.0),
                        Layout::centered_and_justified(egui::Direction::TopDown),
                        |ui| {
                            ui.label(
                                RichText::new(weekday)
                                    .small()
                                    .color(Color32::from_gray(100)),
                            );
                        },
                    );
                }
                ui.end_row();

                let first = self.draft.with_day(1).expect("day one is valid");
                let start = first - Duration::days(first.weekday().num_days_from_monday() as i64);
                let today = Local::now().date_naive();
                for offset in 0..42 {
                    let date = start + Duration::days(offset);
                    let current_month = date.month() == self.draft.month();
                    let selected = date == *self.draft;
                    let weekend = date.weekday().number_from_monday() >= 6;
                    let text_color = if !current_month {
                        Color32::from_gray(165)
                    } else if weekend {
                        Color32::from_rgb(174, 72, 72)
                    } else {
                        ui.visuals().text_color()
                    };
                    let enabled = self.year_range.contains(&date.year());
                    let day = ui.add_enabled_ui(enabled, |ui| {
                        ui.add_sized(egui::vec2(36.0, 26.0), Button::new("").frame(false))
                    });
                    let day = day.inner;
                    let fill = if selected {
                        ui.visuals().selection.bg_fill
                    } else if day.is_pointer_button_down_on() {
                        ui.visuals().widgets.active.weak_bg_fill
                    } else if day.hovered() {
                        ui.visuals().widgets.hovered.weak_bg_fill
                    } else {
                        Color32::TRANSPARENT
                    };
                    ui.painter().rect_filled(day.rect, 3.0, fill);
                    ui.painter().text(
                        day.rect.center(),
                        Align2::CENTER_CENTER,
                        date.day().to_string(),
                        FontId::proportional(13.0),
                        if enabled {
                            text_color
                        } else {
                            ui.visuals().weak_text_color()
                        },
                    );
                    if date == today {
                        ui.painter().rect_stroke(
                            day.rect.shrink(2.0),
                            3.0,
                            Stroke::new(1.0, ui.visuals().widgets.inactive.fg_stroke.color),
                            StrokeKind::Inside,
                        );
                    }
                    if day.clicked() {
                        *self.draft = date;
                    }
                    if (offset + 1) % 7 == 0 {
                        ui.end_row();
                    }
                }
            });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let today = Local::now().date_naive();
            if ui
                .add_enabled(
                    self.year_range.contains(&today.year()),
                    Button::new("Today"),
                )
                .clicked()
            {
                *self.draft = Local::now().date_naive();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Save").clicked() {
                    action = PopupAction::Save;
                }
                if ui.button("Cancel").clicked() {
                    action = PopupAction::Cancel;
                }
            });
        });
        action
    }
}

fn navigation_button(ui: &mut Ui, enabled: bool, text: &str, tooltip: &str) -> bool {
    ui.add_enabled_ui(enabled, |ui| {
        ui.add_sized(egui::vec2(26.0, 26.0), Button::new(text))
    })
    .inner
    .on_hover_text(tooltip)
    .clicked()
}

fn shift_month(date: NaiveDate, months: i32) -> NaiveDate {
    let month_index = date.year() * 12 + date.month0() as i32 + months;
    let year = month_index.div_euclid(12);
    let month = month_index.rem_euclid(12) as u32 + 1;
    date_with_clamped_day(year, month, date.day()).unwrap_or(date)
}

fn shift_year(date: NaiveDate, years: i32) -> NaiveDate {
    date_with_clamped_day(date.year().saturating_add(years), date.month(), date.day())
        .unwrap_or(date)
}

fn date_with_clamped_day(year: i32, month: u32, day: u32) -> Option<NaiveDate> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    let next_month = if month == 12 {
        NaiveDate::from_ymd_opt(year.checked_add(1)?, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)?
    };
    let last_day = (next_month - Duration::days(1)).day();
    first.with_day(day.min(last_day))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_navigation_clamps_to_the_last_valid_day() {
        let january_31 = NaiveDate::from_ymd_opt(2025, 1, 31).unwrap();
        assert_eq!(
            shift_month(january_31, 1),
            NaiveDate::from_ymd_opt(2025, 2, 28).unwrap()
        );
    }

    #[test]
    fn year_navigation_clamps_leap_day() {
        let leap_day = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
        assert_eq!(
            shift_year(leap_day, 1),
            NaiveDate::from_ymd_opt(2025, 2, 28).unwrap()
        );
    }
}
