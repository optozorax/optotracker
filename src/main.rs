#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Local, NaiveDate};
use eframe::egui::{
    self, Align, Align2, Atom, AtomExt, Atoms, Button, Color32, ComboBox, FontData,
    FontDefinitions, FontFamily, FontId, Frame, Layout, RichText, ScrollArea, Stroke,
};

mod app_icon;
mod date_picker;
mod desktop;
mod github_activity;
mod local_time;
mod model;
mod pagination;
mod paths;
mod reports;
mod single_instance;
mod storage;
mod tray;

use date_picker::DatePickerButton;
use github_activity::{
    heat_color, CalendarHeatmap, CellFill, Segment, SelectionChange as CalendarSelectionChange,
};
use local_time::{datetime as local_datetime, TimeEndpoint};

use model::{
    Activity, ActivityId, EntryId, Project, ProjectId, ProjectStatus, RgbColor, Store, TimeRange,
    UnixSeconds, MAX_ENTRY_SECONDS,
};
use pagination::Pagination;
use paths::AppPaths;
use reports::{
    activity_stats, activity_totals, add_interval_to_days, project_stats, totals_by_local_day,
    totals_by_week, week_start, UsageStats,
};
use storage::JsonStore;

// The button colors are dark enough for their white labels to stay readable.
const BLUE: Color32 = Color32::from_rgb(0, 118, 200);
const RED: Color32 = Color32::from_rgb(211, 47, 47);
const GREEN: Color32 = Color32::from_rgb(56, 142, 60);
const TEXT: Color32 = Color32::from_rgb(34, 40, 47);
// The page is nearly white, so cards are told apart by their borders and
// their tinted headers.
const PAGE: Color32 = Color32::from_rgb(249, 250, 252);
const HEADER: Color32 = Color32::from_rgb(235, 241, 246);
const BORDER: Color32 = Color32::from_rgb(203, 212, 220);
const HAIRLINE: Color32 = Color32::from_rgb(226, 232, 237);
const OUTLINE: Color32 = Color32::from_rgb(196, 205, 213);
const HOVER: Color32 = Color32::from_rgb(238, 243, 247);
const PRESSED: Color32 = Color32::from_rgb(224, 231, 237);
const SELECTION: Color32 = Color32::from_rgb(214, 232, 248);
const SELECTION_TEXT: Color32 = Color32::from_rgb(0, 84, 150);
const ROW_HEIGHT: f32 = 26.0;
const TIMER_HEIGHT: f32 = 34.0;
const TIMER_BUTTON_WIDTH: f32 = 84.0;
const TIME_FIELD_WIDTH: f32 = 50.0;
const NAME_FIELD_WIDTH: f32 = 180.0;
// Room for everything to the right of a row's two drop-downs.
const TIMER_CONTROLS_WIDTH: f32 = 264.0;
const ENTRY_CONTROLS_WIDTH: f32 = 292.0;
const MIN_SELECTOR_WIDTH: f32 = 110.0;
const MAX_SELECTOR_WIDTH: f32 = 300.0;
const CALENDAR_PAGE_WEEKS: i32 = 13;
const LEGEND_ITEMS: usize = 6;
// A day with this much tracked time gets the darkest regular heat color.
const FULL_DAY_SECONDS: f32 = 8.0 * 60.0 * 60.0;
const NO_PROJECT_COLOR: Color32 = Color32::from_gray(125);
const STRIPE: Color32 = Color32::from_rgb(246, 249, 251);
const ROW_HOVER: Color32 = Color32::from_rgb(244, 248, 251);
const ENTRIES_PER_PAGE: usize = 100;
const UI_SELECTIONS_MEMORY_ID: &str = "opto_tracker_ui_selections_v1";

fn main() -> eframe::Result {
    let _instance_guard = match single_instance::SingleInstanceGuard::acquire() {
        Ok(Some(guard)) => guard,
        Ok(None) => {
            single_instance::SingleInstanceGuard::show_already_running_message();
            return Ok(());
        }
        Err(error) => {
            return Err(eframe::Error::AppCreation(Box::new(std::io::Error::other(
                error,
            ))));
        }
    };

    let mut start_minimized = false;
    let mut store_path = None;
    for argument in std::env::args_os().skip(1) {
        if argument == "--minimized" {
            start_minimized = true;
        } else if store_path.is_none() {
            store_path = Some(PathBuf::from(argument));
        } else {
            single_instance::SingleInstanceGuard::show_error_message(
                "Usage: OptoTracker.exe [--minimized] [DATA_FILE]",
            );
            return Ok(());
        }
    }
    let app_paths = match AppPaths::discover() {
        Ok(paths) => paths,
        Err(error) => {
            single_instance::SingleInstanceGuard::show_error_message(&error);
            return Ok(());
        }
    };
    let store_path = store_path.unwrap_or_else(|| app_paths.data.clone());
    let repository = JsonStore::new(store_path);
    let store = match repository.load() {
        Ok(store) => store,
        Err(error) => {
            single_instance::SingleInstanceGuard::show_error_message(&error.to_string());
            return Ok(());
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OptoTracker")
            .with_icon(app_icon::icon_data(false))
            .with_visible(true)
            .with_inner_size([720.0, 620.0])
            .with_min_inner_size([420.0, 320.0]),
        persistence_path: Some(app_paths.persistence.clone()),
        ..Default::default()
    };
    eframe::run_native(
        "OptoTracker",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::light());
            configure_style(&cc.egui_ctx);
            configure_fonts(&cc.egui_ctx);
            Ok(Box::new(TimeTrackerApp::load(
                repository.clone(),
                store.clone(),
                app_paths.clone(),
                cc.storage,
                start_minimized,
                &cc.egui_ctx,
            )))
        }),
    )
}

fn configure_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        for (text_style, size) in [
            (egui::TextStyle::Body, 14.0),
            (egui::TextStyle::Button, 14.0),
            (egui::TextStyle::Small, 11.0),
        ] {
            style
                .text_styles
                .insert(text_style, FontId::proportional(size));
        }
        style.spacing.button_padding = egui::vec2(7.0, 2.0);
        style.spacing.interact_size.y = 24.0;

        let visuals = &mut style.visuals;
        visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
        visuals.selection.bg_fill = SELECTION;
        visuals.selection.stroke.color = SELECTION_TEXT;

        // Buttons and fields are white with an outline. egui's own idle look
        // is a gray fill for buttons and no outline at all for text fields
        // and checkboxes, which makes those disappear on the white cards.
        let widgets = &mut visuals.widgets;
        widgets.noninteractive.fg_stroke.color = TEXT;
        widgets.inactive.fg_stroke.color = TEXT;
        widgets.inactive.weak_bg_fill = Color32::WHITE;
        widgets.inactive.bg_stroke = Stroke::new(1.0, OUTLINE);
        widgets.hovered.weak_bg_fill = HOVER;
        widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_gray(150));
        widgets.active.weak_bg_fill = PRESSED;
        widgets.active.bg_stroke = Stroke::new(1.0, Color32::from_gray(110));
        widgets.open.weak_bg_fill = HOVER;
        for state in [
            &mut widgets.noninteractive,
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
            &mut widgets.open,
        ] {
            state.corner_radius = egui::CornerRadius::same(4);
        }
    });
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    // The system's UI font is easier to read at small sizes than egui's light
    // default, which remains the fallback for icons and for systems without
    // these files.
    let regular = load_font(&mut fonts, "regular", &[r"C:\Windows\Fonts\segoeui.ttf"]);
    let semibold = load_font(
        &mut fonts,
        "semibold",
        &[
            r"C:\Windows\Fonts\seguisb.ttf",
            r"C:\Windows\Fonts\segoeuib.ttf",
            r"C:\Windows\Fonts\arialbd.ttf",
        ],
    );

    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    if let Some(regular) = regular {
        proportional.insert(0, regular);
    }
    let mut semibold_fonts: Vec<String> = semibold.into_iter().collect();
    semibold_fonts.extend(proportional.iter().cloned());
    fonts
        .families
        .insert(semibold_font_family(), semibold_fonts);
    ctx.set_fonts(fonts);
}

/// Registers the first font file that can be read and returns its name.
fn load_font(fonts: &mut FontDefinitions, name: &str, paths: &[&str]) -> Option<String> {
    let bytes = paths.iter().find_map(|path| fs::read(path).ok())?;
    fonts.font_data.insert(
        name.to_owned(),
        std::sync::Arc::new(FontData::from_owned(bytes)),
    );
    Some(name.to_owned())
}

fn semibold_font_family() -> FontFamily {
    FontFamily::Name("Semibold".into())
}

fn semibold_font(size: f32) -> FontId {
    FontId::new(size, semibold_font_family())
}

/// Text that stands out by weight. `RichText::strong` only darkens it.
fn semibold(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).font(semibold_font(14.0))
}

/// Shows `text` with every digit in a slot of the same width.
///
/// The digits of the interface font differ in width. Drawn as ordinary text,
/// a running clock would change its width every second and push around
/// whatever is laid out next to it.
fn tabular_digits(ui: &mut egui::Ui, text: &str, font: FontId, color: Color32) -> egui::Response {
    // Whole pixels keep the slots evenly spaced after text is snapped to them.
    let pixels_per_point = ui.pixels_per_point();
    let whole_pixels = |width: f32| (width * pixels_per_point).ceil() / pixels_per_point;
    let (widths, height) = ui.fonts_mut(|fonts| {
        let digit_width = ('0'..='9')
            .map(|digit| fonts.glyph_width(&font, digit))
            .fold(0.0, f32::max);
        let widths: Vec<f32> = text
            .chars()
            .map(|character| {
                whole_pixels(if character.is_ascii_digit() {
                    digit_width
                } else {
                    fonts.glyph_width(&font, character)
                })
            })
            .collect();
        (widths, fonts.row_height(&font))
    });

    let size = egui::vec2(widths.iter().sum(), height);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let mut left = rect.left();
    for (character, width) in text.chars().zip(widths) {
        ui.painter().text(
            egui::pos2(left + width * 0.5, rect.center().y),
            Align2::CENTER_CENTER,
            character,
            font.clone(),
            color,
        );
        left += width;
    }
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), text));
    response
}

