#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Local, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use eframe::egui::{
    self, Align, Align2, AtomExt, Button, Color32, ComboBox, FontData, FontDefinitions, FontFamily,
    FontId, Frame, Layout, RichText, ScrollArea, Stroke,
};

mod app_icon;
mod desktop;
mod github_activity;
mod local_time;
mod model;
mod paths;
mod reports;
mod single_instance;
mod storage;
mod tray;

use github_activity::{heat_color, CalendarHeatmap, CellFill, Segment};
use local_time::{datetime as local_datetime, timestamp as local_naive_timestamp};

use model::{
    Activity, ActivityId, EntryId, Project, ProjectId, ProjectStatus, RgbColor, Store, TimeRange,
    UnixSeconds, MAX_ENTRY_SECONDS,
};
use paths::AppPaths;
use reports::{
    activity_stats, activity_totals, add_interval_to_days, project_stats,
    project_totals_by_local_day, totals_by_local_day, totals_by_week, week_start, UsageStats,
};
use storage::JsonStore;

const BLUE: Color32 = Color32::from_rgb(3, 169, 244);
const RED: Color32 = Color32::from_rgb(244, 67, 54);
const PAGE: Color32 = Color32::from_rgb(242, 247, 250);
const HEADER: Color32 = Color32::from_rgb(228, 236, 241);
const BORDER: Color32 = Color32::from_rgb(218, 227, 232);
const SELECTOR_HEIGHT: f32 = 30.0;
const ENTRIES_PER_PAGE: usize = 100;

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
            .with_visible(!start_minimized)
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([820.0, 560.0]),
        persistence_path: Some(app_paths.persistence.clone()),
        ..Default::default()
    };
    eframe::run_native(
        "OptoTracker",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::light());
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

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let family = project_font_family();
    let mut project_fonts = Vec::new();

    for path in [
        r"C:\Windows\Fonts\segoeuib.ttf",
        r"C:\Windows\Fonts\arialbd.ttf",
    ] {
        if let Ok(bytes) = fs::read(path) {
            let name = "project-bold".to_owned();
            fonts.font_data.insert(
                name.clone(),
                std::sync::Arc::new(FontData::from_owned(bytes)),
            );
            project_fonts.push(name);
            break;
        }
    }

    if let Some(fallbacks) = fonts.families.get(&FontFamily::Proportional) {
        project_fonts.extend(fallbacks.iter().cloned());
    }
    fonts.families.insert(family, project_fonts);
    ctx.set_fonts(fonts);
}

fn project_font_family() -> FontFamily {
    FontFamily::Name("ProjectBold".into())
}

fn project_font() -> FontId {
    FontId::new(14.0, project_font_family())
}

fn project_text(text: impl Into<String>, text_color: Color32) -> RichText {
    RichText::new(text.into())
        .font(project_font())
        .color(text_color)
}

