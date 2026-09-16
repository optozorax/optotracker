use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Local};
use eframe::egui::{
    self, Align, Button, Color32, ComboBox, Frame, Layout, RichText, ScrollArea, Stroke,
};

#[path = "../clockify_model.rs"]
mod model;

use model::{
    ActiveTimeEntry, ActivityId, ProjectId, RgbColor, Store, TimeEntry, TimeRange, UnixSeconds,
};

const BLUE: Color32 = Color32::from_rgb(3, 169, 244);
const RED: Color32 = Color32::from_rgb(244, 67, 54);
const PAGE: Color32 = Color32::from_rgb(242, 247, 250);
const HEADER: Color32 = Color32::from_rgb(228, 236, 241);
const BORDER: Color32 = Color32::from_rgb(218, 227, 232);

fn main() -> eframe::Result {
    let store_path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_store_path);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Personal Time Tracker")
            .with_inner_size([1200.0, 780.0])
            .with_min_inner_size([760.0, 520.0]),
        ..Default::default()
    };

    eframe::run_native(
        "personal-time-tracker",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::light());
            Ok(Box::new(TimeTrackerApp::load(store_path.clone())))
        }),
    )
}

struct TimeTrackerApp {
    store_path: PathBuf,
    store: Store,
    draft_activity: ActivityId,
    draft_project: Option<ProjectId>,
    message: String,
}

impl TimeTrackerApp {
    fn load(store_path: PathBuf) -> Self {
        match read_store(&store_path) {
            Ok(store) => Self {
                store_path,
                store,
                draft_activity: ActivityId(0),
                draft_project: None,
                message: String::new(),
            },
            Err(error) => Self {
                store_path,
                store: Store::default(),
                draft_activity: ActivityId(0),
                draft_project: None,
                message: format!("Could not load data: {error}"),
            },
        }
    }

    fn save(&mut self) {
        match write_store(&self.store_path, &self.store) {
            Ok(()) => self.message = "Saved".to_owned(),
            Err(error) => self.message = format!("Save failed: {error}"),
        }
    }

    fn start_timer(&mut self) {
        if self.store.active_entry.is_some() || self.store.activities.is_empty() {
            return;
        }
        self.store.active_entry = Some(ActiveTimeEntry {
            activity: self.draft_activity,
            project: self.draft_project,
            started_at: UnixSeconds(now()),
        });
        self.save();
    }

    fn stop_timer(&mut self) {
        let Some(active) = self.store.active_entry.take() else {
            return;
        };
        let stopped_at = UnixSeconds(now().max(active.started_at.0));
        self.store.time_entries.push(TimeEntry {
            activity: active.activity,
            project: active.project,
            time: TimeRange {
                started_at: active.started_at,
                stopped_at,
            },
        });
        self.store
            .time_entries
            .sort_by_key(|entry| entry.time.started_at);
        self.draft_activity = active.activity;
        self.draft_project = active.project;
        self.save();
    }

