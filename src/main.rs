#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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
mod i18n;
mod local_time;
mod model;
mod pagination;
mod paths;
mod reports;
mod single_instance;
mod statistics;
mod storage;
mod theme;
mod tray;

use date_picker::DatePickerButton;
use github_activity::{
    heat_color, CalendarHeatmap, CellFill, Segment, SelectionChange as CalendarSelectionChange,
};
use i18n::{tr, Language};
use local_time::{datetime as local_datetime, TimeEndpoint};

use model::{
    Activity, ActivityId, EntryId, Project, ProjectId, ProjectStatus, RgbColor, Store, TimeRange,
    UnixSeconds, MAX_ENTRY_SECONDS,
};
use pagination::Pagination;
use paths::AppPaths;
use reports::{
    activity_project_stats, activity_stats, activity_totals, add_interval_to_days,
    project_activity_stats, project_stats, totals_by_local_day, totals_by_week, week_start,
    UsageStats,
};
use statistics::StatisticsView;
use storage::JsonStore;
use theme::{data_color, palette, Palette};

const ROW_HEIGHT: f32 = 26.0;
const TIMER_HEIGHT: f32 = 34.0;
const TIMER_BUTTON_WIDTH: f32 = 84.0;
const TIME_FIELD_WIDTH: f32 = 50.0;
const NAME_FIELD_WIDTH: f32 = 180.0;
/// Room for "100%" in the column before the totals.
const SHARE_WIDTH: f32 = 38.0;
const SHARE_BAR_SIZE: egui::Vec2 = egui::vec2(56.0, 6.0);
// Room for everything to the right of a row's two drop-downs.
const TIMER_CONTROLS_WIDTH: f32 = 264.0;
const ENTRY_CONTROLS_WIDTH: f32 = 292.0;
// How wide "00:00:00" is at most in the font of Windows, which the two
// widths above were measured with: in the timer and in an entry. A font with
// wider digits is given the difference on top of them.
const TIMER_CLOCK_WIDTH: f32 = 100.0;
const ENTRY_CLOCK_WIDTH: f32 = 64.0;
const MIN_SELECTOR_WIDTH: f32 = 110.0;
const MAX_SELECTOR_WIDTH: f32 = 300.0;
const CALENDAR_PAGE_WEEKS: i32 = 13;
const LEGEND_ITEMS: usize = 6;
// A day with this much tracked time gets the strongest regular heat color.
const FULL_DAY_SECONDS: f32 = 8.0 * 60.0 * 60.0;
const NO_PROJECT_COLOR: Color32 = Color32::from_gray(125);
const ENTRIES_PER_PAGE: usize = 100;
const UI_SELECTIONS_MEMORY_ID: &str = "opto_tracker_ui_selections_v1";
const SETTINGS_KEY: &str = "opto_tracker_settings";

fn main() -> eframe::Result {
    let app_paths = AppPaths::discover();
    // First of all, so that whatever is said below is said in the language
    // of the last run.
    if let Ok(paths) = &app_paths {
        i18n::set_language(remembered_language(&paths.persistence));
    }
    // Packaging starts nothing, so it comes before everything that does.
    #[cfg(target_os = "macos")]
    if write_requested_bundle() {
        return Ok(());
    }
    let directory = app_paths.as_ref().ok().map(|paths| &*paths.directory);
    let instance_guard = match single_instance::SingleInstanceGuard::acquire(directory) {
        Ok(Some(guard)) => guard,
        Ok(None) => {
            // The copy that runs is asked for its window where that is
            // possible, and pointed to where it is not.
            if !single_instance::SingleInstanceGuard::show_running_instance(directory) {
                single_instance::SingleInstanceGuard::show_already_running_message();
            }
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
            single_instance::SingleInstanceGuard::show_error_message(tr().usage);
            return Ok(());
        }
    }
    let app_paths = match app_paths {
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
            single_instance::SingleInstanceGuard::show_error_message(&tr().load_error(&error));
            return Ok(());
        }
    };
    let viewport = egui::ViewportBuilder::default()
        .with_title("OptoTracker")
        .with_icon(if cfg!(target_os = "macos") {
            // The Dock shows the icon of the bundle, which has a picture for
            // every size. An empty icon leaves it in place.
            egui::IconData::default()
        } else {
            app_icon::icon_data(false)
        })
        .with_visible(true)
        .with_inner_size([720.0, 620.0])
        .with_min_inner_size([420.0, 320.0]);
    // What a desktop of Linux finds the entry in its menu by, and with it
    // the icon, for a window that is open.
    #[cfg(target_os = "linux")]
    let viewport = viewport.with_app_id(desktop::APP_ID);
    let options = eframe::NativeOptions {
        viewport,
        persistence_path: Some(app_paths.persistence.clone()),
        event_loop_builder: desktop::event_loop_builder(),
        ..Default::default()
    };
    let instance_guard = &instance_guard;
    eframe::run_native(
        "OptoTracker",
        options,
        Box::new(move |cc| {
            configure_style(&cc.egui_ctx);
            configure_fonts(&cc.egui_ctx);
            Ok(Box::new(TimeTrackerApp::load(
                repository.clone(),
                store.clone(),
                app_paths.clone(),
                cc.storage,
                start_minimized,
                instance_guard,
                &cc.egui_ctx,
            )))
        }),
    )
}