fn total_label(ui: &mut egui::Ui, prefix: &str, seconds: u64, text_color: Color32) {
    let mut text = egui::text::LayoutJob::default();
    text.append(
        prefix,
        0.0,
        egui::TextFormat {
            font_id: FontId::new(14.0, FontFamily::Proportional),
            color: text_color,
            ..Default::default()
        },
    );
    text.append(
        &format_duration(seconds),
        0.0,
        egui::TextFormat {
            font_id: project_font(),
            color: text_color,
            ..Default::default()
        },
    );
    ui.label(text);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppView {
    Entries,
    Projects,
    Activities,
    Settings,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum CalendarMode {
    #[default]
    Time,
    Projects,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum TimeEndpoint {
    Start,
    Stop,
}

#[derive(Clone)]
struct TimeEditRequest {
    entry_id: EntryId,
    endpoint: TimeEndpoint,
    text: String,
}

struct ActivityOption {
    id: ActivityId,
    name: String,
    total_seconds: u64,
}

struct ProjectOption {
    id: ProjectId,
    name: String,
    color: Color32,
    status: ProjectStatus,
}

struct ProjectContribution {
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
    time_edit_buffers: BTreeMap<(EntryId, TimeEndpoint), String>,
    calendar: CalendarHeatmap,
    calendar_mode: CalendarMode,
    entry_page: usize,
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
        let draft_activity = ActivityId(store.activities.len().saturating_sub(1));
        let running = store.active_entry.is_some();
        let tray = tray::TrayController::new(running);
        let preferences: PersistedSettings = storage
            .and_then(|storage| eframe::get_value(storage, "opto_tracker_settings"))
            .unwrap_or_default();
        let message = match &tray {
            Ok(_) => String::new(),
            Err(error) => format!("Could not create tray icon: {error}"),
        };
        let tray = tray.ok();
        let window_hidden = start_minimized && tray.is_some();
        if start_minimized && tray.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
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
            draft_activity,
            draft_project: None,
            message,
            new_project_name: String::new(),
            new_project_color: [104, 159, 56],
            new_activity_name: String::new(),
            time_edit_buffers: BTreeMap::new(),
            calendar: CalendarHeatmap::default(),
            calendar_mode: CalendarMode::default(),
            entry_page: 0,
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
                self.message = format!("Save failed; change was reverted: {error}");
            }
        }
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
        let mut active_changed = false;
        let panel_width = ui.available_width();
        Frame::new()
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.0, BORDER))
            .inner_margin(18.0)
            .show(ui, |ui| {
                ui.set_min_width((panel_width - 38.0).max(0.0));
                ui.horizontal(|ui| {
                    ui.set_height(42.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let started_at = self
                            .store
                            .active_entry
                            .as_ref()
                            .map(|active| active.started_at.0);
                        let elapsed = started_at
                            .map(|started_at| {
                                (now().checked_sub(started_at).unwrap_or(0).max(0) as u64)
                                    .min(MAX_ENTRY_SECONDS as u64)
                            })
                            .unwrap_or(0);
                        if started_at.is_some() {
                            if ui
                                .add_sized(
                                    [90.0, 36.0],
                                    Button::new(RichText::new("STOP").color(Color32::WHITE))
                                        .fill(RED),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                self.stop_timer();
                            }
                            ui.ctx().request_repaint_after(Duration::from_secs(1));
                        } else {
                            if ui
                                .add_enabled(
                                    !activities.is_empty(),
                                    Button::new(RichText::new("START").color(Color32::WHITE))
                                        .fill(BLUE)
                                        .min_size(egui::vec2(90.0, 36.0)),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                self.start_timer();
                            }
                        }
                        ui.add_sized(
                            [105.0, 42.0],
                            egui::Label::new(
                                RichText::new(format_duration(elapsed)).size(20.0).strong(),
                            ),
                        );

                        ui.separator();
                        let selector_width = 220.0 + 190.0 + ui.spacing().item_spacing.x;
                        ui.add_space((ui.available_width() - selector_width).max(0.0));

                        let project = self
                            .store
                            .active_entry
                            .as_mut()
                            .map(|entry| &mut entry.project)
                            .unwrap_or(&mut self.draft_project);
                        let project_changed =
                            project_combo(ui, "timer-project", project, &projects);
                        active_changed |= self.store.active_entry.is_some() && project_changed;

                        if let Some(active) = self.store.active_entry.as_mut() {
                            active_changed |= activity_combo(
                                ui,
                                "active-activity",
                                &mut active.activity,
                                &activities,
                            );
                        } else {
                            activity_combo(
                                ui,
                                "draft-activity",
                                &mut self.draft_activity,
                                &activities,
                            );
                        }
                    });
                });
            });
        if active_changed {
            self.save();
        }
    }

    fn entries_panel(&mut self, ui: &mut egui::Ui) {
        let entry_count = self.store.time_entries.len();
        let page_count = entry_count.div_ceil(ENTRIES_PER_PAGE).max(1);
        self.entry_page = self.entry_page.min(page_count - 1);

        let current_time = now();
        let activities = activity_options(&self.store, current_time);
        let projects = project_options(&self.store);
        let day_totals = totals_by_local_day(&self.store, current_time);
        let project_contributions: BTreeMap<NaiveDate, Vec<ProjectContribution>> =
            project_totals_by_local_day(&self.store, current_time)
                .into_iter()
                .map(|(date, contributions)| {
                    let segments = contributions
                        .into_iter()
                        .filter(|(_, seconds)| *seconds > 0)
                        .map(|(project, seconds)| {
                            let (label, segment_color) = project
                                .and_then(|index| self.store.projects.get(index))
                                .map(|project| (project.name.clone(), color(project.color)))
                                .unwrap_or_else(|| {
                                    ("No project".to_owned(), Color32::from_gray(125))
                                });
                            ProjectContribution {
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
        let week_totals = totals_by_week(&day_totals);
        let today = Local::now().date_naive();
        let this_week = week_start(today);
        let mut changed = false;
        let mut delete = None;
        let mut start_request = None;
        let mut time_commits = Vec::new();
        let mut current_week: Option<NaiveDate> = None;
        let can_start = self.store.active_entry.is_none();
        let mut entries_by_day: BTreeMap<NaiveDate, Vec<usize>> = BTreeMap::new();
        let page_start = self.entry_page * ENTRIES_PER_PAGE;
        for index in (0..self.store.time_entries.len())
            .rev()
            .skip(page_start)
            .take(ENTRIES_PER_PAGE)
        {
            let entry = &self.store.time_entries[index];
            let date = local_datetime(entry.time.started_at.0).date_naive();
            entries_by_day.entry(date).or_default().push(index);
            let mut split = BTreeMap::new();
            add_interval_to_days(&mut split, entry.time.started_at.0, entry.time.stopped_at.0);
            for date in split.into_keys() {
                entries_by_day.entry(date).or_default();
            }
        }
        let display_days: Vec<NaiveDate> = entries_by_day.keys().copied().rev().collect();

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .scroll_bar_visibility(
                egui::containers::scroll_area::ScrollBarVisibility::AlwaysVisible,
            )
            .show(ui, |ui| {
                let activity_width = ui.available_width();
                Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(14.0)
                    .show(ui, |ui| {
                        ui.set_min_width((activity_width - 30.0).max(0.0));
                        ui.horizontal(|ui| {
                            ui.heading("Activity");
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.selectable_value(
                                    &mut self.calendar_mode,
                                    CalendarMode::Projects,
                                    "Projects",
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                                ui.selectable_value(
                                    &mut self.calendar_mode,
                                    CalendarMode::Time,
                                    "Time",
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            });
                        });
                        let calendar_mode = self.calendar_mode;
                        self.calendar.ui(
                            ui,
                            today,
                            |date| match calendar_mode {
                                CalendarMode::Time => day_totals
                                    .get(&date)
                                    .map(|seconds| {
                                        CellFill::Solid(heat_color(
                                            *seconds as f32 / (8.0 * 60.0 * 60.0),
                                        ))
                                    })
                                    .unwrap_or(CellFill::Empty),
                                CalendarMode::Projects => project_segments
                                    .get(&date)
                                    .map(|segments| CellFill::Segmented(segments))
                                    .unwrap_or(CellFill::Empty),
                            },
                            |ui, date| {
                                ui.set_min_width(220.0);
                                ui.label(date.format("%A, %B %-d, %Y").to_string());
                                ui.label(
                                    day_totals
                                        .get(&date)
                                        .copied()
                                        .map(format_duration)
                                        .unwrap_or_else(|| "No time tracked".to_owned()),
                                );
                                if calendar_mode == CalendarMode::Projects {
                                    if let Some(contributions) = project_contributions.get(&date) {
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
                                                ui.with_layout(
                                                    Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        ui.label(&contribution.detail);
                                                    },
                                                );
                                            });
                                        }
                                    }
                                }
                            },
                        );
                    });
                ui.add_space(12.0);
                pagination_controls(ui, &mut self.entry_page, entry_count);

                for date in display_days {
                    let entry_week = week_start(date);
                    if current_week != Some(entry_week) {
                        current_week = Some(entry_week);
                        ui.add_space(18.0);
                        ui.horizontal(|ui| {
                            ui.heading(week_title(entry_week, this_week));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                total_label(
                                    ui,
                                    "Week total: ",
                                    week_totals.get(&entry_week).copied().unwrap_or(0),
                                    ui.visuals().text_color(),
                                );
                            });
                        });
                    }
                    ui.add_space(8.0);
                    Frame::new()
                        .fill(HEADER)
                        .inner_margin(egui::Margin::symmetric(14, 9))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(day_title(date, today))
                                        .color(Color32::from_gray(90))
                                        .strong(),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    total_label(
                                        ui,
                                        "Day total: ",
                                        day_totals.get(&date).copied().unwrap_or(0),
                                        Color32::from_gray(90),
                                    );
                                });
                            });
                        });
                    let indices = entries_by_day.get(&date).cloned().unwrap_or_default();
                    if indices.is_empty() {
                        Frame::new()
                            .fill(Color32::WHITE)
                            .stroke(Stroke::new(1.0, BORDER))
                            .inner_margin(egui::Margin::symmetric(14, 10))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("Time carried over from an overnight entry")
                                        .color(Color32::from_gray(115)),
                                );
                            });
                    }
                    for index in indices {
                        let entry = &mut self.store.time_entries[index];
                        let entry_id = entry.id;
                        Frame::new()
                            .fill(Color32::WHITE)
                            .stroke(Stroke::new(1.0, BORDER))
                            .inner_margin(egui::Margin::symmetric(14, 10))
                            .show(ui, |ui| {
                                // Establish the final row height before egui places the first
                                // widget. Otherwise these selectors are positioned before the
                                // taller icon buttons on the right enlarge the row.
                                ui.set_height(SELECTOR_HEIGHT);
                                ui.horizontal(|ui| {
                                    changed |= activity_combo(
                                        ui,
                                        ("entry-activity", entry_id),
                                        &mut entry.activity,
                                        &activities,
                                    );
                                    changed |= project_combo(
                                        ui,
                                        ("entry-project", entry_id),
                                        &mut entry.project,
                                        &projects,
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if cross_icon_button(ui) {
                                            delete = Some(entry_id);
                                        }
                                        if play_icon_button(ui, can_start) {
                                            start_request = Some((entry.activity, entry.project));
                                        }
                                        ui.separator();
                                        ui.label(
                                            RichText::new(format_duration(
                                                entry.time.duration_seconds().unwrap_or(0),
                                            ))
                                            .size(18.0)
                                            .strong(),
                                        );
                                        if let Some(edit) = inline_time_field(
                                            ui,
                                            &mut self.time_edit_buffers,
                                            entry_id,
                                            TimeEndpoint::Stop,
                                            stop_label(entry.time),
                                        ) {
                                            time_commits.push(edit);
                                        }
                                        ui.label("-");
                                        if let Some(edit) = inline_time_field(
                                            ui,
                                            &mut self.time_edit_buffers,
                                            entry_id,
                                            TimeEndpoint::Start,
                                            start_label(entry.time),
                                        ) {
                                            time_commits.push(edit);
                                        }
                                    });
                                });
                            });
                    }
                }

                ui.add_space(16.0);
                pagination_controls(ui, &mut self.entry_page, entry_count);
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
            self.apply_inline_time_edit(edit);
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
            .and_then(|entry| edited_time_range(entry.time, edit.endpoint, &edit.text));
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

    fn projects_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Projects");
            ui.label(
                RichText::new(format!("{} projects", self.store.projects.len()))
                    .color(Color32::from_gray(110)),
            );
        });
        ui.label(
            "Rename projects, change their color or finish them. Vector order remains stable.",
        );
        ui.add_space(12.0);
        let mut add_clicked = false;
        let add_row_width = ui.available_width();
        Frame::new()
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.0, BORDER))
            .inner_margin(14.0)
            .show(ui, |ui| {
                ui.set_min_width((add_row_width - 30.0).max(0.0));
                ui.horizontal(|ui| {
                    ui.label("New project");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_project_name).desired_width(220.0),
                    );
                    ui.color_edit_button_srgb(&mut self.new_project_color)
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    add_clicked = ui
                        .button("Add")
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked();
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
        let mut changed = false;
        ui.add_space(12.0);
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .scroll_bar_visibility(
                egui::containers::scroll_area::ScrollBarVisibility::AlwaysVisible,
            )
            .show(ui, |ui| {
                for index in (0..self.store.projects.len()).rev() {
                    let project = &mut self.store.projects[index];
                    let stats = &stats[index];
                    let row_width = ui.available_width();
                    Frame::new()
                        .fill(Color32::WHITE)
                        .stroke(Stroke::new(1.0, BORDER))
                        .inner_margin(14.0)
                        .show(ui, |ui| {
                            ui.set_min_width((row_width - 30.0).max(0.0));
                            ui.horizontal(|ui| {
                                let mut rgb =
                                    [project.color.red, project.color.green, project.color.blue];
                                if ui
                                    .color_edit_button_srgb(&mut rgb)
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .changed()
                                {
                                    project.color = RgbColor {
                                        red: rgb[0],
                                        green: rgb[1],
                                        blue: rgb[2],
                                    };
                                    changed = true;
                                }
                                changed |= ui
                                    .add(
                                        egui::TextEdit::singleline(&mut project.name)
                                            .desired_width(260.0)
                                            .font(project_font())
                                            .text_color(color(project.color)),
                                    )
                                    .changed();
                                changed |= project_status_combo(ui, index, &mut project.status);
                            });
                            ui.horizontal_wrapped(|ui| {
                                stat_label(ui, "Total", format_stat_duration(stats.total_seconds));
                                stat_label(ui, "Entries", stats.entries.to_string());
                                stat_label(ui, "Active days", stats.active_days.to_string());
                                stat_label(ui, "First used", format_stat_date(stats.first_used));
                                stat_label(ui, "Last used", format_stat_date(stats.last_used));
                                stat_label(ui, "Activities", stats.related_count.to_string());
                            });
                        });
                    ui.add_space(8.0);
                }
            });
        if changed {
            self.save();
        }
    }

    fn activities_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Activities");
            ui.label(
                RichText::new(format!("{} activities", self.store.activities.len()))
                    .color(Color32::from_gray(110)),
            );
        });
        ui.label("Activity IDs stay stable while names can be edited.");
        ui.add_space(12.0);
        let mut add_clicked = false;
        let add_row_width = ui.available_width();
        Frame::new()
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.0, BORDER))
            .inner_margin(14.0)
            .show(ui, |ui| {
                ui.set_min_width((add_row_width - 30.0).max(0.0));
                ui.horizontal(|ui| {
                    ui.label("New activity");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_activity_name)
                            .desired_width(260.0),
                    );
                    add_clicked = ui
                        .button("Add")
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked();
                });
            });
        if add_clicked {
            let name = self.new_activity_name.trim();
            if name.is_empty() {
                self.message = "Activity name cannot be empty".to_owned();
            } else {
                self.store.activities.push(Activity {
                    name: name.to_owned(),
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
        let mut changed = false;
        ui.add_space(12.0);
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .scroll_bar_visibility(
                egui::containers::scroll_area::ScrollBarVisibility::AlwaysVisible,
            )
            .show(ui, |ui| {
                for index in activity_order {
                    let activity = &mut self.store.activities[index];
                    let stats = &stats[index];
                    let row_width = ui.available_width();
                    Frame::new()
                        .fill(Color32::WHITE)
                        .stroke(Stroke::new(1.0, BORDER))
                        .inner_margin(14.0)
                        .show(ui, |ui| {
                            ui.set_min_width((row_width - 30.0).max(0.0));
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut activity.name)
                                        .desired_width(300.0),
                                )
                                .changed();
                            ui.horizontal_wrapped(|ui| {
                                stat_label(ui, "Total", format_stat_duration(stats.total_seconds));
                                stat_label(ui, "Entries", stats.entries.to_string());
                                stat_label(ui, "Active days", stats.active_days.to_string());
                                stat_label(ui, "First used", format_stat_date(stats.first_used));
                                stat_label(ui, "Last used", format_stat_date(stats.last_used));
                                stat_label(ui, "Projects", stats.related_count.to_string());
                            });
                        });
                    ui.add_space(8.0);
                }
            });
        if changed {
            self.save();
        }
    }

    fn settings_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.add_space(6.0);
        ui.label(
            RichText::new("Installation, storage and Windows startup behavior.")
                .color(Color32::from_gray(95)),
        );
        if !self.settings_message.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(&self.settings_message).color(BLUE));
        }
        ui.add_space(18.0);

        let width = ui.available_width();
        Frame::new()
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.0, BORDER))
            .inner_margin(18.0)
            .show(ui, |ui| {
                ui.set_min_width((width - 38.0).max(0.0));
                ui.heading("Application folder");
                ui.label(RichText::new(self.app_paths.directory.display().to_string()).monospace());
                ui.label(
                    RichText::new(
                        "The installed executable and UI persistence are stored in this folder.",
                    )
                    .color(Color32::from_gray(105)),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
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
                    ui.horizontal(|ui| {
                        ui.label("Installed executable:");
                        ui.label(RichText::new(paths::EXECUTABLE_FILE).monospace());
                    });
                } else {
                    ui.label("OptoTracker is not installed in this folder yet.");
                }
                ui.horizontal(|ui| {
                    ui.label("Data:");
                    ui.label(
                        RichText::new(self.repository.path().display().to_string()).monospace(),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Persistence:");
                    ui.label(
                        RichText::new(self.app_paths.persistence.display().to_string()).monospace(),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Weekly backups:");
                    ui.label(RichText::new("backups").monospace());
                });
            });

        ui.add_space(14.0);
        Frame::new()
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.0, BORDER))
            .inner_margin(18.0)
            .show(ui, |ui| {
                ui.set_min_width((width - 38.0).max(0.0));
                ui.heading("Windows behavior");
                ui.add_space(8.0);

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
        ctx.request_repaint_after(Duration::from_millis(200));
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
        let timer_reached_limit = self.store.active_entry.as_ref().is_some_and(|active| {
            now()
                .checked_sub(active.started_at.0)
                .is_some_and(|elapsed| elapsed >= MAX_ENTRY_SECONDS)
        });
        if timer_reached_limit {
            self.stop_timer();
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

        egui::Panel::top("top-bar")
            .exact_size(58.0)
            .frame(Frame::new().fill(Color32::WHITE).inner_margin(14.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (icon_rect, _) =
                        ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
                    app_icon::paint(ui.painter(), icon_rect, running);
                    ui.label(RichText::new("OptoTracker").size(24.0).strong());
                    ui.add_space(20.0);
                    ui.selectable_value(&mut self.view, AppView::Entries, "Time entries")
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    ui.selectable_value(&mut self.view, AppView::Projects, "Projects")
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    ui.selectable_value(&mut self.view, AppView::Activities, "Activities")
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    ui.selectable_value(&mut self.view, AppView::Settings, "Settings")
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if !self.message.is_empty() {
                            ui.label(RichText::new(&self.message).color(Color32::from_gray(110)));
                        }
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(Frame::new().fill(PAGE).inner_margin(24.0))
            .show(ui, |ui| {
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                match self.view {
                    AppView::Entries => {
                        ui.horizontal(|ui| {
                            ui.heading("Time entries");
                            ui.label(
                                RichText::new(format!("{} entries", self.store.time_entries.len()))
                                    .color(Color32::from_gray(110)),
                            );
                        });
                        ui.add_space(12.0);
                        self.timer_panel(ui);
                        ui.add_space(18.0);
                        self.entries_panel(ui);
                    }
                    AppView::Projects => self.projects_panel(ui),
                    AppView::Activities => self.activities_panel(ui),
                    AppView::Settings => self.settings_panel(ui),
                }
            });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "opto_tracker_settings", &self.preferences);
    }

    fn auto_save_interval(&self) -> Duration {
        Duration::from_secs(5)
    }
}

fn pagination_controls(ui: &mut egui::Ui, page: &mut usize, total_entries: usize) {
    let page_count = total_entries.div_ceil(ENTRIES_PER_PAGE).max(1);
    *page = (*page).min(page_count - 1);
    let first = if total_entries == 0 {
        0
    } else {
        *page * ENTRIES_PER_PAGE + 1
    };
    let last = ((*page + 1) * ENTRIES_PER_PAGE).min(total_entries);

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("Showing {first}-{last} of {total_entries}"))
                .color(Color32::from_gray(105)),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add_enabled(*page + 1 < page_count, Button::new("Next"))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                *page += 1;
            }
            ui.label(format!("Page {} of {page_count}", *page + 1));
            if ui
                .add_enabled(*page > 0, Button::new("Previous"))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                *page -= 1;
            }
        });
    });
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