/// A caption and a duration at the right edge of a row.
fn total_label(ui: &mut egui::Ui, caption: &str, seconds: u64) {
    right_aligned(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let color = ui.visuals().text_color();
        tabular_digits(ui, &format_duration(seconds), semibold_font(14.0), color);
        ui.label(caption);
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppView {
    Entries,
    Projects,
    Activities,
    Settings,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
enum CalendarMode {
    #[default]
    Time,
    Projects,
    Activities,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct RememberedSelections {
    activity: ActivityId,
    project: Option<ProjectId>,
    calendar_mode: CalendarMode,
}

impl RememberedSelections {
    fn defaults_for(store: &Store) -> Self {
        Self {
            activity: ActivityId(store.activities.len().saturating_sub(1)),
            project: None,
            calendar_mode: CalendarMode::default(),
        }
    }

    fn validated_for(self, store: &Store) -> Self {
        let defaults = Self::defaults_for(store);
        Self {
            activity: if self.activity.0 < store.activities.len() {
                self.activity
            } else {
                defaults.activity
            },
            project: self
                .project
                .filter(|project| project.0 < store.projects.len()),
            calendar_mode: self.calendar_mode,
        }
    }
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
struct PersistedSettings {
    close_or_minimize_to_tray: bool,
    start_minimized_on_autorun: bool,
}

impl Default for PersistedSettings {
    fn default() -> Self {
        Self {
            close_or_minimize_to_tray: true,
            start_minimized_on_autorun: true,
        }
    }
}

#[derive(Clone)]
struct TimeEditRequest {
    entry_id: EntryId,
    endpoint: TimeEndpoint,
    text: String,
}

#[derive(Clone, Copy)]
struct DateEditRequest {
    entry_id: EntryId,
    date: NaiveDate,
}

struct ActiveStartEdit {
    original: UnixSeconds,
    time: String,
}

struct ActivityOption {
    id: ActivityId,
    name: String,
    color: Color32,
    total_seconds: u64,
}

struct ProjectOption {
    id: ProjectId,
    name: String,
    color: Color32,
    status: ProjectStatus,
}

struct CalendarContribution {
    label: String,
    detail: String,
    color: Color32,
    weight: f32,
}

struct TimeTrackerApp {
    repository: JsonStore,
    app_paths: AppPaths,
    store: Store,
    last_saved_store: Store,
    view: AppView,
    draft_activity: ActivityId,
    draft_project: Option<ProjectId>,
    message: String,
    new_project_name: String,
    new_project_color: [u8; 3],
    new_activity_name: String,
    new_activity_color: [u8; 3],
    time_edit_buffers: BTreeMap<(EntryId, TimeEndpoint), String>,
    active_start_edit: Option<ActiveStartEdit>,
    calendar: CalendarHeatmap,
    calendar_mode: CalendarMode,
    selected_date: Option<NaiveDate>,
    entry_pagination: Pagination,
    last_icon_running: Option<bool>,
    tray: Option<tray::TrayController>,
    exit_requested: bool,
    window_hidden: bool,
    preferences: PersistedSettings,
    autorun_enabled: bool,
    settings_message: String,
}

impl TimeTrackerApp {
    fn load(
        repository: JsonStore,
        store: Store,
        app_paths: AppPaths,
        storage: Option<&dyn eframe::Storage>,
        start_minimized: bool,
        ctx: &egui::Context,
    ) -> Self {
        let remembered = ctx
            .data_mut(|data| {
                data.get_persisted::<RememberedSelections>(egui::Id::new(UI_SELECTIONS_MEMORY_ID))
            })
            .map(|selections| selections.validated_for(&store))
            .unwrap_or_else(|| RememberedSelections::defaults_for(&store));
        let running = store.active_entry.is_some();
        let tray = tray::TrayController::new(running, {
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        });
        let preferences: PersistedSettings = storage
            .and_then(|storage| eframe::get_value(storage, "opto_tracker_settings"))
            .unwrap_or_default();
        let message = match &tray {
            Ok(_) => String::new(),
            Err(error) => format!("Could not create tray icon: {error}"),
        };
        let tray = tray.ok();
        let window_hidden = start_minimized && tray.is_some();
        if window_hidden {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        let autorun_enabled = desktop::autorun_enabled();
        if autorun_enabled {
            let _ = desktop::set_autorun(&app_paths, true, preferences.start_minimized_on_autorun);
        }
        Self {
            repository,
            app_paths,
            last_saved_store: store.clone(),
            store,
            view: AppView::Entries,
            draft_activity: remembered.activity,
            draft_project: remembered.project,
            message,
            new_project_name: String::new(),
            new_project_color: [104, 159, 56],
            new_activity_name: String::new(),
            new_activity_color: [79, 121, 203],
            time_edit_buffers: BTreeMap::new(),
            active_start_edit: None,
            calendar: CalendarHeatmap::default(),
            calendar_mode: remembered.calendar_mode,
            selected_date: None,
            entry_pagination: Pagination::new(ENTRIES_PER_PAGE),
            last_icon_running: None,
            tray,
            exit_requested: false,
            window_hidden,
            preferences,
            autorun_enabled,
            settings_message: String::new(),
        }
    }

    fn save(&mut self) {
        match self.repository.save(&self.store) {
            Ok(outcome) => {
                self.last_saved_store = self.store.clone();
                self.message = outcome
                    .backup_warning
                    .map(|warning| format!("Saved; backup warning: {warning}"))
                    .unwrap_or_else(|| "Saved".to_owned());
            }
            Err(error) => {
                self.store = self.last_saved_store.clone();
                self.time_edit_buffers.clear();
                self.active_start_edit = None;
                self.message = format!("Save failed; change was reverted: {error}");
            }
        }
    }

    fn remember_selections(&self, ctx: &egui::Context) {
        let id = egui::Id::new(UI_SELECTIONS_MEMORY_ID);
        let selections = RememberedSelections {
            activity: self.draft_activity,
            project: self.draft_project,
            calendar_mode: self.calendar_mode,
        };
        ctx.data_mut(|data| {
            if data.get_persisted::<RememberedSelections>(id) != Some(selections) {
                data.insert_persisted(id, selections);
            }
        });
    }

    fn start_timer(&mut self) {
        self.start_with(self.draft_activity, self.draft_project);
    }

    fn start_with(&mut self, activity: ActivityId, project: Option<ProjectId>) {
        if let Err(error) = self
            .store
            .start_timer(activity, project, UnixSeconds(now()))
        {
            self.message = error.to_string();
            return;
        }
        self.draft_activity = activity;
        self.draft_project = project;
        self.active_start_edit = None;
        self.save();
    }

    fn stop_timer(&mut self) {
        let Some(active) = self.store.active_entry.clone() else {
            return;
        };
        let outcome = match self.store.stop_timer(UnixSeconds(now())) {
            Ok(outcome) => outcome,
            Err(error) => {
                self.message = error.to_string();
                return;
            }
        };
        self.time_edit_buffers.clear();
        self.active_start_edit = None;
        self.draft_activity = active.activity;
        self.draft_project = active.project;
        self.save();
        if outcome.capped_at_limit && self.store.active_entry.is_none() {
            self.message = "Timer stopped at the 24-hour limit".to_owned();
        }
    }

    fn timer_panel(&mut self, ui: &mut egui::Ui) {
        let activities = activity_options(&self.store, now());
        let projects = project_options(&self.store);
        let layout = RowLayout::new(
            ui.available_width(),
            TIMER_CONTROLS_WIDTH,
            ui.spacing().item_spacing.x,
        );
        let can_start = !activities.is_empty();
        let mut active_changed = false;
        row(ui, TIMER_HEIGHT, |ui| {
            active_changed =
                self.timer_selectors(ui, &activities, &projects, layout.selector_width);
            if !layout.two_lines {
                right_aligned(ui, |ui| self.timer_controls(ui, can_start));
            }
        });
        if layout.two_lines {
            row(ui, TIMER_HEIGHT, |ui| {
                right_aligned(ui, |ui| self.timer_controls(ui, can_start));
            });
        }
        if active_changed {
            self.save();
        }
    }

    /// The activity and project of the running timer, or of the next one.
    /// Returns whether the running timer was changed.
    fn timer_selectors(
        &mut self,
        ui: &mut egui::Ui,
        activities: &[ActivityOption],
        projects: &[ProjectOption],
        width: f32,
    ) -> bool {
        let size = egui::vec2(width, TIMER_HEIGHT - 2.0);
        let (activity, project) = match self.store.active_entry.as_mut() {
            Some(active) => (&mut active.activity, &mut active.project),
            None => (&mut self.draft_activity, &mut self.draft_project),
        };
        let place = SelectorPlace::Timer;
        let changed = activity_selector(ui, "timer-activity", activity, activities, size, place)
            | project_selector(ui, "timer-project", project, projects, size, place);
        changed && self.store.active_entry.is_some()
    }

    /// Laid out from the right: the button, the clock and the start time.
    fn timer_controls(&mut self, ui: &mut egui::Ui, can_start: bool) {
        let started_at = self
            .store
            .active_entry
            .as_ref()
            .map(|active| active.started_at.0);
        let button = |text: &str, fill: Color32| {
            Button::new(semibold(text).color(Color32::WHITE))
                .fill(fill)
                .stroke(Stroke::NONE)
                .min_size(egui::vec2(TIMER_BUTTON_WIDTH, TIMER_HEIGHT - 2.0))
        };
        if started_at.is_some() {
            if ui.add(button("STOP", RED)).clicked() {
                self.stop_timer();
            }
        } else if ui.add_enabled(can_start, button("START", BLUE)).clicked() {
            self.start_timer();
        }

        let (elapsed, color) = match started_at {
            Some(started_at) => (
                (now().checked_sub(started_at).unwrap_or(0).max(0) as u64)
                    .min(MAX_ENTRY_SECONDS as u64),
                ui.visuals().text_color(),
            ),
            None => (0, ui.visuals().weak_text_color()),
        };
        ui.add_space(6.0);
        tabular_digits(ui, &format_duration(elapsed), semibold_font(24.0), color);
        ui.add_space(6.0);
        self.active_start_field(ui);
    }

    fn active_start_field(&mut self, ui: &mut egui::Ui) {
        let Some(started_at) = self
            .store
            .active_entry
            .as_ref()
            .map(|entry| entry.started_at)
        else {
            self.active_start_edit = None;
            return;
        };
        if self
            .active_start_edit
            .as_ref()
            .is_none_or(|edit| edit.original != started_at)
        {
            let local = local_datetime(started_at.0);
            self.active_start_edit = Some(ActiveStartEdit {
                original: started_at,
                time: local.format("%H:%M").to_string(),
            });
        }

        let edit = self.active_start_edit.as_mut().expect("initialized above");
        let response = ui
            .add(
                egui::TextEdit::singleline(&mut edit.time)
                    .id_salt("active-start-time")
                    .desired_width(TIME_FIELD_WIDTH)
                    .horizontal_align(Align::Center),
            )
            .on_hover_text("Start time (HH:MM); later times count as yesterday");
        let shown = local_datetime(started_at.0).format("%H:%M").to_string();
        let commit = response.lost_focus()
            || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
        if commit && edit.time != shown {
            let current_time = UnixSeconds(now());
            let result = local_time::edit_active_start(started_at, current_time, &edit.time)
                .and_then(|new_start| {
                    self.store
                        .set_timer_start(new_start, current_time)
                        .map_err(|error| error.to_string())
                });
            match result {
                Ok(()) => {
                    self.active_start_edit = None;
                    self.save();
                }
                Err(error) => {
                    self.active_start_edit = None;
                    self.message = error;
                }
            }
        }
    }

    fn entries_panel(&mut self, ui: &mut egui::Ui) {
        let entry_count = self.store.time_entries.len();
        let current_time = now();
        let activities = activity_options(&self.store, current_time);
        let projects = project_options(&self.store);
        let day_totals = totals_by_local_day(&self.store, current_time);
        let project_contributions: BTreeMap<NaiveDate, Vec<CalendarContribution>> = day_totals
            .iter()
            .map(|(&date, day)| {
                let segments = day
                    .by_project
                    .iter()
                    .filter(|(_, &seconds)| seconds > 0)
                    .map(|(&project, &seconds)| {
                        let (label, segment_color) = project
                            .and_then(|index| self.store.projects.get(index))
                            .map(|project| (project.name.clone(), color(project.color)))
                            .unwrap_or_else(|| ("No project".to_owned(), NO_PROJECT_COLOR));
                        CalendarContribution {
                            label,
                            detail: format_duration(seconds),
                            color: segment_color,
                            weight: seconds as f32,
                        }
                    })
                    .collect();
                (date, segments)
            })
            .collect();
        let project_segments: BTreeMap<NaiveDate, Vec<Segment>> = project_contributions
            .iter()
            .map(|(&date, contributions)| {
                (
                    date,
                    contributions
                        .iter()
                        .map(|contribution| Segment {
                            color: contribution.color,
                            weight: contribution.weight,
                        })
                        .collect(),
                )
            })
            .collect();
        let activity_contributions: BTreeMap<NaiveDate, Vec<CalendarContribution>> = day_totals
            .iter()
            .map(|(&date, day)| {
                let contributions = day
                    .by_activity
                    .iter()
                    .filter(|(_, &seconds)| seconds > 0)
                    .filter_map(|(&index, &seconds)| {
                        self.store
                            .activities
                            .get(index)
                            .map(|activity| CalendarContribution {
                                label: activity.name.clone(),
                                detail: format_duration(seconds),
                                color: color(activity.color),
                                weight: seconds as f32,
                            })
                    })
                    .collect();
                (date, contributions)
            })
            .collect();
        let activity_segments: BTreeMap<NaiveDate, Vec<Segment>> = activity_contributions
            .iter()
            .map(|(&date, contributions)| {
                (
                    date,
                    contributions
                        .iter()
                        .map(|contribution| Segment {
                            color: contribution.color,
                            weight: contribution.weight,
                        })
                        .collect(),
                )
            })
            .collect();
        let week_totals = totals_by_week(&day_totals);
        let today = Local::now().date_naive();
        let this_week = week_start(today);
        let mut changed = false;
        let mut delete = None;
        let mut start_request = None;
        let mut time_commits = Vec::new();
        let mut date_commits = Vec::new();
        let mut current_week: Option<NaiveDate> = None;
        let can_start = self.store.active_entry.is_none();

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .scroll_bar_visibility(
                egui::containers::scroll_area::ScrollBarVisibility::AlwaysVisible,
            )
            .show(ui, |ui| {
                let calendar_change = card(ui, |ui| {
                    // The controls sit above the cells, which are centered
                    // in a wide window. Their extent is known from the
                    // previous frame.
                    let mut controls = ui.available_rect_before_wrap();
                    let cells_x = self.calendar.cells_x_range();
                    if cells_x.span() > 0.0 {
                        controls = controls.intersect(egui::Rect::everything_right_of(cells_x.min));
                        controls = controls.intersect(egui::Rect::everything_left_of(cells_x.max));
                    }
                    controls.set_height(ui.spacing().interact_size.y);
                    ui.scope_builder(egui::UiBuilder::new().max_rect(controls), |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("⏴").on_hover_text("Earlier").clicked() {
                                self.calendar.scroll_weeks(CALENDAR_PAGE_WEEKS);
                            }
                            if ui.button("⏵").on_hover_text("Later").clicked() {
                                self.calendar.scroll_weeks(-CALENDAR_PAGE_WEEKS);
                            }
                            if self.calendar.is_scrolled() && ui.button("This year").clicked() {
                                self.calendar.reset_scroll();
                            }
                            right_aligned(ui, |ui| {
                                frameless_until_hovered(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.calendar_mode,
                                        CalendarMode::Activities,
                                        "Activities",
                                    );
                                    ui.selectable_value(
                                        &mut self.calendar_mode,
                                        CalendarMode::Projects,
                                        "Projects",
                                    );
                                    ui.selectable_value(
                                        &mut self.calendar_mode,
                                        CalendarMode::Time,
                                        "Time",
                                    );
                                });
                            });
                        });
                    });
                    let calendar_mode = self.calendar_mode;
                    let change = self.calendar.ui(
                        ui,
                        today,
                        self.selected_date,
                        |date| match calendar_mode {
                            CalendarMode::Time => day_totals
                                .get(&date)
                                .map(|day| {
                                    CellFill::Solid(heat_color(
                                        day.seconds as f32 / FULL_DAY_SECONDS,
                                    ))
                                })
                                .unwrap_or(CellFill::Empty),
                            CalendarMode::Projects => project_segments
                                .get(&date)
                                .map(|segments| CellFill::Segmented(segments))
                                .unwrap_or(CellFill::Empty),
                            CalendarMode::Activities => activity_segments
                                .get(&date)
                                .map(|segments| CellFill::Segmented(segments))
                                .unwrap_or(CellFill::Empty),
                        },
                        |ui, date| {
                            ui.set_min_width(220.0);
                            ui.label(local_time::format_date_with_weekday(date));
                            ui.label(
                                day_totals
                                    .get(&date)
                                    .map(|day| format_duration(day.seconds))
                                    .unwrap_or_else(|| "No time tracked".to_owned()),
                            );
                            if calendar_mode != CalendarMode::Time {
                                let contributions = match calendar_mode {
                                    CalendarMode::Projects => project_contributions.get(&date),
                                    CalendarMode::Activities => activity_contributions.get(&date),
                                    CalendarMode::Time => None,
                                };
                                if let Some(contributions) = contributions {
                                    ui.separator();
                                    for contribution in contributions {
                                        ui.horizontal(|ui| {
                                            let (swatch, _) = ui.allocate_exact_size(
                                                egui::vec2(10.0, 10.0),
                                                egui::Sense::hover(),
                                            );
                                            ui.painter().rect_filled(
                                                swatch,
                                                2.0,
                                                contribution.color,
                                            );
                                            ui.label(&contribution.label);
                                            right_aligned(ui, |ui| {
                                                ui.label(&contribution.detail);
                                            });
                                        });
                                    }
                                }
                            }
                        },
                    );

                    let shown = day_totals
                        .range(self.calendar.visible_days(today))
                        .map(|(_, day)| day);
                    let legend = match calendar_mode {
                        CalendarMode::Time => Vec::new(),
                        CalendarMode::Projects => {
                            legend_items(shown.flat_map(|day| &day.by_project), |project| {
                                Some(
                                    match project.and_then(|index| self.store.projects.get(index)) {
                                        Some(project) => (
                                            Swatch::Project(color(project.color)),
                                            project.name.clone(),
                                        ),
                                        None => (
                                            Swatch::Project(NO_PROJECT_COLOR),
                                            "No project".to_owned(),
                                        ),
                                    },
                                )
                            })
                        }
                        CalendarMode::Activities => {
                            legend_items(shown.flat_map(|day| &day.by_activity), |activity| {
                                self.store.activities.get(activity).map(|activity| {
                                    (
                                        Swatch::Activity(color(activity.color)),
                                        activity.name.clone(),
                                    )
                                })
                            })
                        }
                    };
                    calendar_legend(ui, self.calendar.cells_x_range(), calendar_mode, &legend);
                    change
                });
                match calendar_change {
                    CalendarSelectionChange::Select(date) => {
                        self.selected_date = Some(date);
                        self.entry_pagination
                            .set_start(entry_offset_for_date(&self.store.time_entries, date));
                    }
                    CalendarSelectionChange::Clear => {
                        self.selected_date = None;
                        self.entry_pagination.set_start(0);
                    }
                    CalendarSelectionChange::Unchanged => {}
                }
                ui.add_space(8.0);
                pagination_controls(ui, &mut self.entry_pagination, entry_count);

                let visible_entries = self.entry_pagination.visible_range(entry_count);
                let mut entries_by_day: BTreeMap<NaiveDate, Vec<usize>> = BTreeMap::new();
                for index in (0..self.store.time_entries.len())
                    .rev()
                    .skip(visible_entries.start)
                    .take(visible_entries.len())
                {
                    let entry = &self.store.time_entries[index];
                    let date = local_datetime(entry.time.started_at.0).date_naive();
                    entries_by_day.entry(date).or_default().push(index);
                    let mut split = BTreeMap::new();
                    add_interval_to_days(
                        &mut split,
                        entry.time.started_at.0,
                        entry.time.stopped_at.0,
                    );
                    for date in split.into_keys() {
                        entries_by_day.entry(date).or_default();
                    }
                }
                let display_days: Vec<NaiveDate> = entries_by_day.keys().copied().rev().collect();

                for date in display_days {
                    let entry_week = week_start(date);
                    if current_week != Some(entry_week) {
                        current_week = Some(entry_week);
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            section_title(ui, &week_title(entry_week, this_week));
                            total_label(
                                ui,
                                "Week total:",
                                week_totals.get(&entry_week).copied().unwrap_or(0),
                            );
                        });
                    }
                    ui.add_space(4.0);
                    let indices = entries_by_day.get(&date).cloned().unwrap_or_default();
                    // One card per day: a header, then the entries divided by hairlines.
                    card_frame().show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 0.0;
                        Frame::new()
                            .fill(HEADER)
                            .corner_radius(egui::CornerRadius {
                                nw: 4,
                                ne: 4,
                                sw: 0,
                                se: 0,
                            })
                            .inner_margin(egui::Margin::symmetric(10, 5))
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(semibold(day_title(date, today)));
                                    total_label(
                                        ui,
                                        "Day total:",
                                        day_totals.get(&date).map_or(0, |day| day.seconds),
                                    );
                                });
                            });
                        if indices.is_empty() {
                            hairline(ui);
                            entry_frame().show(ui, |ui| {
                                ui.label(
                                    RichText::new("Time carried over from an overnight entry")
                                        .weak(),
                                );
                            });
                        }
                        for index in indices {
                            hairline(ui);
                            let entry = &mut self.store.time_entries[index];
                            let entry_id = entry.id;
                            let highlight = ui.painter().add(egui::Shape::Noop);
                            let frame = entry_frame().show(ui, |ui| {
                                let layout = RowLayout::new(
                                    ui.available_width(),
                                    ENTRY_CONTROLS_WIDTH,
                                    ui.spacing().item_spacing.x,
                                );
                                let mut controls = None;
                                row(ui, ROW_HEIGHT, |ui| {
                                    changed |= entry_selectors(
                                        ui,
                                        entry,
                                        &activities,
                                        &projects,
                                        layout.selector_width,
                                    );
                                    if !layout.two_lines {
                                        controls = Some(right_aligned(ui, |ui| {
                                            entry_controls(
                                                ui,
                                                entry,
                                                &mut self.time_edit_buffers,
                                                can_start,
                                            )
                                        }));
                                    }
                                });
                                if layout.two_lines {
                                    row(ui, ROW_HEIGHT, |ui| {
                                        controls = Some(right_aligned(ui, |ui| {
                                            entry_controls(
                                                ui,
                                                entry,
                                                &mut self.time_edit_buffers,
                                                can_start,
                                            )
                                        }));
                                    });
                                }
                                if let Some(controls) = controls {
                                    if controls.delete {
                                        delete = Some(entry_id);
                                    }
                                    if controls.restart {
                                        start_request = Some((entry.activity, entry.project));
                                    }
                                    time_commits.extend(controls.time_edits);
                                    date_commits.extend(
                                        controls
                                            .date
                                            .map(|date| DateEditRequest { entry_id, date }),
                                    );
                                }
                            });
                            let row_rect = frame.response.rect;
                            if ui.rect_contains_pointer(row_rect) {
                                ui.painter().set(
                                    highlight,
                                    egui::Shape::rect_filled(row_rect, 0.0, ROW_HOVER),
                                );
                            }
                        }
                    });
                }

                ui.add_space(10.0);
                pagination_controls(ui, &mut self.entry_pagination, entry_count);
            });
        if let Some(entry_id) = delete {
            self.store.remove_entry(entry_id);
            self.time_edit_buffers.clear();
            changed = true;
        }
        if changed {
            self.save();
        }
        for edit in time_commits {
            if delete != Some(edit.entry_id) {
                self.apply_inline_time_edit(edit);
            }
        }
        for edit in date_commits {
            if delete != Some(edit.entry_id) {
                self.apply_date_edit(edit);
            }
        }
        if let Some((activity, project)) = start_request {
            self.start_with(activity, project);
        }
    }

    fn apply_inline_time_edit(&mut self, edit: TimeEditRequest) {
        let result = self
            .store
            .entry(edit.entry_id)
            .cloned()
            .ok_or_else(|| "The entry no longer exists".to_owned())
            .and_then(|entry| local_time::edit_time(entry.time, edit.endpoint, &edit.text));
        match result {
            Ok(time) => {
                if let Some(entry) = self.store.entry_mut(edit.entry_id) {
                    entry.time = time;
                    self.store
                        .time_entries
                        .sort_by_key(|entry| entry.time.started_at);
                    self.time_edit_buffers.clear();
                    self.save();
                }
            }
            Err(error) => {
                self.message = error;
                self.time_edit_buffers
                    .remove(&(edit.entry_id, edit.endpoint));
            }
        }
    }

    fn apply_date_edit(&mut self, edit: DateEditRequest) {
        let result = self
            .store
            .entry(edit.entry_id)
            .cloned()
            .ok_or_else(|| "The entry no longer exists".to_owned())
            .and_then(|entry| local_time::edit_date(entry.time, edit.date));
        match result {
            Ok(time) => {
                if let Some(entry) = self.store.entry_mut(edit.entry_id) {
                    entry.time = time;
                    self.store
                        .time_entries
                        .sort_by_key(|entry| entry.time.started_at);
                    self.time_edit_buffers.clear();
                    self.save();
                }
            }
            Err(error) => self.message = error,
        }
    }

    fn projects_panel(&mut self, ui: &mut egui::Ui) {
        let mut add_clicked = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_project_name)
                    .hint_text("New project")
                    .desired_width(220.0),
            );
            ui.color_edit_button_srgb(&mut self.new_project_color)
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            add_clicked = ui.button("Add").clicked();
            right_aligned(ui, |ui| {
                count_label(ui, format!("{} projects", self.store.projects.len()));
            });
        });
        if add_clicked {
            let name = self.new_project_name.trim();
            if name.is_empty() {
                self.message = "Project name cannot be empty".to_owned();
            } else {
                self.store.projects.push(Project {
                    name: name.to_owned(),
                    color: RgbColor {
                        red: self.new_project_color[0],
                        green: self.new_project_color[1],
                        blue: self.new_project_color[2],
                    },
                    status: ProjectStatus::Active,
                });
                self.draft_project = Some(ProjectId(self.store.projects.len() - 1));
                self.new_project_name.clear();
                self.save();
            }
        }

        let current_time = now();
        let stats: Vec<UsageStats> = (0..self.store.projects.len())
            .map(|index| project_stats(&self.store, ProjectId(index), current_time))
            .collect();
        let largest_total = largest_total(&stats);
        let mut changed = false;
        ui.add_space(8.0);
        usage_table(
            ui,
            "projects",
            &["", "Project", "Status"],
            "Activities",
            |ui| {
                for index in (0..self.store.projects.len()).rev() {
                    let project = &mut self.store.projects[index];
                    changed |= color_cell(ui, &mut project.color);
                    let name_color = match project.status {
                        ProjectStatus::Active => ui.visuals().text_color(),
                        ProjectStatus::Finished => ui.visuals().weak_text_color(),
                    };
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut project.name)
                                .min_size(egui::vec2(NAME_FIELD_WIDTH, 0.0))
                                .desired_width(NAME_FIELD_WIDTH)
                                .font(semibold_font(14.0))
                                .text_color(name_color),
                        )
                        .changed();
                    changed |= project_status_combo(ui, index, &mut project.status);
                    usage_cells(ui, &stats[index], largest_total, color(project.color));
                    ui.end_row();
                }
            },
        );
        if changed {
            self.save();
        }
    }

    fn activities_panel(&mut self, ui: &mut egui::Ui) {
        let mut add_clicked = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_activity_name)
                    .hint_text("New activity")
                    .desired_width(220.0),
            );
            ui.color_edit_button_srgb(&mut self.new_activity_color)
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            add_clicked = ui.button("Add").clicked();
            right_aligned(ui, |ui| {
                count_label(ui, format!("{} activities", self.store.activities.len()));
            });
        });
        if add_clicked {
            let name = self.new_activity_name.trim();
            if name.is_empty() {
                self.message = "Activity name cannot be empty".to_owned();
            } else {
                self.store.activities.push(Activity {
                    name: name.to_owned(),
                    color: RgbColor {
                        red: self.new_activity_color[0],
                        green: self.new_activity_color[1],
                        blue: self.new_activity_color[2],
                    },
                });
                self.draft_activity = ActivityId(self.store.activities.len() - 1);
                self.new_activity_name.clear();
                self.save();
            }
        }

        let current_time = now();
        let stats: Vec<UsageStats> = (0..self.store.activities.len())
            .map(|index| activity_stats(&self.store, ActivityId(index), current_time))
            .collect();
        let mut activity_order: Vec<usize> = (0..self.store.activities.len()).collect();
        activity_order.sort_by(|left, right| {
            stats[*right]
                .total_seconds
                .cmp(&stats[*left].total_seconds)
                .then_with(|| right.cmp(left))
        });
        let largest_total = largest_total(&stats);
        let mut changed = false;
        ui.add_space(8.0);
        usage_table(ui, "activities", &["", "Activity"], "Projects", |ui| {
            for index in activity_order {
                let activity = &mut self.store.activities[index];
                changed |= color_cell(ui, &mut activity.color);
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut activity.name)
                            .min_size(egui::vec2(NAME_FIELD_WIDTH, 0.0))
                            .desired_width(NAME_FIELD_WIDTH),
                    )
                    .changed();
                usage_cells(ui, &stats[index], largest_total, color(activity.color));
                ui.end_row();
            }
        });
        if changed {
            self.save();
        }
    }

    fn settings_panel(&mut self, ui: &mut egui::Ui) {
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.settings_contents(ui));
    }

    fn settings_contents(&mut self, ui: &mut egui::Ui) {
        if !self.settings_message.is_empty() {
            ui.label(RichText::new(&self.settings_message).color(BLUE));
            ui.add_space(6.0);
        }

        card(ui, |ui| {
            section_title(ui, "Application folder");
            ui.label(RichText::new(self.app_paths.directory.display().to_string()).monospace());
            ui.label(
                RichText::new(
                    "The installed executable and UI persistence are stored in this folder.",
                )
                .color(Color32::from_gray(105)),
            );
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(if desktop::is_installed(&self.app_paths) {
                        "Update installed app"
                    } else {
                        "Install OptoTracker here"
                    })
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    match desktop::install_current_executable(&self.app_paths) {
                        Ok(()) => {
                            self.settings_message = "OptoTracker was installed.".to_owned();
                            if self.autorun_enabled {
                                if let Err(error) = desktop::set_autorun(
                                    &self.app_paths,
                                    true,
                                    self.preferences.start_minimized_on_autorun,
                                ) {
                                    self.settings_message = error;
                                }
                            }
                        }
                        Err(error) => self.settings_message = error,
                    }
                }
                if ui
                    .button("Open folder")
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    if let Err(error) = desktop::open_app_directory(&self.app_paths) {
                        self.settings_message = error;
                    }
                }
                if ui
                    .button("Create desktop shortcut")
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    match desktop::create_desktop_shortcut(&self.app_paths) {
                        Ok(()) => {
                            self.settings_message = "Desktop shortcut created.".to_owned();
                        }
                        Err(error) => self.settings_message = error,
                    }
                }
            });
            ui.add_space(10.0);
            if desktop::is_installed(&self.app_paths) {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Installed executable:");
                    ui.label(RichText::new(paths::EXECUTABLE_FILE).monospace());
                });
            } else {
                ui.label("OptoTracker is not installed in this folder yet.");
            }
            ui.horizontal_wrapped(|ui| {
                ui.label("Data:");
                ui.label(RichText::new(self.repository.path().display().to_string()).monospace());
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Persistence:");
                ui.label(
                    RichText::new(self.app_paths.persistence.display().to_string()).monospace(),
                );
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Weekly backups:");
                ui.label(RichText::new("backups").monospace());
            });
        });

        ui.add_space(8.0);
        card(ui, |ui| {
            section_title(ui, "Windows behavior");
            ui.add_space(4.0);

            let mut autorun = self.autorun_enabled;
            if ui
                .checkbox(&mut autorun, "Start OptoTracker when I sign in")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .changed()
            {
                let result = if autorun {
                    desktop::install_current_executable(&self.app_paths).and_then(|()| {
                        desktop::set_autorun(
                            &self.app_paths,
                            true,
                            self.preferences.start_minimized_on_autorun,
                        )
                    })
                } else {
                    desktop::set_autorun(&self.app_paths, false, false)
                };
                match result {
                    Ok(()) => {
                        self.autorun_enabled = autorun;
                        self.settings_message = if autorun {
                            "Windows autorun enabled.".to_owned()
                        } else {
                            "Windows autorun disabled".to_owned()
                        };
                    }
                    Err(error) => self.settings_message = error,
                }
            }
            let minimized_changed = ui
                .checkbox(
                    &mut self.preferences.start_minimized_on_autorun,
                    "Start minimized to tray on autorun",
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .changed();
            if minimized_changed && self.autorun_enabled {
                match desktop::set_autorun(
                    &self.app_paths,
                    true,
                    self.preferences.start_minimized_on_autorun,
                ) {
                    Ok(()) => {
                        self.settings_message = "Autorun behavior updated.".to_owned();
                    }
                    Err(error) => self.settings_message = error,
                }
            }

            ui.add_space(6.0);
            ui.add_enabled(
                self.tray.is_some(),
                egui::Checkbox::new(
                    &mut self.preferences.close_or_minimize_to_tray,
                    "Send OptoTracker to the tray when closed or minimized",
                ),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
            ui.label(
                RichText::new(
                    "When disabled, minimizing behaves normally and closing exits the app.",
                )
                .small()
                .color(Color32::from_gray(105)),
            );
        });
    }

    fn handle_window_and_tray(&mut self, ctx: &egui::Context) {
        if let Some(action) = self.tray.as_ref().and_then(|tray| tray.poll_action()) {
            match action {
                tray::TrayAction::Show => {
                    self.window_hidden = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                tray::TrayAction::Exit => {
                    self.exit_requested = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        let (close_requested, minimized) = ctx.input(|input| {
            (
                input.viewport().close_requested(),
                input.viewport().minimized == Some(true),
            )
        });
        if self.preferences.close_or_minimize_to_tray && self.tray.is_some() && !self.exit_requested
        {
            if close_requested {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.window_hidden = true;
            } else if minimized && !self.window_hidden {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.window_hidden = true;
            }
        }
    }
}

impl eframe::App for TimeTrackerApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let until_limit = self.store.active_entry.as_ref().and_then(|active| {
            now()
                .checked_sub(active.started_at.0)
                .map(|elapsed| MAX_ENTRY_SECONDS.saturating_sub(elapsed))
        });
        match until_limit {
            Some(remaining) if remaining <= 0 => self.stop_timer(),
            // A hidden or minimized window is otherwise never repainted, so
            // ask to be woken when the running timer reaches its limit.
            Some(remaining) => ctx.request_repaint_after(Duration::from_secs(remaining as u64)),
            None => {}
        }
        self.handle_window_and_tray(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let running = self.store.active_entry.is_some();
        if self.last_icon_running != Some(running) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Icon(Some(std::sync::Arc::new(
                    app_icon::icon_data(running),
                ))));
            app_icon::set_taskbar_running(frame, running);
            if let Some(tray) = &self.tray {
                if let Err(error) = tray.set_running(running) {
                    self.message = format!("Could not update tray icon: {error}");
                }
            }
            self.last_icon_running = Some(running);
        }
        if running && !self.window_hidden {
            // Keep the displayed running time ticking while it can be seen.
            ui.ctx().request_repaint_after(until_next_second());
        }

        egui::Panel::top("top-bar")
            .frame(
                Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ui, |ui| {
                row(ui, ROW_HEIGHT, |ui| {
                    let (icon_rect, _) =
                        ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                    app_icon::paint(ui.painter(), icon_rect, running);
                    frameless_until_hovered(ui, |ui| {
                        ui.selectable_value(&mut self.view, AppView::Entries, "Time entries");
                        ui.selectable_value(&mut self.view, AppView::Projects, "Projects");
                        ui.selectable_value(&mut self.view, AppView::Activities, "Activities");
                        ui.selectable_value(&mut self.view, AppView::Settings, "Settings");
                    });
                    right_aligned(ui, |ui| {
                        if !self.message.is_empty() {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&self.message).color(Color32::from_gray(110)),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&self.message);
                        }
                    });
                });
            });
        if self.view == AppView::Entries {
            egui::Panel::top("timer")
                .frame(
                    Frame::new()
                        .fill(Color32::WHITE)
                        .inner_margin(egui::Margin::symmetric(12, 8)),
                )
                .show(ui, |ui| self.timer_panel(ui));
        }
        egui::CentralPanel::default()
            .frame(Frame::new().fill(PAGE).inner_margin(12.0))
            .show(ui, |ui| {
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                match self.view {
                    AppView::Entries => self.entries_panel(ui),
                    AppView::Projects => self.projects_panel(ui),
                    AppView::Activities => self.activities_panel(ui),
                    AppView::Settings => self.settings_panel(ui),
                }
            });
        self.remember_selections(ui.ctx());
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "opto_tracker_settings", &self.preferences);
    }

    fn auto_save_interval(&self) -> Duration {
        Duration::from_secs(5)
    }
}