/// `--bundle FOLDER` writes the application into the folder as
/// `OptoTracker.app` and does nothing else. Says whether that was asked for.
#[cfg(target_os = "macos")]
fn write_requested_bundle() -> bool {
    let mut arguments = std::env::args_os().skip(1);
    if arguments
        .next()
        .is_none_or(|argument| argument != "--bundle")
    {
        return false;
    }
    let (Some(folder), None) = (arguments.next(), arguments.next()) else {
        eprintln!("{}", tr().usage);
        std::process::exit(2);
    };
    match desktop::bundle_into(Path::new(&folder)) {
        Ok(bundle) => println!("{}", bundle.display()),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
    true
}

/// The language that an earlier run was left in. What is said before the
/// window exists is said in it: eframe hands the settings over only once the
/// window is there, so they are read here from the file it keeps them in, a
/// RON map of values that are RON themselves.
fn remembered_language(persistence: &Path) -> Language {
    let read = || {
        let file = fs::read_to_string(persistence).ok()?;
        let values: BTreeMap<String, String> = ron::from_str(&file).ok()?;
        let settings: PersistedSettings = ron::from_str(values.get(SETTINGS_KEY)?).ok()?;
        Some(settings.language)
    };
    read().unwrap_or_default()
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
    });
    for mode in [egui::Theme::Light, egui::Theme::Dark] {
        ctx.set_visuals_of(mode, theme::visuals(mode));
    }
    // For a system that does not say which theme it uses. Windows always does.
    ctx.options_mut(|options| options.fallback_theme = egui::Theme::Light);
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    // The system's UI font is easier to read at small sizes than egui's light
    // default, which remains the fallback for icons and for systems without
    // these files.
    let system = desktop::system_fonts();
    let regular = load_font(&mut fonts, "regular", &system.regular);
    let semibold = load_font(&mut fonts, "semibold", &system.semibold);

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