fn activity_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut ActivityId,
    activities: &[ActivityOption],
) -> bool {
    let before = *value;
    let selected = activities
        .iter()
        .find(|activity| activity.id == *value)
        .map(|activity| activity.name.as_str())
        .unwrap_or("Choose activity");
    invisible_combo(
        ui,
        id,
        RichText::new(selected).color(Color32::from_gray(30)),
        220.0,
        |ui| {
            for activity in activities {
                ui.selectable_value(value, activity.id, &activity.name);
            }
        },
    );
    *value != before
}

fn project_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut Option<ProjectId>,
    projects: &[ProjectOption],
) -> bool {
    let before = *value;
    let selected = value
        .and_then(|id| projects.iter().find(|project| project.id == id))
        .map(|project| project_text(&project.name, project.color))
        .unwrap_or_else(|| project_text("No project", Color32::from_gray(110)));
    invisible_combo(ui, id, selected, 190.0, |ui| {
        for project in projects
            .iter()
            .rev()
            .filter(|project| project.status == ProjectStatus::Active)
        {
            ui.selectable_value(
                value,
                Some(project.id),
                project_text(&project.name, project.color),
            );
        }
        if projects
            .iter()
            .any(|project| project.status == ProjectStatus::Finished)
        {
            ui.separator();
            for project in projects
                .iter()
                .rev()
                .filter(|project| project.status == ProjectStatus::Finished)
            {
                ui.selectable_value(
                    value,
                    Some(project.id),
                    project_text(&project.name, project.color),
                );
            }
        }
        ui.separator();
        ui.selectable_value(
            value,
            None,
            project_text("No project", Color32::from_gray(110)),
        );
    });
    *value != before
}