fn pagination_controls(ui: &mut egui::Ui, pagination: &mut Pagination, total_entries: usize) {
    let visible = pagination.visible_range(total_entries);
    let first = if total_entries == 0 {
        0
    } else {
        visible.start + 1
    };
    let last = visible.end;

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{first}–{last} of {total_entries}"))
                .color(Color32::from_gray(105)),
        );
        right_aligned(ui, |ui| {
            if ui
                .add_enabled(pagination.has_next(total_entries), Button::new("Next"))
                .clicked()
            {
                pagination.next(total_entries);
            }
            if ui
                .add_enabled(pagination.has_previous(), Button::new("Previous"))
                .clicked()
            {
                pagination.previous();
            }
        });
    });
}

/// Returns the number of newest-first entries to skip so the list begins at
/// the newest entry on `date`, the nearest older entry, or the oldest entry if
/// the selected date predates the entire history.
fn entry_offset_for_date(entries: &[model::TimeEntry], date: NaiveDate) -> usize {
    let offset = entries
        .iter()
        .rev()
        .take_while(|entry| local_datetime(entry.time.started_at.0).date_naive() > date)
        .count();
    offset.min(entries.len().saturating_sub(1))
}

fn activity_options(store: &Store, now: i64) -> Vec<ActivityOption> {
    let totals = activity_totals(store, now);
    let mut options: Vec<_> = store
        .activities
        .iter()
        .enumerate()
        .map(|(index, activity)| ActivityOption {
            id: ActivityId(index),
            name: activity.name.clone(),
            color: color(activity.color),
            total_seconds: totals[index],
        })
        .collect();
    options.sort_by(|left, right| {
        right
            .total_seconds
            .cmp(&left.total_seconds)
            .then_with(|| right.id.0.cmp(&left.id.0))
    });
    options
}