    fn timer_panel(&mut self, ui: &mut egui::Ui) {
        let activity_names: Vec<String> = self
            .store
            .activities
            .iter()
            .map(|activity| activity.name.clone())
            .collect();
        let project_names: Vec<String> = self
            .store
            .projects
            .iter()
            .map(|project| project.name.clone())
            .collect();
        let mut active_changed = false;

        Frame::new()
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.0, BORDER))
            .inner_margin(18.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_height(42.0);
                    let available = (ui.available_width() - 420.0).max(180.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(available, 42.0),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            if let Some(active) = self.store.active_entry.as_mut() {
                                active_changed |= activity_combo(
                                    ui,
                                    "active-activity",
                                    &mut active.activity,
                                    &activity_names,
                                );
                            } else {
                                activity_combo(
                                    ui,
                                    "draft-activity",
                                    &mut self.draft_activity,
                                    &activity_names,
                                );
                            }
                        },
                    );

                    let project = self
                        .store
                        .active_entry
                        .as_mut()
                        .map(|entry| &mut entry.project)
                        .unwrap_or(&mut self.draft_project);
                    let project_changed =
                        project_combo(ui, "timer-project", project, &project_names);
                    active_changed |= self.store.active_entry.is_some() && project_changed;

                    ui.separator();
                    if let Some(active) = &self.store.active_entry {
                        let elapsed = now().saturating_sub(active.started_at.0) as u64;
                        ui.label(RichText::new(format_duration(elapsed)).size(20.0).strong());
                        if ui
                            .add_sized(
                                [90.0, 36.0],
                                Button::new(RichText::new("STOP").color(Color32::WHITE)).fill(RED),
                            )
                            .clicked()
                        {
                            self.stop_timer();
                        }
                        ui.ctx().request_repaint_after(Duration::from_secs(1));
                    } else if ui
                        .add_enabled(
                            !activity_names.is_empty(),
                            Button::new(RichText::new("START").color(Color32::WHITE))
                                .fill(BLUE)
                                .min_size(egui::vec2(90.0, 36.0)),
                        )
                        .clicked()
                    {
                        self.start_timer();
                    }
                });
            });

        if active_changed {
            self.save();
        }
    }

    fn entries_panel(&mut self, ui: &mut egui::Ui) {
        let activity_names: Vec<String> = self
            .store
            .activities
            .iter()
            .map(|activity| activity.name.clone())
            .collect();
        let project_names: Vec<String> = self
            .store
            .projects
            .iter()
            .map(|project| project.name.clone())
            .collect();
        let project_colors: Vec<Color32> = self
            .store
            .projects
            .iter()
            .map(|project| color(project.color))
            .collect();

        let mut changed = false;
        let mut delete = None;
        let mut current_day: Option<String> = None;
        let mut shown = 0usize;

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for index in (0..self.store.time_entries.len()).rev() {
                    if shown == 250 {
                        break;
                    }
                    shown += 1;
                    let entry = &mut self.store.time_entries[index];
                    let day = local_datetime(entry.time.started_at.0)
                        .format("%A, %B %-d, %Y")
                        .to_string();

                    if current_day.as_deref() != Some(day.as_str()) {
                        current_day = Some(day.clone());
                        ui.add_space(12.0);
                        Frame::new()
                            .fill(HEADER)
                            .inner_margin(egui::Margin::symmetric(14, 9))
                            .show(ui, |ui| {
                                ui.label(RichText::new(day).color(Color32::from_gray(95)).strong());
                            });
                    }

                    Frame::new()
                        .fill(Color32::WHITE)
                        .stroke(Stroke::new(1.0, BORDER))
                        .inner_margin(egui::Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                changed |= activity_combo(
                                    ui,
                                    ("entry-activity", index),
                                    &mut entry.activity,
                                    &activity_names,
                                );

                                if let Some(project) = entry.project {
                                    ui.colored_label(
                                        project_colors.get(project.0).copied().unwrap_or(BLUE),
                                        "●",
                                    );
                                } else {
                                    ui.label("○");
                                }
                                changed |= project_combo(
                                    ui,
                                    ("entry-project", index),
                                    &mut entry.project,
                                    &project_names,
                                );

                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button("Delete").clicked() {
                                        delete = Some(index);
                                    }
                                    ui.separator();
                                    ui.label(
                                        RichText::new(format_duration(
                                            entry.time.duration_seconds().unwrap_or(0),
                                        ))
                                        .size(18.0)
                                        .strong(),
                                    );
                                    ui.label(format!(
                                        "{} – {}",
                                        local_datetime(entry.time.started_at.0).format("%H:%M"),
                                        local_datetime(entry.time.stopped_at.0).format("%H:%M")
                                    ));
                                });
                            });
                        });
                }
            });

        if let Some(index) = delete {
            self.store.time_entries.remove(index);
            changed = true;
        }
        if changed {
            self.save();
        }
    }
}