/// Registers the first font that can be read and returns its name.
fn load_font(
    fonts: &mut FontDefinitions,
    name: &str,
    candidates: &[desktop::SystemFont],
) -> Option<String> {
    let (font, bytes) = candidates.iter().find_map(|font| {
        let bytes = fs::read(&font.path).ok()?;
        // egui stops the application over a file that it cannot read as a
        // font, so the file is tried with the reader that egui uses.
        let readable = skrifa::FontRef::from_index(&bytes, font.index).is_ok();
        readable.then_some((font, bytes))
    })?;
    let mut data = FontData::from_owned(bytes);
    data.index = font.index;
    if let Some(weight) = font.weight {
        data.tweak.coords = egui::epaint::text::VariationCoords::new([("wght", weight)]);
    }
    fonts.font_data.insert(name.to_owned(), Arc::new(data));
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
    let (widths, height) = tabular_widths(ui, text, &font);

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

/// The slot of every character of `text` as [`tabular_digits`] shows it, and
/// the height of the row.
fn tabular_widths(ui: &egui::Ui, text: &str, font: &FontId) -> (Vec<f32>, f32) {
    // Whole pixels keep the slots evenly spaced after text is snapped to them.
    let pixels_per_point = ui.pixels_per_point();
    let whole_pixels = |width: f32| (width * pixels_per_point).ceil() / pixels_per_point;
    ui.fonts_mut(|fonts| {
        let digit_width = ('0'..='9')
            .map(|digit| fonts.glyph_width(font, digit))
            .fold(0.0, f32::max);
        let widths: Vec<f32> = text
            .chars()
            .map(|character| {
                whole_pixels(if character.is_ascii_digit() {
                    digit_width
                } else {
                    fonts.glyph_width(font, character)
                })
            })
            .collect();
        (widths, fonts.row_height(font))
    })
}

/// What a clock in `font` needs beyond the `expected` width that the row
/// around it was laid out for.
fn wider_clock(ui: &egui::Ui, font: FontId, expected: f32) -> f32 {
    let width: f32 = tabular_widths(ui, "00:00:00", &font).0.iter().sum();
    (width - expected).max(0.0)
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
    Statistics,
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
    theme: egui::ThemePreference,
    language: Language,
}

impl Default for PersistedSettings {
    fn default() -> Self {
        Self {
            close_or_minimize_to_tray: true,
            start_minimized_on_autorun: true,
            theme: egui::ThemePreference::System,
            language: Language::default(),
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
    statistics: StatisticsView,
    last_icon_running: Option<bool>,
    /// Whether the title bar was last told to keep a theme of its own.
    window_keeps_theme: Option<bool>,
    tray: Option<tray::TrayController>,
    /// Whether the window can be sent to the tray: there is a tray, and the
    /// window can leave the screen and come back.
    hides_to_tray: bool,
    /// Set from outside the interface when the window is asked for: by
    /// another copy that was started, or by a click on the icon in the Dock.
    show_requested: Arc<AtomicBool>,
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
        instance_guard: &single_instance::SingleInstanceGuard,
        ctx: &egui::Context,
    ) -> Self {
        let remembered = ctx
            .data_mut(|data| {
                data.get_persisted::<RememberedSelections>(egui::Id::new(UI_SELECTIONS_MEMORY_ID))
            })
            .map(|selections| selections.validated_for(&store))
            .unwrap_or_else(|| RememberedSelections::defaults_for(&store));
        let preferences: PersistedSettings = storage
            .and_then(|storage| eframe::get_value(storage, SETTINGS_KEY))
            .unwrap_or_default();
        desktop::watch_system_theme({
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        });
        ctx.set_theme(preferences.theme);
        // Before anything is worded, the menu of the tray icon included.
        i18n::set_language(preferences.language);
        let running = store.active_entry.is_some();
        let tray = tray::TrayController::new(running, {
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        });
        let message = match &tray {
            Ok(_) => String::new(),
            // A desktop of Linux without a tray is nothing out of the
            // ordinary. Settings says what follows from it.
            Err(_) if cfg!(target_os = "linux") => String::new(),
            Err(error) => format!("{}: {error}", tr().tray_not_created),
        };
        let tray = tray.ok();
        let hides_to_tray = tray.is_some() && desktop::window_can_hide();
        let window_hidden = start_minimized && hides_to_tray;
        if window_hidden {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        let show_requested = Arc::new(AtomicBool::new(false));
        let request_show = {
            let show_requested = Arc::clone(&show_requested);
            let ctx = ctx.clone();
            move || {
                show_requested.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            }
        };
        instance_guard.on_second_instance(request_show.clone());
        desktop::on_reopen(request_show);
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
            statistics: StatisticsView::load(ctx),
            last_icon_running: None,
            window_keeps_theme: None,
            tray,
            hides_to_tray,
            show_requested,
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
                self.message = match outcome.backup_warning {
                    Some(warning) => {
                        let warning = tr().backup_error(&warning);
                        format!("{}: {warning}", tr().saved_backup_warning)
                    }
                    None => tr().saved.to_owned(),
                };
            }
            Err(error) => {
                self.store = self.last_saved_store.clone();
                self.time_edit_buffers.clear();
                self.active_start_edit = None;
                self.message = format!("{}: {}", tr().save_failed, tr().save_error(&error));
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
            self.message = tr().model_error(&error);
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
                self.message = tr().model_error(&error);
                return;
            }
        };
        self.time_edit_buffers.clear();
        self.active_start_edit = None;
        self.draft_activity = active.activity;
        self.draft_project = active.project;
        self.save();
        if outcome.capped_at_limit && self.store.active_entry.is_none() {
            self.message = tr().stopped_at_limit.to_owned();
        }
    }

    fn timer_panel(&mut self, ui: &mut egui::Ui) {
        let activities = activity_options(&self.store, now());
        let projects = project_options(&self.store);
        let layout = RowLayout::new(
            ui.available_width(),
            TIMER_CONTROLS_WIDTH + wider_clock(ui, semibold_font(24.0), TIMER_CLOCK_WIDTH),
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
            if ui.add(button(tr().stop, palette(ui).stop)).clicked() {
                self.stop_timer();
            }
        } else if ui
            .add_enabled(can_start, button(tr().start, palette(ui).start))
            .clicked()
        {
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
            .on_hover_text(tr().start_time_hint);
        let shown = local_datetime(started_at.0).format("%H:%M").to_string();
        let commit = response.lost_focus()
            || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
        if commit && edit.time != shown {
            let current_time = UnixSeconds(now());
            let result = local_time::edit_active_start(started_at, current_time, &edit.time)
                .map_err(|error| tr().time_error(error))
                .and_then(|new_start| {
                    self.store
                        .set_timer_start(new_start, current_time)
                        .map_err(|error| tr().model_error(&error))
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
        let wider_entry_clock = wider_clock(ui, semibold_font(15.0), ENTRY_CLOCK_WIDTH);
        let entry_count = self.store.time_entries.len();
        let current_time = now();
        let activities = activity_options(&self.store, current_time);
        let projects = project_options(&self.store);
        let day_totals = totals_by_local_day(&self.store, current_time);
        let shown_color = |rgb: RgbColor| data_color(ui.visuals(), color(rgb));
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
                            .map(|project| (project.name.clone(), shown_color(project.color)))
                            .unwrap_or_else(|| (tr().no_project.to_owned(), NO_PROJECT_COLOR));
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
                                color: shown_color(activity.color),
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
                            // The controls move together to fit a narrow window.
                            if ui.available_width() < 380.0 {
                                ui.spacing_mut().item_spacing.x = 2.0;
                            }
                            if ui.button("⏴").on_hover_text(tr().earlier).clicked() {
                                self.calendar.scroll_weeks(CALENDAR_PAGE_WEEKS);
                            }
                            if ui.button("⏵").on_hover_text(tr().later).clicked() {
                                self.calendar.scroll_weeks(-CALENDAR_PAGE_WEEKS);
                            }
                            if self.calendar.is_scrolled() && ui.button(tr().this_year).clicked() {
                                self.calendar.reset_scroll();
                            }
                            right_aligned(ui, |ui| {
                                frameless_until_hovered(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.calendar_mode,
                                        CalendarMode::Activities,
                                        tr().activities,
                                    );
                                    ui.selectable_value(
                                        &mut self.calendar_mode,
                                        CalendarMode::Projects,
                                        tr().projects,
                                    );
                                    ui.selectable_value(
                                        &mut self.calendar_mode,
                                        CalendarMode::Time,
                                        tr().time,
                                    );
                                });
                            });
                        });
                    });
                    let calendar_mode = self.calendar_mode;
                    let dark_mode = ui.visuals().dark_mode;
                    let change = self.calendar.ui(
                        ui,
                        today,
                        self.selected_date,
                        &tr().calendar(),
                        |date| match calendar_mode {
                            CalendarMode::Time => day_totals
                                .get(&date)
                                .map(|day| {
                                    CellFill::Solid(heat_color(
                                        day.seconds as f32 / FULL_DAY_SECONDS,
                                        dark_mode,
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
                            ui.label(tr().date_with_weekday(date));
                            ui.label(
                                day_totals
                                    .get(&date)
                                    .map(|day| format_duration(day.seconds))
                                    .unwrap_or_else(|| tr().no_time_tracked.to_owned()),
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
                                            tr().no_project.to_owned(),
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
                                tr().week_total,
                                week_totals.get(&entry_week).copied().unwrap_or(0),
                            );
                        });
                    }
                    ui.add_space(4.0);
                    let indices = entries_by_day.get(&date).cloned().unwrap_or_default();
                    // One card per day: a header, then the entries divided by hairlines.
                    card_frame(ui).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 0.0;
                        Frame::new()
                            .fill(palette(ui).header)
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
                                        tr().day_total,
                                        day_totals.get(&date).map_or(0, |day| day.seconds),
                                    );
                                });
                            });
                        if indices.is_empty() {
                            hairline(ui);
                            entry_frame().show(ui, |ui| {
                                ui.label(RichText::new(tr().overnight_time).weak());
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
                                    ENTRY_CONTROLS_WIDTH + wider_entry_clock,
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
                                    egui::Shape::rect_filled(row_rect, 0.0, palette(ui).row_hover),
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
            .ok_or_else(|| tr().entry_gone.to_owned())
            .and_then(|entry| {
                local_time::edit_time(entry.time, edit.endpoint, &edit.text)
                    .map_err(|error| tr().time_error(error))
            });
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
            .ok_or_else(|| tr().entry_gone.to_owned())
            .and_then(|entry| {
                local_time::edit_date(entry.time, edit.date).map_err(|error| tr().time_error(error))
            });
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
                    .hint_text(tr().new_project)
                    .desired_width(220.0),
            );
            ui.color_edit_button_srgb(&mut self.new_project_color)
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            add_clicked = ui.button(tr().add).clicked();
            right_aligned(ui, |ui| {
                count_label(ui, tr().project_count(self.store.projects.len()));
            });
        });
        if add_clicked {
            let name = self.new_project_name.trim();
            if name.is_empty() {
                self.message = tr().project_name_empty.to_owned();
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
        table_page(ui, |ui| {
            let page_width = ui.available_width();
            let table_width = usage_table(
                ui,
                "projects",
                &["", "", tr().project, tr().status],
                tr().activities,
                |ui| {
                    for index in (0..self.store.projects.len()).rev() {
                        let expanded = disclosure_button(
                            ui,
                            egui::Id::new(("project-activities", index)),
                            stats[index].entries > 0,
                            [tr().show_activities_hint, tr().hide_activities_hint],
                        );
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
                        let status = project_status_combo(ui, index, &mut project.status);
                        changed |= status.changed();
                        usage_cells(ui, &stats[index], largest_total, color(project.color));
                        number_cell(ui, stats[index].related_count.to_string());
                        ui.end_row();

                        if expanded {
                            let activities =
                                project_activity_stats(&self.store, ProjectId(index), current_time);
                            let parts = PartRows {
                                whole: stats[index].total_seconds,
                                largest: activities
                                    .first()
                                    .map_or(0, |(_, stats)| stats.total_seconds),
                                // The names may use the room that the status
                                // leaves in front of the shares.
                                name_right: status.rect.right() - SHARE_WIDTH,
                            };
                            for (id, usage) in &activities {
                                let activity = &self.store.activities[id.0];
                                let swatch = Swatch::Activity(color(activity.color));
                                parts.show(ui, swatch, &activity.name, usage);
                            }
                        }
                    }
                },
            );
            ui.add_space(8.0);
            // As wide as the table, but no wider than the window: this card
            // is to be read without scrolling sideways.
            ui.scope(|ui| {
                ui.set_max_width(table_width.min(page_width));
                statistics::project_comparison(ui, &self.store, current_time);
            });
        });
        if changed {
            self.save();
        }
    }

    fn activities_panel(&mut self, ui: &mut egui::Ui) {
        let mut add_clicked = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_activity_name)
                    .hint_text(tr().new_activity)
                    .desired_width(220.0),
            );
            ui.color_edit_button_srgb(&mut self.new_activity_color)
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            add_clicked = ui.button(tr().add).clicked();
            right_aligned(ui, |ui| {
                count_label(ui, tr().activity_count(self.store.activities.len()));
            });
        });
        if add_clicked {
            let name = self.new_activity_name.trim();
            if name.is_empty() {
                self.message = tr().activity_name_empty.to_owned();
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
        let all_seconds: u64 = stats.iter().map(|stats| stats.total_seconds).sum();
        let mut changed = false;
        ui.add_space(8.0);
        let leading = ["", "", tr().activity, tr().share];
        table_page(ui, |ui| {
            usage_table(ui, "activities", &leading, tr().projects, |ui| {
                for index in activity_order {
                    let expanded = disclosure_button(
                        ui,
                        egui::Id::new(("activity-projects", index)),
                        stats[index].entries > 0,
                        [tr().show_projects_hint, tr().hide_projects_hint],
                    );
                    let activity = &mut self.store.activities[index];
                    changed |= color_cell(ui, &mut activity.color);
                    let name = ui.add(
                        egui::TextEdit::singleline(&mut activity.name)
                            .min_size(egui::vec2(NAME_FIELD_WIDTH, 0.0))
                            .desired_width(NAME_FIELD_WIDTH),
                    );
                    changed |= name.changed();
                    let share = format_share(stats[index].total_seconds, all_seconds);
                    number_cell(ui, RichText::new(share).weak());
                    usage_cells(ui, &stats[index], largest_total, color(activity.color));
                    number_cell(ui, stats[index].related_count.to_string());
                    ui.end_row();

                    if expanded {
                        let projects =
                            activity_project_stats(&self.store, ActivityId(index), current_time);
                        let parts = PartRows {
                            whole: stats[index].total_seconds,
                            largest: projects.first().map_or(0, |(_, stats)| stats.total_seconds),
                            name_right: name.rect.right(),
                        };
                        for (id, usage) in &projects {
                            let project = id.and_then(|id| self.store.projects.get(id.0));
                            let (swatch, name) = match project {
                                Some(project) => {
                                    (Swatch::Project(color(project.color)), &*project.name)
                                }
                                None => (Swatch::Empty, tr().no_project),
                            };
                            parts.show(ui, swatch, name, usage);
                        }
                    }
                }
            });
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
            ui.label(RichText::new(&self.settings_message).color(palette(ui).notice));
            ui.add_space(6.0);
        }

        card(ui, |ui| {
            section_title(ui, tr().appearance);
            ui.add_space(4.0);
            let language = self.preferences.language;
            ui.horizontal_wrapped(|ui| {
                ui.label(tr().language_label);
                for language in Language::ALL {
                    ui.radio_value(&mut self.preferences.language, language, language.name());
                }
            });
            if self.preferences.language != language {
                self.change_language();
            }
            let before = self.preferences.theme;
            ui.horizontal_wrapped(|ui| {
                ui.label(tr().theme_label);
                for (theme, name) in [
                    (egui::ThemePreference::System, tr().theme_system),
                    (egui::ThemePreference::Light, tr().theme_light),
                    (egui::ThemePreference::Dark, tr().theme_dark),
                ] {
                    ui.radio_value(&mut self.preferences.theme, theme, name);
                }
            });
            if self.preferences.theme != before {
                ui.ctx().set_theme(self.preferences.theme);
            }
            if self.preferences.theme == egui::ThemePreference::System {
                let app_mode = match ui.ctx().system_theme() {
                    Some(egui::Theme::Dark) => tr().follows_dark_system,
                    Some(egui::Theme::Light) => tr().follows_light_system,
                    None => tr().follows_unknown_system,
                };
                ui.label(RichText::new(app_mode).small().color(palette(ui).muted));
            }
        });

        ui.add_space(8.0);
        card(ui, |ui| {
            section_title(ui, tr().application_folder);
            ui.label(RichText::new(self.app_paths.directory.display().to_string()).monospace());
            ui.label(RichText::new(tr().application_folder_note).color(palette(ui).muted));
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(if desktop::is_installed(&self.app_paths) {
                        tr().update_installed
                    } else {
                        tr().install_here
                    })
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    match desktop::install_current_executable(&self.app_paths) {
                        Ok(()) => {
                            self.settings_message = tr().installed.to_owned();
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
                    .button(tr().open_folder)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    if let Err(error) = desktop::open_app_directory(&self.app_paths) {
                        self.settings_message = error;
                    }
                }
                if ui
                    .button(tr().create_shortcut)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    match desktop::create_desktop_shortcut(&self.app_paths) {
                        Ok(()) => {
                            self.settings_message = tr().shortcut_created.to_owned();
                        }
                        Err(error) => self.settings_message = error,
                    }
                }
            });
            ui.add_space(10.0);
            if desktop::is_installed(&self.app_paths) {
                ui.horizontal_wrapped(|ui| {
                    ui.label(tr().installed_executable);
                    ui.label(RichText::new(paths::INSTALLED_APPLICATION).monospace());
                });
            } else {
                ui.label(tr().not_installed);
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(tr().data);
                ui.label(RichText::new(self.repository.path().display().to_string()).monospace());
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(tr().persistence);
                ui.label(
                    RichText::new(self.app_paths.persistence.display().to_string()).monospace(),
                );
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(tr().weekly_backups);
                ui.label(RichText::new("backups").monospace());
            });
        });

        ui.add_space(8.0);
        card(ui, |ui| {
            section_title(ui, tr().system_behavior);
            ui.add_space(4.0);

            let mut autorun = self.autorun_enabled;
            if ui
                .checkbox(&mut autorun, tr().autorun)
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
                            tr().autorun_enabled.to_owned()
                        } else {
                            tr().autorun_disabled.to_owned()
                        };
                    }
                    Err(error) => self.settings_message = error,
                }
            }
            let minimized_changed = ui
                .checkbox(
                    &mut self.preferences.start_minimized_on_autorun,
                    tr().autorun_minimized,
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
                        self.settings_message = tr().autorun_updated.to_owned();
                    }
                    Err(error) => self.settings_message = error,
                }
            }

            ui.add_space(6.0);
            ui.add_enabled(
                self.hides_to_tray,
                egui::Checkbox::new(
                    &mut self.preferences.close_or_minimize_to_tray,
                    tr().to_tray,
                ),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
            #[cfg(target_os = "linux")]
            let note = if self.hides_to_tray {
                tr().to_tray_note
            } else {
                tr().to_tray_unavailable
            };
            #[cfg(not(target_os = "linux"))]
            let note = tr().to_tray_note;
            ui.label(RichText::new(note).small().color(palette(ui).muted));
        });
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        self.window_hidden = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    /// Puts the interface into the language that was just chosen.
    fn change_language(&mut self) {
        i18n::set_language(self.preferences.language);
        // These were worded in the language before.
        self.message.clear();
        self.settings_message.clear();
        if let Some(tray) = &self.tray {
            tray.translate();
        }
        // The descriptions of the tray and taskbar icons are written anew
        // together with the icons.
        self.last_icon_running = None;
    }

    fn handle_window_and_tray(&mut self, ctx: &egui::Context) {
        let show_requested = self.show_requested.swap(false, Ordering::Relaxed);
        let action = self.tray.as_ref().and_then(|tray| tray.poll_action());
        match action {
            Some(tray::TrayAction::Exit) => {
                self.exit_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Some(tray::TrayAction::Show) => self.show_window(ctx),
            None if show_requested => self.show_window(ctx),
            None => {}
        }

        let (close_requested, minimized) = ctx.input(|input| {
            (
                input.viewport().close_requested(),
                input.viewport().minimized == Some(true),
            )
        });
        if self.preferences.close_or_minimize_to_tray && self.hides_to_tray && !self.exit_requested
        {
            // A minimized window of macOS has its place in the Dock.
            let minimizes_to_tray = !cfg!(target_os = "macos");
            if close_requested {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.window_hidden = true;
            } else if minimized && !self.window_hidden && minimizes_to_tray {
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
                .send_viewport_cmd(egui::ViewportCommand::Icon(Some(Arc::new(
                    app_icon::icon_data(running),
                ))));
            app_icon::set_taskbar_running(frame, running);
            if let Some(tray) = &self.tray {
                if let Err(error) = tray.set_running(running) {
                    self.message = format!("{}: {error}", tr().tray_not_updated);
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
                    .fill(palette(ui).card)
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ui, |ui| {
                row(ui, ROW_HEIGHT, |ui| {
                    let (icon_rect, _) =
                        ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                    app_icon::paint(ui.painter(), icon_rect, running);
                    frameless_until_hovered(ui, |ui| {
                        // The tabs move together to fit a narrow window.
                        if ui.available_width() < 440.0 {
                            ui.spacing_mut().item_spacing.x = 2.0;
                        }
                        let texts = tr();
                        for (view, name) in [
                            (AppView::Entries, texts.time_entries),
                            (AppView::Projects, texts.projects),
                            (AppView::Activities, texts.activities),
                            (AppView::Statistics, texts.statistics),
                            (AppView::Settings, texts.settings),
                        ] {
                            ui.selectable_value(&mut self.view, view, name);
                        }
                    });
                    right_aligned(ui, |ui| {
                        if !self.message.is_empty() {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&self.message).color(palette(ui).muted),
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
                        .fill(palette(ui).card)
                        .inner_margin(egui::Margin::symmetric(12, 8)),
                )
                .show(ui, |ui| self.timer_panel(ui));
        }
        egui::CentralPanel::default()
            .frame(Frame::new().fill(palette(ui).page).inner_margin(12.0))
            .show(ui, |ui| {
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                match self.view {
                    AppView::Entries => self.entries_panel(ui),
                    AppView::Projects => self.projects_panel(ui),
                    AppView::Activities => self.activities_panel(ui),
                    AppView::Statistics => self.statistics.ui(ui, &self.store, now()),
                    AppView::Settings => self.settings_panel(ui),
                }
            });
        self.remember_selections(ui.ctx());

        let keeps_theme = self.preferences.theme != egui::ThemePreference::System;
        if self.window_keeps_theme != Some(keeps_theme) {
            desktop::keep_window_theme(frame, keeps_theme);
            self.window_keeps_theme = Some(keeps_theme);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS_KEY, &self.preferences);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // For a system whose windows are not told its theme.
        if let Some(theme) = desktop::system_theme() {
            raw_input.system_theme = Some(theme);
        }
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
        let shown = tr().entries_shown(first, last, total_entries);
        ui.label(RichText::new(shown).color(palette(ui).muted));
        right_aligned(ui, |ui| {
            if ui
                .add_enabled(pagination.has_next(total_entries), Button::new(tr().next))
                .clicked()
            {
                pagination.next(total_entries);
            }
            if ui
                .add_enabled(pagination.has_previous(), Button::new(tr().previous))
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
        .unwrap_or_else(|| (Swatch::Empty, RichText::new(tr().choose_activity).weak()));
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
    let no_project = || RichText::new(tr().no_project).weak();
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

    fn color(self) -> Color32 {
        match self {
            Self::Activity(color) | Self::Project(color) => color,
            Self::Empty => NO_PROJECT_COLOR,
        }
    }

    fn id() -> egui::Id {
        egui::Id::new("swatch")
    }

    fn paint(self, ui: &egui::Ui, rect: egui::Rect) {
        let painter = ui.painter();
        match self {
            Self::Activity(color) => {
                let color = data_color(ui.visuals(), color);
                painter.circle_filled(rect.center(), rect.width() * 0.5, color);
            }
            Self::Project(color) => {
                painter.rect_filled(rect, 2.0, data_color(ui.visuals(), color));
            }
            Self::Empty => {
                painter.rect_stroke(
                    rect,
                    2.0,
                    Stroke::new(1.0, palette(ui).empty_swatch),
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
        swatch.paint(ui, rect);
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
                    let strength = if hovered { 0.3 } else { 0.16 };
                    let tint = data_color(ui.visuals(), color).gamma_multiply(strength);
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

fn card_frame(ui: &egui::Ui) -> Frame {
    let palette = palette(ui);
    Frame::new()
        .fill(palette.card)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(5)
}

/// A bordered panel that spans the available width.
fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    card_frame(ui)
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
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0, palette(ui).hairline),
    );
}

/// How wide a text is when it is set in one line.
fn text_width(ui: &egui::Ui, text: &str, font: &FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font.clone(), Color32::PLACEHOLDER)
        .size()
        .x
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
        palette(ui).icon_disabled
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
        palette(ui).icon
    };
    (response, color)
}

fn play_icon_button(ui: &mut egui::Ui, enabled: bool) -> bool {
    let (response, color) = quiet_icon_button(ui, enabled, palette(ui).restart);
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
        .on_hover_text(tr().restart_hint)
        .on_disabled_hover_text(tr().stop_first_hint)
        .clicked()
}

fn cross_icon_button(ui: &mut egui::Ui) -> bool {
    let (response, color) = quiet_icon_button(ui, true, palette(ui).delete);
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
    response.on_hover_text(tr().delete_entry_hint).clicked()
}

fn project_status_combo(
    ui: &mut egui::Ui,
    index: usize,
    status: &mut ProjectStatus,
) -> egui::Response {
    let before = *status;
    let choices = [ProjectStatus::Active, ProjectStatus::Finished];
    let name = |status: ProjectStatus| match status {
        ProjectStatus::Active => tr().status_active,
        ProjectStatus::Finished => tr().status_finished,
    };
    // Every row gets a box as wide as the longer name needs, with the arrow
    // and the padding that egui puts around it.
    let font = egui::TextStyle::Button.resolve(ui.style());
    let name_width = choices
        .map(|choice| text_width(ui, name(choice), &font))
        .into_iter()
        .fold(0.0, f32::max);
    let spacing = ui.spacing();
    let width =
        name_width + spacing.icon_spacing + spacing.icon_width + 2.0 * spacing.button_padding.x;
    let mut response = ComboBox::from_id_salt(("project-status", index))
        .width(width.max(84.0))
        .selected_text(name(*status))
        .show_ui(ui, |ui| {
            for choice in choices {
                ui.selectable_value(status, choice, name(choice));
            }
        })
        .response
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if *status != before {
        response.mark_changed();
    }
    response
}

/// A triangle that shows or hides the rows below it, which hold the parts
/// of its own row. The hints say what a click does while the rows are
/// hidden and while they are shown. Like a collapsing header it keeps its
/// state in egui's memory. Returns whether the rows are shown.
fn disclosure_button(
    ui: &mut egui::Ui,
    id: egui::Id,
    enabled: bool,
    [show_hint, hide_hint]: [&str; 2],
) -> bool {
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
    let (response, color) = quiet_icon_button(ui, enabled, palette(ui).text);
    if response.clicked() {
        state.toggle(ui);
        state.store(ui.ctx());
    }
    // It points right while closed and turns down as it opens.
    let angle = egui::remap(
        state.openness(ui.ctx()),
        0.0..=1.0,
        -std::f32::consts::FRAC_PI_2..=0.0,
    );
    let rotation = egui::emath::Rot2::from_angle(angle);
    let center = response.rect.center();
    let points = [
        egui::vec2(-4.5, -2.5),
        egui::vec2(4.5, -2.5),
        egui::vec2(0.0, 3.5),
    ]
    .map(|offset| center + rotation * offset);
    ui.painter().add(egui::Shape::convex_polygon(
        points.to_vec(),
        color,
        Stroke::NONE,
    ));
    response.on_hover_text(if state.is_open() {
        hide_hint
    } else {
        show_hint
    });
    enabled && state.is_open()
}

/// The rows under an expanded row of a table, which split it into parts.
struct PartRows {
    /// The time of the expanded row.
    whole: u64,
    /// The time of its largest part.
    largest: u64,
    /// How far to the right a name may reach.
    name_right: f32,
}

impl PartRows {
    fn show(&self, ui: &mut egui::Ui, swatch: Swatch, name: &str, stats: &UsageStats) {
        ui.label("");
        ui.label("");
        overflowing_name(ui, swatch, name, self.name_right);
        let share = format_share(stats.total_seconds, self.whole);
        number_cell(ui, RichText::new(share).weak());
        usage_cells(ui, stats, self.largest, swatch.color());
        ui.end_row();
    }
}

/// A swatch and a name in a table cell. A name wider than its column runs on
/// over the cells that follow, as far as `right`.
fn overflowing_name(ui: &mut egui::Ui, swatch: Swatch, name: &str, right: f32) {
    // The margin of a text field, to line up with the names in those.
    let swatch_left = ui.cursor().left() + 4.0;
    let text_left = swatch_left + Swatch::SIZE + 6.0;
    let galley = egui::WidgetText::from(name).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (right - text_left).max(0.0),
        egui::TextStyle::Body,
    );
    let width = (text_left - ui.cursor().left() + galley.size().x).min(ui.available_width());
    let size = egui::vec2(width, ui.spacing().interact_size.y);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    swatch.paint(
        ui,
        egui::Rect::from_center_size(
            egui::pos2(swatch_left + Swatch::SIZE * 0.5, rect.center().y),
            egui::Vec2::splat(Swatch::SIZE),
        ),
    );
    let text_top = rect.center().y - galley.size().y * 0.5;
    let elided = galley.elided;
    ui.painter().galley(
        egui::pos2(text_left, text_top),
        galley,
        ui.visuals().text_color(),
    );
    if elided {
        response.on_hover_text(name);
    }
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
    let caption_color = palette(ui).caption;
    let caption = |text: &str| {
        RichText::new(text.to_owned())
            .font(FontId::proportional(11.0))
            .color(caption_color)
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
                    let color = heat_color(value, ui.visuals().dark_mode);
                    ui.painter().rect_filled(rect, 2.0, color);
                    response.on_hover_text(meaning);
                };
                let texts = tr();
                ui.label(caption(texts.legend_more));
                step(ui, 1.5, texts.more_than_8_hours);
                ui.add_space(8.0);
                ui.label(caption(texts.legend_full_day));
                step(ui, 0.9, texts.from_6_to_8_hours);
                step(ui, 0.6, texts.from_4_to_6_hours);
                step(ui, 0.3, texts.from_2_to_4_hours);
                step(ui, 0.1, texts.less_than_2_hours);
                step(ui, 0.0, texts.no_time_tracked);
                ui.label(caption(texts.legend_nothing));
            });
        } else {
            swatch_legend(ui, items);
        }
    });
}

/// The names behind some colors, in a line that wraps. Only the first few
/// are spelled out.
fn swatch_legend(ui: &mut egui::Ui, items: &[(Swatch, String)]) {
    let font = FontId::proportional(11.0);
    let caption = palette(ui).caption;
    let text_offset = Swatch::SIZE + 4.0;
    let text_width = (ui.available_width() - text_offset).max(0.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        for (swatch, name) in items.iter().take(LEGEND_ITEMS) {
            // Each entry is a single widget, so that a line break never
            // separates a swatch from its name.
            let mut job =
                egui::text::LayoutJob::simple_singleline(name.clone(), font.clone(), caption);
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
            swatch.paint(ui, swatch_rect);
            let text_top = rect.center().y - galley.size().y * 0.5;
            ui.painter().galley(
                egui::pos2(rect.left() + text_offset, text_top),
                galley,
                caption,
            );
        }
        if items.len() > LEGEND_ITEMS {
            let more = tr().more(items.len() - LEGEND_ITEMS);
            ui.label(RichText::new(more).font(font).color(caption));
        }
    });
}

/// The Projects and Activities tables share their statistics columns, which
/// follow the `leading` ones. Returns the width of the table's card.
fn usage_table(
    ui: &mut egui::Ui,
    id: &str,
    leading: &[&str],
    related: &str,
    rows: impl FnOnce(&mut egui::Ui),
) -> f32 {
    let heading = |text: &str| RichText::new(text.to_owned()).weak();
    // Unlike the other cards this one hugs its table, so that the row
    // stripes reach its edges in a wide window.
    let card = card_frame(ui).inner_margin(10.0).show(ui, |ui| {
        egui::Grid::new(id)
            .spacing([12.0, 6.0])
            .min_col_width(0.0)
            .with_row_color(|row, style| {
                (row > 0 && row % 2 == 0).then_some(Palette::of(&style.visuals).stripe)
            })
            .show(ui, |ui| {
                for title in leading {
                    ui.label(heading(title));
                }
                let texts = tr();
                number_cell(ui, heading(texts.total));
                ui.label("");
                for title in [
                    texts.entries,
                    texts.active_days,
                    texts.first_used,
                    texts.last_used,
                    related,
                ] {
                    number_cell(ui, heading(title));
                }
                ui.end_row();
                rows(ui);
            });
    });
    card.response.rect.width()
}

/// A page that scrolls both ways, for a table that is wider than the window.
fn table_page(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ScrollArea::both().auto_shrink([false, false]).show(ui, add);
}

/// The statistics columns of one row, up to the count that ends it.
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
    let (track, _) = ui.allocate_exact_size(SHARE_BAR_SIZE, egui::Sense::hover());
    paint_share_bar(ui, track, share, color);
}

fn paint_share_bar(ui: &egui::Ui, track: egui::Rect, share: f32, color: Color32) {
    let painter = ui.painter();
    painter.rect_filled(track, 3.0, palette(ui).track);
    if share > 0.0 {
        let mut bar = track;
        bar.set_width((track.width() * share).max(2.0));
        painter.rect_filled(bar, 3.0, data_color(ui.visuals(), color));
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
        .map(|value| tr().date(local_datetime(value).date_naive()))
        .unwrap_or_else(|| "-".to_owned())
}

fn week_title(week: NaiveDate, this_week: NaiveDate) -> String {
    if week == this_week {
        tr().this_week.to_owned()
    } else if week == this_week - ChronoDuration::weeks(1) {
        tr().last_week.to_owned()
    } else {
        tr().week_of(week)
    }
}

fn day_title(date: NaiveDate, today: NaiveDate) -> String {
    if date == today {
        tr().today.to_owned()
    } else if date == today - ChronoDuration::days(1) {
        tr().yesterday.to_owned()
    } else {
        tr().date_with_weekday(date)
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
    let picker = DatePickerButton::new(&mut selected, ("entry-date", entry_id), tr().date_picker());
    let response = ui
        .add(picker.year_range(2000..=3000))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tr().change_date(original));
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
        tr().hour_count(seconds / 3600)
    } else if seconds >= 60 {
        tr().minute_count(seconds / 60)
    } else {
        tr().second_count(seconds)
    }
}

/// A duration to the minute, or to the hour once the minutes stop mattering.
fn format_hours_minutes(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = seconds % 3600 / 60;
    let (h, m) = (tr().hours_letter, tr().minutes_letter);
    if hours >= 100 {
        format!("{hours} {h}")
    } else if hours > 0 {
        format!("{hours} {h} {minutes} {m}")
    } else {
        format!("{minutes} {m}")
    }
}

/// What part of `total` the given seconds are, in whole percent.
fn format_share(seconds: u64, total: u64) -> String {
    let percent = seconds as f64 * 100.0 / total.max(1) as f64;
    if seconds > 0 && percent < 1.0 {
        "<1%".to_owned()
    } else {
        format!("{percent:.0}%")
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
    fn the_language_is_read_from_the_settings_that_eframe_keeps() {
        let directory = std::env::temp_dir().join(format!(
            "opto-tracker-language-{}-{}",
            std::process::id(),
            now()
        ));
        fs::create_dir_all(&directory).unwrap();
        let file = directory.join("temp.ron");
        let settings = |settings: &str| {
            let contents = format!(
                r#"{{
    "window": "(fullscreen:false,maximized:false)",
    "{SETTINGS_KEY}": "{settings}",
}}"#
            );
            fs::write(&file, contents).unwrap();
            remembered_language(&file)
        };

        assert_eq!(
            settings("(close_or_minimize_to_tray:true,theme:Light,language:Russian)"),
            Language::Russian
        );
        // Written by a version that had one language.
        assert_eq!(
            settings("(close_or_minimize_to_tray:true,theme:Light)"),
            Language::English
        );
        fs::write(&file, "not RON").unwrap();
        assert_eq!(remembered_language(&file), Language::English);
        fs::remove_dir_all(&directory).unwrap();
        assert_eq!(remembered_language(&file), Language::English);
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