fn project_options(store: &Store) -> Vec<ProjectOption> {
    store
        .projects
        .iter()
        .enumerate()
        .map(|(index, project)| ProjectOption {
            id: ProjectId(index),
            name: project.name.clone(),
            color: color(project.color),
            status: project.status,
        })
        .collect()
}

fn activity_selector(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut ActivityId,
    activities: &[ActivityOption],
    size: egui::Vec2,
    place: SelectorPlace,
) -> bool {
    let before = *value;
    let (swatch, text) = activities
        .iter()
        .find(|activity| activity.id == *value)
        .map(|activity| {
            (
                Swatch::Activity(activity.color),
                RichText::new(&activity.name),
            )
        })
        .unwrap_or_else(|| (Swatch::Empty, RichText::new("Choose activity").weak()));
    let style = match place {
        SelectorPlace::Timer => SelectorStyle::Button,
        SelectorPlace::Entry => SelectorStyle::Quiet,
    };
    selector(ui, id, swatch, text, size, style, |ui| {
        for activity in activities {
            let selected = *value == activity.id;
            let swatch = Swatch::Activity(activity.color);
            if swatch_item(ui, selected, swatch, RichText::new(&activity.name)).clicked() {
                *value = activity.id;
            }
        }
    });
    *value != before
}