fn invisible_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    selected: RichText,
    width: f32,
    contents: impl FnOnce(&mut egui::Ui),
) {
    ui.push_id(id, |ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(width, SELECTOR_HEIGHT),
            Layout::left_to_right(Align::Center),
            |ui| {
                let button = Button::new(selected.atom_grow(true).atom_align(Align2::LEFT_CENTER))
                    .frame(false)
                    .min_size(egui::vec2(width, SELECTOR_HEIGHT));
                let (response, _) =
                    egui::containers::menu::MenuButton::from_button(button).ui(ui, contents);
                response.on_hover_cursor(egui::CursorIcon::PointingHand);
            },
        );
    });
}

fn bright_combo<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope(|ui| {
        let widgets = &mut ui.visuals_mut().widgets;
        for visuals in [
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
            &mut widgets.open,
        ] {
            visuals.bg_fill = Color32::WHITE;
            visuals.weak_bg_fill = Color32::WHITE;
            visuals.bg_stroke = Stroke::new(1.0, Color32::from_rgb(168, 192, 202));
            visuals.fg_stroke = Stroke::new(1.5, Color32::from_rgb(45, 142, 184));
        }
        add(ui)
    })
    .inner
}

fn play_icon_button(ui: &mut egui::Ui, enabled: bool) -> bool {
    let response = ui.add_enabled(
        enabled,
        Button::new("")
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .min_size(egui::vec2(30.0, 26.0)),
    );
    let response = if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    let fill = if !enabled {
        Color32::from_gray(210)
    } else if response.is_pointer_button_down_on() {
        Color32::from_rgb(46, 125, 50)
    } else if response.hovered() {
        Color32::from_rgb(67, 160, 71)
    } else {
        Color32::from_rgb(76, 175, 80)
    };
    ui.painter()
        .rect_filled(response.rect, egui::CornerRadius::same(3), fill);
    let center = response.rect.center();
    let icon_color = if enabled {
        Color32::WHITE
    } else {
        Color32::from_gray(145)
    };
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(center.x - 4.0, center.y - 7.0),
            egui::pos2(center.x - 4.0, center.y + 7.0),
            egui::pos2(center.x + 7.0, center.y),
        ],
        icon_color,
        Stroke::NONE,
    ));
    response.clicked()
}