impl eframe::App for TimeTrackerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("top-bar")
            .exact_size(58.0)
            .frame(Frame::new().fill(Color32::WHITE).inner_margin(14.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("◷").size(26.0).color(BLUE));
                    ui.label(RichText::new("personal tracker").size(24.0).strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if !self.message.is_empty() {
                            ui.label(RichText::new(&self.message).color(Color32::from_gray(110)));
                        }
                        ui.label(
                            RichText::new(self.store_path.display().to_string())
                                .small()
                                .color(Color32::from_gray(130)),
                        );
                    });
                });
            });

        egui::Panel::left("sidebar")
            .exact_size(190.0)
            .frame(Frame::new().fill(Color32::WHITE).inner_margin(18.0))
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.label(RichText::new("TIME TRACKER").color(BLUE).strong());
                ui.add_space(24.0);
                ui.label(RichText::new("DATA").small().color(Color32::from_gray(145)));
                ui.add_space(10.0);
                ui.label(format!("{} entries", self.store.time_entries.len()));
                ui.label(format!("{} projects", self.store.projects.len()));
                ui.label(format!("{} activities", self.store.activities.len()));
            });

        egui::CentralPanel::default()
            .frame(Frame::new().fill(PAGE).inner_margin(24.0))
            .show(ui, |ui| {
                self.timer_panel(ui);
                ui.add_space(26.0);
                ui.heading("Recent entries");
                ui.label(
                    RichText::new("The newest 250 entries are shown.")
                        .small()
                        .color(Color32::from_gray(120)),
                );
                self.entries_panel(ui);
            });
    }
}

fn activity_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut ActivityId,
    names: &[String],
) -> bool {
    let before = *value;
    let selected = names
        .get(value.0)
        .map(String::as_str)
        .unwrap_or("Choose activity");
    ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(220.0)
        .show_ui(ui, |ui| {
            for (index, name) in names.iter().enumerate() {
                ui.selectable_value(value, ActivityId(index), name);
            }
        });
    *value != before
}

fn project_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut Option<ProjectId>,
    names: &[String],
) -> bool {
    let before = *value;
    let selected = value
        .and_then(|id| names.get(id.0))
        .map(String::as_str)
        .unwrap_or("No project");
    ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(190.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, "No project");
            for (index, name) in names.iter().enumerate() {
                ui.selectable_value(value, Some(ProjectId(index)), name);
            }
        });
    *value != before
}

fn read_store(path: &Path) -> Result<Store, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

fn default_store_path() -> PathBuf {
    let relative = PathBuf::from("data/time_tracker_store.json");
    if relative.exists() {
        relative
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join("time_tracker_store.json")
    }
}

fn write_store(path: &Path, store: &Store) -> Result<(), String> {
    let mut json = serde_json::to_string_pretty(store).map_err(|error| error.to_string())?;
    json.push('\n');
    fs::write(path, json).map_err(|error| error.to_string())
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn local_datetime(seconds: i64) -> DateTime<Local> {
    DateTime::from_timestamp(seconds, 0)
        .unwrap_or(DateTime::UNIX_EPOCH)
        .with_timezone(&Local)
}

fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = seconds % 3600 / 60;
    let seconds = seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn color(color: RgbColor) -> Color32 {
    Color32::from_rgb(color.red, color.green, color.blue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converted_store_loads_and_all_indices_are_valid() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join("time_tracker_store.json");
        let store = read_store(&path).expect("converted store must load");

        assert!(!store.time_entries.is_empty());
        for entry in &store.time_entries {
            assert!(entry.activity.0 < store.activities.len());
            assert!(entry
                .project
                .is_none_or(|project| project.0 < store.projects.len()));
            assert!(entry.time.stopped_at >= entry.time.started_at);
        }
        if let Some(entry) = &store.active_entry {
            assert!(entry.activity.0 < store.activities.len());
            assert!(entry
                .project
                .is_none_or(|project| project.0 < store.projects.len()));
        }
    }
}