fn project_selector(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut Option<ProjectId>,
    projects: &[ProjectOption],
    size: egui::Vec2,
    place: SelectorPlace,
) -> bool {
    let before = *value;
    let no_project = || RichText::new("No project").weak();
    let current = value.and_then(|id| projects.iter().find(|project| project.id == id));
    let (swatch, text) = current
        .map(|project| (Swatch::Project(project.color), semibold(&project.name)))
        .unwrap_or_else(|| (Swatch::Empty, no_project()));
    let style = match (place, current) {
        (SelectorPlace::Timer, _) => SelectorStyle::Button,
        (SelectorPlace::Entry, Some(project)) => SelectorStyle::Tag(project.color),
        (SelectorPlace::Entry, None) => SelectorStyle::Quiet,
    };
    selector(ui, id, swatch, text, size, style, |ui| {
        let mut items = |ui: &mut egui::Ui, status: ProjectStatus| {
            for project in projects
                .iter()
                .rev()
                .filter(|project| project.status == status)
            {
                let selected = *value == Some(project.id);
                let swatch = Swatch::Project(project.color);
                if swatch_item(ui, selected, swatch, semibold(&project.name)).clicked() {
                    *value = Some(project.id);
                }
            }
        };
        items(ui, ProjectStatus::Active);
        if projects
            .iter()
            .any(|project| project.status == ProjectStatus::Finished)
        {
            ui.separator();
            items(ui, ProjectStatus::Finished);
        }
        ui.separator();
        if swatch_item(ui, value.is_none(), Swatch::Empty, no_project()).clicked() {
            *value = None;
        }
    });
    *value != before
}