fn cross_icon_button(ui: &mut egui::Ui) -> bool {
    let response = ui
        .add(
            Button::new("")
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::NONE)
                .min_size(egui::vec2(30.0, 26.0)),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if response.is_pointer_button_down_on() {
        Color32::from_rgb(244, 190, 190)
    } else if response.hovered() {
        Color32::from_rgb(255, 224, 224)
    } else {
        Color32::from_rgb(255, 243, 243)
    };
    ui.painter()
        .rect_filled(response.rect, egui::CornerRadius::same(3), fill);
    let center = response.rect.center();
    let stroke = Stroke::new(1.8, Color32::from_rgb(198, 65, 65));
    ui.painter().line_segment(
        [
            egui::pos2(center.x - 5.0, center.y - 5.0),
            egui::pos2(center.x + 5.0, center.y + 5.0),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            egui::pos2(center.x + 5.0, center.y - 5.0),
            egui::pos2(center.x - 5.0, center.y + 5.0),
        ],
        stroke,
    );
    response.clicked()
}

fn project_status_combo(ui: &mut egui::Ui, index: usize, status: &mut ProjectStatus) -> bool {
    let before = *status;
    bright_combo(ui, |ui| {
        let popup = ComboBox::from_id_salt(("project-status", index))
            .selected_text(match status {
                ProjectStatus::Active => "Active",
                ProjectStatus::Finished => "Finished",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(status, ProjectStatus::Active, "Active");
                ui.selectable_value(status, ProjectStatus::Finished, "Finished");
            });
        popup
            .response
            .on_hover_cursor(egui::CursorIcon::PointingHand);
    });
    *status != before
}