/// The color marker drawn in front of an activity or project name. The
/// names themselves keep the normal text color, so that a light color does
/// not make them unreadable.
#[derive(Clone, Copy)]
enum Swatch {
    Activity(Color32),
    Project(Color32),
    Empty,
}

impl Swatch {
    const SIZE: f32 = 10.0;

    fn id() -> egui::Id {
        egui::Id::new("swatch")
    }

    fn paint(self, painter: &egui::Painter, rect: egui::Rect) {
        match self {
            Self::Activity(color) => {
                painter.circle_filled(rect.center(), rect.width() * 0.5, color);
            }
            Self::Project(color) => {
                painter.rect_filled(rect, 2.0, color);
            }
            Self::Empty => {
                painter.rect_stroke(
                    rect,
                    2.0,
                    Stroke::new(1.0, Color32::from_gray(175)),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }
}

/// A swatch followed by `text`. A growing text fills the button and stays
/// left-aligned in it.
fn swatch_atoms<'a>(text: RichText, grow: bool) -> Atoms<'a> {
    Atoms::new((
        Atom::custom(Swatch::id(), egui::Vec2::splat(Swatch::SIZE)),
        text.atom_grow(grow).atom_align(Align2::LEFT_CENTER),
    ))
}

/// Shows a button built from [`swatch_atoms`] and paints its swatch.
fn swatch_button(ui: &mut egui::Ui, swatch: Swatch, button: Button<'_>) -> egui::Response {
    let response = button.atom_ui(ui);
    if let Some(rect) = response.rect(Swatch::id()) {
        swatch.paint(ui.painter(), rect);
    }
    response.response
}

fn swatch_item(
    ui: &mut egui::Ui,
    selected: bool,
    swatch: Swatch,
    text: RichText,
) -> egui::Response {
    let atoms = swatch_atoms(text, true);
    swatch_button(ui, swatch, Button::selectable(selected, atoms))
}

#[derive(Clone, Copy)]
enum SelectorPlace {
    Timer,
    Entry,
}

#[derive(Clone, Copy)]
enum SelectorStyle {
    /// A regular button with a drop-down arrow.
    Button,
    /// Bare text that gets a button frame only while it is hovered, which
    /// keeps a long list of selectors quiet.
    Quiet,
    /// A rounded tag tinted with the given color, no wider than its text.
    Tag(Color32),
}

/// A drop-down showing the current choice as a swatch and a name, truncated
/// to fit `size`.
fn selector(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    swatch: Swatch,
    text: RichText,
    size: egui::Vec2,
    style: SelectorStyle,
    contents: impl FnOnce(&mut egui::Ui),
) {
    ui.push_id(id, |ui| {
        ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
            let response = match style {
                SelectorStyle::Button => {
                    let mut atoms = swatch_atoms(text, true);
                    atoms.push_right(RichText::new("⏷").small().weak());
                    swatch_button(ui, swatch, Button::new(atoms).truncate().min_size(size))
                }
                SelectorStyle::Quiet => frameless_until_hovered(ui, |ui| {
                    let button = Button::new(swatch_atoms(text, true))
                        .truncate()
                        .frame_when_inactive(false)
                        .min_size(size);
                    swatch_button(ui, swatch, button)
                }),
                SelectorStyle::Tag(color) => {
                    // The button looks its own state up in the same way, so
                    // the tint follows the pointer one frame late.
                    let hovered = ui
                        .ctx()
                        .read_response(ui.next_auto_id())
                        .is_some_and(|response| response.hovered());
                    let tint = color.gamma_multiply(if hovered { 0.3 } else { 0.16 });
                    let button = Button::new(swatch_atoms(text, false))
                        .truncate()
                        .fill(tint)
                        .stroke(Stroke::NONE)
                        .corner_radius(size.y * 0.5);
                    swatch_button(ui, swatch, button)
                }
            };
            // Long lists scroll instead of running past the edge of a small window.
            let max_height = (ui.ctx().content_rect().height() - 80.0).clamp(120.0, 360.0);
            egui::Popup::menu(&response).show(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ScrollArea::vertical()
                    .max_height(max_height)
                    .show(ui, contents);
            });
        });
    });
}

/// Shows buttons that are drawn without a frame until they are hovered.
///
/// egui pads a button for the outline of its current state even while no
/// frame is drawn. Idle widgets get an outline in [`configure_style`], so
/// without this a label would shift by its width when the frame appears.
fn frameless_until_hovered<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope(|ui| {
        ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::NONE;
        add(ui)
    })
    .inner
}

/// How a row divides its width between its two drop-downs and the controls
/// to their right. A window too narrow for both moves the controls to a
/// second line.
struct RowLayout {
    selector_width: f32,
    two_lines: bool,
}

impl RowLayout {
    fn new(available: f32, controls: f32, spacing: f32) -> Self {
        let beside_controls = (available - controls - 2.0 * spacing) / 2.0;
        if beside_controls >= MIN_SELECTOR_WIDTH {
            Self {
                selector_width: beside_controls.min(MAX_SELECTOR_WIDTH),
                two_lines: false,
            }
        } else {
            Self {
                selector_width: ((available - spacing) / 2.0).max(0.0),
                two_lines: true,
            }
        }
    }
}

/// A horizontal strip with vertically centered contents. Its height is fixed
/// up front because egui places each widget before it knows that a taller
/// one will follow.
fn row<R>(ui: &mut egui::Ui, height: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.set_height(height);
        add(ui)
    })
    .inner
}

fn right_aligned<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.with_layout(Layout::right_to_left(Align::Center), add)
        .inner
}

fn card_frame() -> Frame {
    Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(5)
}

/// A white panel that spans the available width.
fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    card_frame()
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn entry_frame() -> Frame {
    Frame::new().inner_margin(egui::Margin::symmetric(10, 2))
}

fn hairline(ui: &mut egui::Ui) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, HAIRLINE));
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(semibold_font(15.0)));
}

fn count_label(ui: &mut egui::Ui, text: String) {
    ui.add(egui::Label::new(RichText::new(text).weak()).truncate());
}

fn entry_selectors(
    ui: &mut egui::Ui,
    entry: &mut model::TimeEntry,
    activities: &[ActivityOption],
    projects: &[ProjectOption],
    width: f32,
) -> bool {
    let size = egui::vec2(width, ROW_HEIGHT);
    let activity_id = ("entry-activity", entry.id);
    let project_id = ("entry-project", entry.id);
    let place = SelectorPlace::Entry;
    activity_selector(
        ui,
        activity_id,
        &mut entry.activity,
        activities,
        size,
        place,
    ) | project_selector(ui, project_id, &mut entry.project, projects, size, place)
}

struct EntryControls {
    delete: bool,
    restart: bool,
    time_edits: Vec<TimeEditRequest>,
    date: Option<NaiveDate>,
}

/// Laid out from the right: delete, restart, the duration, the date and the
/// stop and start times.
fn entry_controls(
    ui: &mut egui::Ui,
    entry: &model::TimeEntry,
    buffers: &mut BTreeMap<(EntryId, TimeEndpoint), String>,
    can_start: bool,
) -> EntryControls {
    ui.spacing_mut().item_spacing.x = 6.0;
    let delete = cross_icon_button(ui);
    let restart = play_icon_button(ui, can_start);
    let duration = format_duration(entry.time.duration_seconds().unwrap_or(0));
    let text_color = ui.visuals().text_color();
    ui.add_space(4.0);
    tabular_digits(ui, &duration, semibold_font(15.0), text_color);
    ui.add_space(4.0);
    let date = entry_date_picker(ui, entry.id, entry.time);
    let mut time_edits = Vec::new();
    time_edits.extend(inline_time_field(
        ui,
        buffers,
        entry.id,
        TimeEndpoint::Stop,
        stop_label(entry.time),
    ));
    ui.label("–");
    time_edits.extend(inline_time_field(
        ui,
        buffers,
        entry.id,
        TimeEndpoint::Start,
        start_label(entry.time),
    ));
    EntryControls {
        delete,
        restart,
        time_edits,
        date,
    }
}

/// An icon button that stays gray until it is hovered, so that a list full
/// of them does not compete with the entries. Returns the response and the
/// color to draw the icon in.
fn quiet_icon_button(
    ui: &mut egui::Ui,
    enabled: bool,
    accent: Color32,
) -> (egui::Response, Color32) {
    let size = egui::vec2(22.0, 22.0);
    let response = ui.add_enabled(enabled, Button::new("").frame(false).min_size(size));
    let color = if !enabled {
        Color32::from_gray(210)
    } else if response.hovered() {
        let strength = if response.is_pointer_button_down_on() {
            0.3
        } else {
            0.15
        };
        ui.painter()
            .rect_filled(response.rect, 3.0, accent.gamma_multiply(strength));
        accent
    } else {
        Color32::from_gray(140)
    };
    (response, color)
}

fn play_icon_button(ui: &mut egui::Ui, enabled: bool) -> bool {
    let (response, color) = quiet_icon_button(ui, enabled, GREEN);
    let center = response.rect.center();
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(center.x - 3.5, center.y - 5.0),
            egui::pos2(center.x - 3.5, center.y + 5.0),
            egui::pos2(center.x + 5.0, center.y),
        ],
        color,
        Stroke::NONE,
    ));
    response
        .on_hover_text("Start a timer with this activity and project")
        .on_disabled_hover_text("Stop the running timer first")
        .clicked()
}

fn cross_icon_button(ui: &mut egui::Ui) -> bool {
    let (response, color) = quiet_icon_button(ui, true, RED);
    let center = response.rect.center();
    let stroke = Stroke::new(1.5, color);
    for slope in [1.0, -1.0] {
        ui.painter().line_segment(
            [
                center + egui::vec2(-4.0, -4.0 * slope),
                center + egui::vec2(4.0, 4.0 * slope),
            ],
            stroke,
        );
    }
    response.on_hover_text("Delete this entry").clicked()
}

fn project_status_combo(ui: &mut egui::Ui, index: usize, status: &mut ProjectStatus) -> bool {
    let before = *status;
    ComboBox::from_id_salt(("project-status", index))
        .width(84.0)
        .selected_text(match status {
            ProjectStatus::Active => "Active",
            ProjectStatus::Finished => "Finished",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(status, ProjectStatus::Active, "Active");
            ui.selectable_value(status, ProjectStatus::Finished, "Finished");
        })
        .response
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    *status != before
}

/// The names behind the colors of the calendar, ordered by how much of the
/// given days they account for.
fn legend_items<'a, K: Ord + Copy + 'a>(
    totals: impl Iterator<Item = (&'a K, &'a u64)>,
    describe: impl Fn(K) -> Option<(Swatch, String)>,
) -> Vec<(Swatch, String)> {
    let mut seconds_by_key: BTreeMap<K, u64> = BTreeMap::new();
    for (&key, &seconds) in totals {
        *seconds_by_key.entry(key).or_default() += seconds;
    }
    let mut ranked: Vec<_> = seconds_by_key
        .into_iter()
        .filter(|(_, seconds)| *seconds > 0)
        .collect();
    ranked.sort_by_key(|(_, seconds)| std::cmp::Reverse(*seconds));
    ranked
        .into_iter()
        .filter_map(|(key, _)| describe(key))
        .collect()
}

/// Explains the calendar's colors in a line below it, within the width of
/// its cells.
fn calendar_legend(
    ui: &mut egui::Ui,
    cells_x: egui::Rangef,
    mode: CalendarMode,
    items: &[(Swatch, String)],
) {
    let font = FontId::proportional(11.0);
    let text_color = Color32::from_rgb(87, 96, 106);
    let caption = |text: &str| {
        RichText::new(text.to_owned())
            .font(font.clone())
            .color(text_color)
    };
    let top = ui.cursor().top();
    let area = egui::Rect::from_x_y_ranges(cells_x, top..=top + Swatch::SIZE + 4.0);
    ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
        if mode == CalendarMode::Time {
            // Laid out from the right; it reads "0h ... 8h" and then the
            // color for longer days.
            right_aligned(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                let step = |ui: &mut egui::Ui, value: f32, meaning: &str| {
                    let (rect, response) = ui
                        .allocate_exact_size(egui::Vec2::splat(Swatch::SIZE), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, heat_color(value));
                    response.on_hover_text(meaning);
                };
                ui.label(caption("more than 8h"));
                step(ui, 1.5, "More than 8 hours");
                ui.add_space(8.0);
                ui.label(caption("8h"));
                step(ui, 0.9, "6 to 8 hours");
                step(ui, 0.6, "4 to 6 hours");
                step(ui, 0.3, "2 to 4 hours");
                step(ui, 0.1, "Less than 2 hours");
                step(ui, 0.0, "No time tracked");
                ui.label(caption("0h"));
            });
        } else {
            let text_offset = Swatch::SIZE + 4.0;
            let text_width = (ui.available_width() - text_offset).max(0.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                for (swatch, name) in items.iter().take(LEGEND_ITEMS) {
                    // Each entry is a single widget, so that a line break
                    // never separates a swatch from its name.
                    let mut job = egui::text::LayoutJob::simple_singleline(
                        name.clone(),
                        font.clone(),
                        text_color,
                    );
                    job.wrap = egui::text::TextWrapping::truncate_at_width(text_width);
                    let galley = ui.painter().layout_job(job);
                    let size = egui::vec2(
                        text_offset + galley.size().x,
                        galley.size().y.max(Swatch::SIZE),
                    );
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    let swatch_rect = egui::Rect::from_center_size(
                        egui::pos2(rect.left() + Swatch::SIZE * 0.5, rect.center().y),
                        egui::Vec2::splat(Swatch::SIZE),
                    );
                    swatch.paint(ui.painter(), swatch_rect);
                    let text_top = rect.center().y - galley.size().y * 0.5;
                    ui.painter().galley(
                        egui::pos2(rect.left() + text_offset, text_top),
                        galley,
                        text_color,
                    );
                }
                if items.len() > LEGEND_ITEMS {
                    ui.label(caption(&format!("+{} more", items.len() - LEGEND_ITEMS)));
                }
            });
        }
    });
}