fn stat_label(ui: &mut egui::Ui, label: &str, value: String) {
    ui.label(
        RichText::new(format!("{label}: {value}"))
            .small()
            .color(Color32::from_gray(100)),
    );
}

fn format_stat_date(timestamp: Option<i64>) -> String {
    timestamp
        .map(|value| local_datetime(value).format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "-".to_owned())
}

fn week_title(week: NaiveDate, this_week: NaiveDate) -> String {
    if week == this_week {
        "This week".to_owned()
    } else if week == this_week - ChronoDuration::weeks(1) {
        "Last week".to_owned()
    } else {
        format!("Week of {}", week.format("%B %-d, %Y"))
    }
}

fn day_title(date: NaiveDate, today: NaiveDate) -> String {
    if date == today {
        "Today".to_owned()
    } else if date == today - ChronoDuration::days(1) {
        "Yesterday".to_owned()
    } else {
        date.format("%A, %B %-d, %Y").to_string()
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
            .desired_width(52.0)
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

fn edited_time_range(
    original: TimeRange,
    endpoint: TimeEndpoint,
    edited_text: &str,
) -> Result<TimeRange, String> {
    let original_start = local_datetime(original.started_at.0);
    let original_stop = local_datetime(original.stopped_at.0);
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
    let started_at = local_naive_timestamp(NaiveDateTime::new(date, start_time))?;
    let stopped_at = local_naive_timestamp(NaiveDateTime::new(stop_date, stop_time))?;
    let duration = stopped_at - started_at;
    if duration <= 0 || duration > 24 * 60 * 60 {
        return Err("A time entry must be between 1 minute and 24 hours".to_owned());
    }
    Ok(TimeRange {
        started_at: UnixSeconds(started_at),
        stopped_at: UnixSeconds(stopped_at),
    })
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
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
    fn overnight_entry_is_split_between_local_days() {
        let start = local_naive_timestamp(
            NaiveDate::from_ymd_opt(2026, 9, 15)
                .unwrap()
                .and_hms_opt(23, 30, 0)
                .unwrap(),
        )
        .unwrap();
        let stop = local_naive_timestamp(
            NaiveDate::from_ymd_opt(2026, 9, 16)
                .unwrap()
                .and_hms_opt(2, 20, 0)
                .unwrap(),
        )
        .unwrap();
        let mut totals = BTreeMap::new();
        add_interval_to_days(&mut totals, start, stop);
        assert_eq!(
            totals[&NaiveDate::from_ymd_opt(2026, 9, 15).unwrap()],
            30 * 60
        );
        assert_eq!(
            totals[&NaiveDate::from_ymd_opt(2026, 9, 16).unwrap()],
            2 * 60 * 60 + 20 * 60
        );
    }

    #[test]
    fn inline_stop_time_before_start_is_the_following_day() {
        let start = local_naive_timestamp(
            NaiveDate::from_ymd_opt(2026, 9, 15)
                .unwrap()
                .and_hms_opt(23, 0, 0)
                .unwrap(),
        )
        .unwrap();
        let stop = local_naive_timestamp(
            NaiveDate::from_ymd_opt(2026, 9, 16)
                .unwrap()
                .and_hms_opt(0, 2, 0)
                .unwrap(),
        )
        .unwrap();
        let edited = edited_time_range(
            TimeRange {
                started_at: UnixSeconds(start),
                stopped_at: UnixSeconds(stop),
            },
            TimeEndpoint::Stop,
            "02:20",
        )
        .unwrap();

        assert_eq!(start_label(edited), "23:00");
        assert_eq!(stop_label(edited), "02:20");
        assert_eq!(edited.duration_seconds(), Some(3 * 60 * 60 + 20 * 60));
    }

    #[test]
    fn unchanged_displayed_time_preserves_hidden_seconds() {
        let start = local_naive_timestamp(
            NaiveDate::from_ymd_opt(2026, 9, 16)
                .unwrap()
                .and_hms_opt(15, 2, 17)
                .unwrap(),
        )
        .unwrap();
        let stop = local_naive_timestamp(
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

        let edited = edited_time_range(original, TimeEndpoint::Start, "15:02").unwrap();
        assert_eq!(edited, original);
    }

    #[test]
    fn statistics_use_one_largest_unit() {
        assert_eq!(format_stat_duration(300 * 3600 + 42 * 60), "300 hours");
        assert_eq!(format_stat_duration(30 * 60 + 42), "30 minutes");
        assert_eq!(format_stat_duration(42), "42 seconds");
    }
}