/// The Projects and Activities tables share their statistics columns, which
/// follow the `leading` ones.
fn usage_table(
    ui: &mut egui::Ui,
    id: &str,
    leading: &[&str],
    related: &str,
    rows: impl FnOnce(&mut egui::Ui),
) {
    let heading = |text: &str| RichText::new(text.to_owned()).weak();
    ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Unlike the other cards this one hugs its table, so that the
            // row stripes reach its edges in a wide window.
            card_frame().inner_margin(10.0).show(ui, |ui| {
                egui::Grid::new(id)
                    .spacing([12.0, 6.0])
                    .min_col_width(0.0)
                    .with_row_color(|row, _| (row > 0 && row % 2 == 0).then_some(STRIPE))
                    .show(ui, |ui| {
                        for title in leading {
                            ui.label(heading(title));
                        }
                        number_cell(ui, heading("Total"));
                        ui.label("");
                        for title in ["Entries", "Active days", "First used", "Last used", related]
                        {
                            number_cell(ui, heading(title));
                        }
                        ui.end_row();
                        rows(ui);
                    });
            });
        });
}

fn usage_cells(ui: &mut egui::Ui, stats: &UsageStats, largest_total: u64, bar: Color32) {
    number_cell(ui, format_stat_duration(stats.total_seconds));
    let share = if largest_total == 0 {
        0.0
    } else {
        stats.total_seconds as f32 / largest_total as f32
    };
    share_bar(ui, share, bar);
    number_cell(ui, stats.entries.to_string());
    number_cell(ui, stats.active_days.to_string());
    number_cell(ui, format_stat_date(stats.first_used));
    number_cell(ui, format_stat_date(stats.last_used));
    number_cell(ui, stats.related_count.to_string());
}

fn largest_total(stats: &[UsageStats]) -> u64 {
    stats
        .iter()
        .map(|stats| stats.total_seconds)
        .max()
        .unwrap_or(0)
}

fn number_cell(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) {
    right_aligned(ui, |ui| {
        ui.label(text);
    });
}

/// A bar that makes the totals in a column comparable at a glance.
fn share_bar(ui: &mut egui::Ui, share: f32, color: Color32) {
    let (track, _) = ui.allocate_exact_size(egui::vec2(56.0, 6.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(track, 3.0, Color32::from_gray(235));
    if share > 0.0 {
        let mut bar = track;
        bar.set_width((track.width() * share).max(2.0));
        ui.painter().rect_filled(bar, 3.0, color);
    }
}

fn color_cell(ui: &mut egui::Ui, value: &mut RgbColor) -> bool {
    let mut rgb = [value.red, value.green, value.blue];
    let changed = ui
        .color_edit_button_srgb(&mut rgb)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .changed();
    if changed {
        *value = RgbColor {
            red: rgb[0],
            green: rgb[1],
            blue: rgb[2],
        };
    }
    changed
}

fn format_stat_date(timestamp: Option<i64>) -> String {
    timestamp
        .map(|value| local_time::format_date(local_datetime(value).date_naive()))
        .unwrap_or_else(|| "-".to_owned())
}

fn week_title(week: NaiveDate, this_week: NaiveDate) -> String {
    if week == this_week {
        "This week".to_owned()
    } else if week == this_week - ChronoDuration::weeks(1) {
        "Last week".to_owned()
    } else {
        format!("Week of {}", local_time::format_date(week))
    }
}

fn day_title(date: NaiveDate, today: NaiveDate) -> String {
    if date == today {
        "Today".to_owned()
    } else if date == today - ChronoDuration::days(1) {
        "Yesterday".to_owned()
    } else {
        local_time::format_date_with_weekday(date)
    }
}

fn start_label(time: TimeRange) -> String {
    local_datetime(time.started_at.0)
        .format("%H:%M")
        .to_string()
}

fn stop_label(time: TimeRange) -> String {
    local_datetime(time.stopped_at.0)
        .format("%H:%M")
        .to_string()
}

fn entry_date_picker(ui: &mut egui::Ui, entry_id: EntryId, time: TimeRange) -> Option<NaiveDate> {
    let original = local_datetime(time.started_at.0).date_naive();
    let mut selected = original;
    let response = ui
        .add(DatePickerButton::new(&mut selected, ("entry-date", entry_id)).year_range(2000..=3000))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!(
            "Change date ({})",
            local_time::format_date(original)
        ));
    response.changed().then_some(selected)
}

fn inline_time_field(
    ui: &mut egui::Ui,
    buffers: &mut BTreeMap<(EntryId, TimeEndpoint), String>,
    entry_id: EntryId,
    endpoint: TimeEndpoint,
    label: String,
) -> Option<TimeEditRequest> {
    let text = buffers
        .entry((entry_id, endpoint))
        .or_insert_with(|| label.clone());
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .id_salt(("inline-time", entry_id, endpoint))
            .desired_width(TIME_FIELD_WIDTH)
            .horizontal_align(Align::Center),
    );
    if response.lost_focus() && *text != label {
        Some(TimeEditRequest {
            entry_id,
            endpoint,
            text: text.clone(),
        })
    } else {
        None
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// How long until `now()` changes. egui wakes up about one frame early, so the
/// margin keeps the repaint on the far side of the second boundary.
fn until_next_second() -> Duration {
    let elapsed_millis = chrono::Utc::now().timestamp_subsec_millis().min(999);
    Duration::from_millis(u64::from(1000 - elapsed_millis) + 20)
}

fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = seconds % 3600 / 60;
    let seconds = seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn format_stat_duration(seconds: u64) -> String {
    if seconds >= 3600 {
        let hours = seconds / 3600;
        format!("{hours} {}", if hours == 1 { "hour" } else { "hours" })
    } else if seconds >= 60 {
        let minutes = seconds / 60;
        format!(
            "{minutes} {}",
            if minutes == 1 { "minute" } else { "minutes" }
        )
    } else {
        format!(
            "{seconds} {}",
            if seconds == 1 { "second" } else { "seconds" }
        )
    }
}

fn color(color: RgbColor) -> Color32 {
    Color32::from_rgb(color.red, color.green, color.blue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_store_has_valid_references() {
        let store = Store {
            projects: vec![Project {
                name: "Project".to_owned(),
                color: RgbColor {
                    red: 1,
                    green: 2,
                    blue: 3,
                },
                status: ProjectStatus::Active,
            }],
            activities: vec![Activity {
                name: "Activity".to_owned(),
                color: RgbColor {
                    red: 79,
                    green: 121,
                    blue: 203,
                },
            }],
            time_entries: vec![model::TimeEntry {
                id: EntryId(1),
                activity: ActivityId(0),
                project: Some(ProjectId(0)),
                time: TimeRange {
                    started_at: UnixSeconds(10),
                    stopped_at: UnixSeconds(20),
                },
            }],
            ..Store::default()
        };
        store.validate().unwrap();
        for entry in &store.time_entries {
            assert!(entry.activity.0 < store.activities.len());
            assert!(entry
                .project
                .is_none_or(|project| project.0 < store.projects.len()));
            assert!(entry.time.stopped_at >= entry.time.started_at);
        }
    }

    #[test]
    fn statistics_use_one_largest_unit() {
        assert_eq!(format_stat_duration(300 * 3600 + 42 * 60), "300 hours");
        assert_eq!(format_stat_duration(30 * 60 + 42), "30 minutes");
        assert_eq!(format_stat_duration(42), "42 seconds");
    }

    #[test]
    fn calendar_date_anchors_at_the_newest_entry_on_or_before_that_date() {
        let dates = [(14, 9), (15, 10), (15, 16), (17, 8)];
        let entries: Vec<_> = dates
            .into_iter()
            .enumerate()
            .map(|(index, (day, hour))| {
                let started_at = local_time::timestamp(
                    NaiveDate::from_ymd_opt(2026, 9, day)
                        .unwrap()
                        .and_hms_opt(hour, 0, 0)
                        .unwrap(),
                )
                .unwrap();
                model::TimeEntry {
                    id: EntryId(index as u64 + 1),
                    activity: ActivityId(0),
                    project: None,
                    time: TimeRange {
                        started_at: UnixSeconds(started_at),
                        stopped_at: UnixSeconds(started_at + 60),
                    },
                }
            })
            .collect();

        assert_eq!(
            entry_offset_for_date(&entries, NaiveDate::from_ymd_opt(2026, 9, 15).unwrap()),
            1
        );
        assert_eq!(
            entry_offset_for_date(&entries, NaiveDate::from_ymd_opt(2026, 9, 16).unwrap()),
            1
        );
        assert_eq!(
            entry_offset_for_date(&entries, NaiveDate::from_ymd_opt(2026, 9, 14).unwrap()),
            3
        );
        assert_eq!(
            entry_offset_for_date(&entries, NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()),
            3
        );
    }

    #[test]
    fn remembered_selections_discard_missing_model_references() {
        let store = Store {
            projects: vec![Project {
                name: "Project".to_owned(),
                color: RgbColor {
                    red: 1,
                    green: 2,
                    blue: 3,
                },
                status: ProjectStatus::Active,
            }],
            activities: vec![
                Activity {
                    name: "First".to_owned(),
                    color: RgbColor {
                        red: 79,
                        green: 121,
                        blue: 203,
                    },
                },
                Activity {
                    name: "Latest".to_owned(),
                    color: RgbColor {
                        red: 79,
                        green: 121,
                        blue: 203,
                    },
                },
            ],
            ..Store::default()
        };
        let restored = RememberedSelections {
            activity: ActivityId(99),
            project: Some(ProjectId(99)),
            calendar_mode: CalendarMode::Projects,
        }
        .validated_for(&store);

        assert_eq!(restored.activity, ActivityId(1));
        assert_eq!(restored.project, None);
        assert_eq!(restored.calendar_mode, CalendarMode::Projects);
    }
}
