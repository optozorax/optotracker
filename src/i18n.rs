//! The words of the interface in each language it can be shown in.
//!
//! One language is in use at a time. It is chosen in Settings and kept here
//! for the whole process, so code that shows a text asks [`tr`] for it
//! instead of being handed the language.
//!
//! A text that is shown as it stands is a line of the table below, with its
//! translations next to each other. A text around a number, a name or a date
//! is a method of [`Texts`]. The model, the storage and the calendar policy
//! know nothing of languages: they report errors as values, which are put
//! into words here.

use std::sync::atomic::{AtomicU8, Ordering};

use chrono::{Datelike as _, NaiveDate};

use crate::local_time::TimeError;
use crate::model::ModelError;
use crate::storage::{BackupError, LoadError, SaveError};
use crate::{date_picker, github_activity};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum Language {
    #[default]
    English,
    Russian,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::English, Self::Russian];

    /// What the language calls itself, so that it can be found in a list by
    /// someone who cannot read the language that is in use.
    pub fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Russian => "Русский",
        }
    }

    pub fn texts(self) -> &'static Texts {
        match self {
            Self::English => &ENGLISH,
            Self::Russian => &RUSSIAN,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(Language::English as u8);

pub fn set_language(language: Language) {
    CURRENT.store(language as u8, Ordering::Relaxed);
}

/// The texts in the language of the interface.
pub fn tr() -> &'static Texts {
    let current = CURRENT.load(Ordering::Relaxed);
    Language::ALL
        .into_iter()
        .find(|&language| language as u8 == current)
        .unwrap_or_default()
        .texts()
}

/// Declares [`Texts`] and fills it in for every language, from lines of the
/// form `name: "English", "Russian";`.
///
/// A line may be preceded by `#[cfg(...)]`. That is how a text that names
/// the system, or that only one of the systems needs, is given once for each
/// of them. `freedesktop` stands for Linux and FreeBSD, whose desktops are
/// the same.
macro_rules! texts {
    ($($(#[$system:meta])* $name:ident: $english:literal, $russian:literal;)*) => {
        /// The texts of one language.
        pub struct Texts {
            language: Language,
            $($(#[$system])* pub $name: &'static str,)*
        }

        static ENGLISH: Texts = Texts {
            language: Language::English,
            $($(#[$system])* $name: $english,)*
        };

        static RUSSIAN: Texts = Texts {
            language: Language::Russian,
            $($(#[$system])* $name: $russian,)*
        };
    };
}

texts! {
    // The tabs, and the same words wherever else they head something.
    time_entries: "Time entries", "Записи";
    projects: "Projects", "Проекты";
    activities: "Activities", "Занятия";
    statistics: "Statistics", "Статистика";
    settings: "Settings", "Настройки";

    // The timer.
    start: "START", "СТАРТ";
    stop: "STOP", "СТОП";
    start_time_hint:
        "Start time (HH:MM); later times count as yesterday",
        "Время начала (ЧЧ:ММ); время позже текущего считается вчерашним";
    choose_activity: "Choose activity", "Выберите занятие";
    no_project: "No project", "Без проекта";
    stopped_at_limit:
        "Timer stopped at the 24-hour limit",
        "Таймер остановлен: достигнут предел в 24 часа";

    // What happened to the last change, next to the tabs.
    saved: "Saved", "Сохранено";
    saved_backup_warning:
        "Saved; backup warning",
        "Сохранено; предупреждение о резервной копии";
    save_failed:
        "Save failed; change was reverted",
        "Не удалось сохранить; изменение отменено";
    entry_gone: "The entry no longer exists", "Запись больше не существует";
    #[cfg(not(target_os = "macos"))]
    tray_not_created: "Could not create tray icon", "Не удалось создать значок в трее";
    #[cfg(target_os = "macos")]
    tray_not_created:
        "Could not create the menu bar icon",
        "Не удалось создать значок в строке меню";
    #[cfg(not(target_os = "macos"))]
    tray_not_updated: "Could not update tray icon", "Не удалось обновить значок в трее";
    #[cfg(target_os = "macos")]
    tray_not_updated:
        "Could not update the menu bar icon",
        "Не удалось обновить значок в строке меню";

    // The calendar.
    earlier: "Earlier", "Раньше";
    later: "Later", "Позже";
    this_year: "This year", "Этот год";
    time: "Time", "Время";
    no_time_tracked: "No time tracked", "Нет учтённого времени";
    legend_nothing: "0h", "0ч";
    legend_full_day: "8h", "8ч";
    legend_more: "more than 8h", "больше 8ч";
    more_than_8_hours: "More than 8 hours", "Больше 8 часов";
    from_6_to_8_hours: "6 to 8 hours", "От 6 до 8 часов";
    from_4_to_6_hours: "4 to 6 hours", "От 4 до 6 часов";
    from_2_to_4_hours: "2 to 4 hours", "От 2 до 4 часов";
    less_than_2_hours: "Less than 2 hours", "Меньше 2 часов";

    // The list of entries.
    today: "Today", "Сегодня";
    yesterday: "Yesterday", "Вчера";
    this_week: "This week", "Эта неделя";
    last_week: "Last week", "Прошлая неделя";
    week_total: "Week total:", "За неделю:";
    day_total: "Day total:", "За день:";
    overnight_time:
        "Time carried over from an overnight entry",
        "Время из записи, перешедшей через полночь";
    restart_hint:
        "Start a timer with this activity and project",
        "Запустить таймер с этим занятием и проектом";
    stop_first_hint: "Stop the running timer first", "Сначала остановите запущенный таймер";
    delete_entry_hint: "Delete this entry", "Удалить эту запись";
    next: "Next", "Далее";
    previous: "Previous", "Назад";

    // The date picker.
    previous_year: "Previous year", "Предыдущий год";
    previous_month: "Previous month", "Предыдущий месяц";
    next_month: "Next month", "Следующий месяц";
    next_year: "Next year", "Следующий год";
    save: "Save", "Сохранить";
    cancel: "Cancel", "Отмена";

    // The tables of projects and of activities.
    new_project: "New project", "Новый проект";
    new_activity: "New activity", "Новое занятие";
    add: "Add", "Добавить";
    project_name_empty:
        "Project name cannot be empty",
        "Название проекта не может быть пустым";
    activity_name_empty:
        "Activity name cannot be empty",
        "Название занятия не может быть пустым";
    project: "Project", "Проект";
    activity: "Activity", "Занятие";
    status: "Status", "Статус";
    share: "Share", "Доля";
    total: "Total", "Всего";
    entries: "Entries", "Записи";
    active_days: "Active days", "Активные дни";
    first_used: "First used", "Первый раз";
    last_used: "Last used", "Последний раз";
    status_active: "Active", "Активен";
    status_finished: "Finished", "Завершён";
    show_activities_hint: "Show the time of its activities", "Показать время по занятиям";
    hide_activities_hint: "Hide its activities", "Скрыть занятия";
    show_projects_hint: "Show the time of its projects", "Показать время по проектам";
    hide_projects_hint: "Hide its projects", "Скрыть проекты";
    projects_compared: "Projects compared", "Сравнение проектов";
    by_activity: "by activity", "по занятиям";
    all_projects: "All projects", "Все проекты";

    // Statistics.
    nothing_tracked: "Nothing has been tracked yet.", "Пока ничего не учтено.";
    week: "Week", "Неделя";
    month: "Month", "Месяц";
    year: "Year", "Год";
    all_time: "All time", "Всё время";
    custom: "Custom", "Период";
    tracked: "Tracked", "Учтено";
    days_worked: "Days worked", "Рабочих дней";
    per_worked_day: "Per worked day", "За рабочий день";
    busiest_day: "Busiest day", "Самый загруженный день";
    time_per_day: "Time per day", "Время по дням";
    time_per_week: "Time per week", "Время по неделям";
    time_per_month: "Time per month", "Время по месяцам";
    show_fewer: "Show fewer", "Показать меньше";
    habits: "Habits", "Привычки";
    average_weekday: "An average day of the week", "Средний день недели";
    time_by_hour: "Time by hour of the day", "Время по часам суток";
    typical_session: "Typical session", "Типичная сессия";
    typical_session_hint:
        "Half of the sessions are longer than this, half are shorter. \
         Entries less than five minutes apart are one session.",
        "Половина сессий длиннее, половина короче. \
         Записи с перерывом меньше пяти минут считаются одной сессией.";
    longest_session: "Longest session", "Самая длинная сессия";
    without_break: "without a break", "без перерыва";
    longest_streak: "Longest streak", "Самая длинная серия";
    longest_streak_hint:
        "The most days in a row with tracked time in this period.",
        "Наибольшее число дней подряд с учтённым временем за этот период.";
    current_streak: "Current streak", "Текущая серия";
    in_a_row_until_now: "in a row until now", "подряд на сегодня";
    current_streak_hint:
        "Days in a row with tracked time, up to today.",
        "Дни подряд с учтённым временем, вплоть до сегодняшнего.";

    // The letters after the hours and the minutes of a duration.
    hours_letter: "h", "ч";
    minutes_letter: "m", "м";

    // Settings.
    appearance: "Appearance", "Внешний вид";
    language_label: "Language:", "Язык:";
    theme_label: "Theme:", "Тема:";
    #[cfg(target_os = "windows")]
    theme_system: "Same as Windows", "Как в Windows";
    #[cfg(target_os = "macos")]
    theme_system: "Same as macOS", "Как в macOS";
    #[cfg(freedesktop)]
    theme_system: "Same as the system", "Как в системе";
    theme_light: "Light", "Светлая";
    theme_dark: "Dark", "Тёмная";
    #[cfg(target_os = "windows")]
    follows_dark_system:
        "Follows the app mode in the color settings of Windows, which is dark now.",
        "Следует режиму приложений в настройках цветов Windows; сейчас он тёмный.";
    #[cfg(target_os = "windows")]
    follows_light_system:
        "Follows the app mode in the color settings of Windows, which is light now.",
        "Следует режиму приложений в настройках цветов Windows; сейчас он светлый.";
    #[cfg(target_os = "windows")]
    follows_unknown_system:
        "Follows the app mode in the color settings of Windows, which could not be read.",
        "Следует режиму приложений в настройках цветов Windows, но определить его не удалось.";
    #[cfg(target_os = "macos")]
    follows_dark_system:
        "Follows the appearance in the settings of macOS, which is dark now.",
        "Следует оформлению в настройках macOS; сейчас оно тёмное.";
    #[cfg(target_os = "macos")]
    follows_light_system:
        "Follows the appearance in the settings of macOS, which is light now.",
        "Следует оформлению в настройках macOS; сейчас оно светлое.";
    #[cfg(target_os = "macos")]
    follows_unknown_system:
        "Follows the appearance in the settings of macOS, which could not be read.",
        "Следует оформлению в настройках macOS, но определить его не удалось.";
    #[cfg(freedesktop)]
    follows_dark_system:
        "Follows the color scheme of the desktop, which is dark now.",
        "Следует цветовой схеме рабочего стола; сейчас она тёмная.";
    #[cfg(freedesktop)]
    follows_light_system:
        "Follows the color scheme of the desktop, which is light now.",
        "Следует цветовой схеме рабочего стола; сейчас она светлая.";
    #[cfg(freedesktop)]
    follows_unknown_system:
        "Follows the color scheme of the desktop, which could not be read.",
        "Следует цветовой схеме рабочего стола, но определить её не удалось.";
    application_folder: "Application folder", "Папка приложения";
    application_folder_note:
        "The installed executable and UI persistence are stored in this folder.",
        "В этой папке хранятся установленный исполняемый файл и состояние интерфейса.";
    update_installed: "Update installed app", "Обновить установленное приложение";
    install_here: "Install OptoTracker here", "Установить OptoTracker сюда";
    installed: "OptoTracker was installed.", "OptoTracker установлен.";
    open_folder: "Open folder", "Открыть папку";
    #[cfg(not(freedesktop))]
    create_shortcut: "Create desktop shortcut", "Создать ярлык на рабочем столе";
    #[cfg(not(freedesktop))]
    shortcut_created: "Desktop shortcut created.", "Ярлык на рабочем столе создан.";
    #[cfg(freedesktop)]
    create_shortcut: "Add to the applications menu", "Добавить в меню приложений";
    #[cfg(freedesktop)]
    shortcut_created:
        "OptoTracker was added to the applications menu.",
        "OptoTracker добавлен в меню приложений.";
    installed_executable: "Installed executable:", "Установленный исполняемый файл:";
    not_installed:
        "OptoTracker is not installed in this folder yet.",
        "OptoTracker ещё не установлен в эту папку.";
    data: "Data:", "Данные:";
    persistence: "Persistence:", "Состояние интерфейса:";
    weekly_backups: "Weekly backups:", "Еженедельные резервные копии:";
    #[cfg(target_os = "windows")]
    system_behavior: "Windows behavior", "Поведение в Windows";
    #[cfg(target_os = "macos")]
    system_behavior: "macOS behavior", "Поведение в macOS";
    #[cfg(target_os = "linux")]
    system_behavior: "Linux behavior", "Поведение в Linux";
    #[cfg(target_os = "freebsd")]
    system_behavior: "FreeBSD behavior", "Поведение во FreeBSD";
    autorun: "Start OptoTracker when I sign in", "Запускать OptoTracker при входе в систему";
    #[cfg(target_os = "windows")]
    autorun_enabled: "Windows autorun enabled.", "Автозапуск Windows включён.";
    #[cfg(target_os = "windows")]
    autorun_disabled: "Windows autorun disabled", "Автозапуск Windows отключён";
    #[cfg(not(target_os = "windows"))]
    autorun_enabled: "Autorun enabled.", "Автозапуск включён.";
    #[cfg(not(target_os = "windows"))]
    autorun_disabled: "Autorun disabled", "Автозапуск отключён";
    #[cfg(not(target_os = "macos"))]
    autorun_minimized: "Start minimized to tray on autorun", "При автозапуске сворачивать в трей";
    #[cfg(target_os = "macos")]
    autorun_minimized:
        "Start in the menu bar on autorun, without a window",
        "При автозапуске оставаться в строке меню, без окна";
    autorun_updated: "Autorun behavior updated.", "Параметры автозапуска обновлены.";
    #[cfg(not(target_os = "macos"))]
    to_tray:
        "Send OptoTracker to the tray when closed or minimized",
        "Убирать OptoTracker в трей при закрытии и сворачивании";
    #[cfg(not(target_os = "macos"))]
    to_tray_note:
        "When disabled, minimizing behaves normally and closing exits the app.",
        "Если отключено, сворачивание работает как обычно, а закрытие завершает приложение.";
    // A window of macOS is minimized into the Dock, as its users expect.
    #[cfg(target_os = "macos")]
    to_tray:
        "Keep OptoTracker in the menu bar when its window is closed",
        "Оставлять OptoTracker в строке меню при закрытии окна";
    #[cfg(target_os = "macos")]
    to_tray_note:
        "When disabled, closing the window exits the app.",
        "Если отключено, закрытие окна завершает приложение.";
    #[cfg(freedesktop)]
    to_tray_unavailable:
        "Not available here: this desktop has no system tray, or does not let a window hide.",
        "Здесь недоступно: на этом рабочем столе нет системного трея или окно нельзя скрыть.";

    // What the system shows outside the window.
    exit: "Exit OptoTracker", "Выйти из OptoTracker";
    open_app: "Open OptoTracker", "Открыть OptoTracker";
    #[cfg(not(freedesktop))]
    tray_timer_running: "OptoTracker — timer running", "OptoTracker — таймер запущен";
    #[cfg(not(target_os = "macos"))]
    timer_running: "Timer running", "Таймер запущен";
    #[cfg(target_os = "windows")]
    shortcut_description: "Open OptoTracker", "Открыть OptoTracker";
    #[cfg(not(target_os = "macos"))]
    already_running:
        "OptoTracker is already running. Open it from the system tray.",
        "OptoTracker уже запущен. Откройте его из системного трея.";
    #[cfg(target_os = "macos")]
    already_running:
        "OptoTracker is already running. Open it from the menu bar.",
        "OptoTracker уже запущен. Откройте его из строки меню.";
    could_not_start: "OptoTracker could not start", "Не удалось запустить OptoTracker";
    #[cfg(target_os = "windows")]
    usage:
        "Usage: OptoTracker.exe [--minimized] [DATA_FILE]",
        "Использование: OptoTracker.exe [--minimized] [ФАЙЛ_ДАННЫХ]";
    #[cfg(target_os = "macos")]
    usage:
        "Usage: OptoTracker [--minimized] [DATA_FILE]\n       OptoTracker --bundle FOLDER",
        "Использование: OptoTracker [--minimized] [ФАЙЛ_ДАННЫХ]\n               OptoTracker --bundle ПАПКА";
    #[cfg(freedesktop)]
    usage:
        "Usage: opto-tracker [--minimized] [DATA_FILE]",
        "Использование: opto-tracker [--minimized] [ФАЙЛ_ДАННЫХ]";

    // What can go wrong on the desktop. The reason follows after a colon.
    executable_not_found:
        "Could not locate the running executable",
        "Не удалось найти запущенный исполняемый файл";
    folder_not_created:
        "Could not create the application folder",
        "Не удалось создать папку приложения";
    install_failed: "Could not install OptoTracker", "Не удалось установить OptoTracker";
    could_not_create: "Could not create", "Не удалось создать";
    could_not_open: "Could not open", "Не удалось открыть";
    #[cfg(target_os = "windows")]
    autorun_not_updated:
        "Could not update Windows autorun",
        "Не удалось изменить автозапуск Windows";
    #[cfg(not(target_os = "windows"))]
    autorun_not_updated: "Could not update autorun", "Не удалось изменить автозапуск";
    #[cfg(target_os = "windows")]
    autorun_rejected:
        "Windows rejected the autorun change",
        "Windows отклонила изменение автозапуска";
    #[cfg(not(freedesktop))]
    desktop_not_found:
        "Could not locate the Desktop folder",
        "Не удалось найти папку рабочего стола";
    #[cfg(target_os = "windows")]
    desktop_path_unreadable:
        "Could not read the Desktop folder path",
        "Не удалось прочитать путь к папке рабочего стола";
    #[cfg(not(freedesktop))]
    shortcut_not_created:
        "Could not create a desktop shortcut",
        "Не удалось создать ярлык на рабочем столе";
    #[cfg(freedesktop)]
    shortcut_not_created:
        "Could not add OptoTracker to the applications menu",
        "Не удалось добавить OptoTracker в меню приложений";
    #[cfg(target_os = "windows")]
    shortcut_target_not_set:
        "Could not set the shortcut target",
        "Не удалось задать объект ярлыка";
    #[cfg(target_os = "windows")]
    shortcut_folder_not_set:
        "Could not set the shortcut folder",
        "Не удалось задать рабочую папку ярлыка";
    #[cfg(target_os = "windows")]
    shortcut_description_not_set:
        "Could not set the shortcut description",
        "Не удалось задать описание ярлыка";
    #[cfg(target_os = "windows")]
    shortcut_icon_not_set:
        "Could not set the shortcut icon",
        "Не удалось задать значок ярлыка";
    #[cfg(target_os = "windows")]
    shortcut_not_saved:
        "Could not save the desktop shortcut",
        "Не удалось сохранить ярлык на рабочем столе";
}

/// The time that a period is compared with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Before {
    Week,
    Month,
    Year,
    Days,
}

impl Texts {
    /// A number with the form of a word that goes with it.
    fn counted(&self, count: u64, english: [&str; 2], russian: [&str; 3]) -> String {
        let word = match self.language {
            Language::English => english[usize::from(count != 1)],
            Language::Russian => russian_form(count, russian),
        };
        format!("{count} {word}")
    }

    pub fn project_count(&self, count: usize) -> String {
        let russian = ["проект", "проекта", "проектов"];
        self.counted(count as u64, ["project", "projects"], russian)
    }

    pub fn activity_count(&self, count: usize) -> String {
        let russian = ["занятие", "занятия", "занятий"];
        self.counted(count as u64, ["activity", "activities"], russian)
    }

    pub fn day_count(&self, count: u64) -> String {
        self.counted(count, ["day", "days"], ["день", "дня", "дней"])
    }

    pub fn hour_count(&self, count: u64) -> String {
        self.counted(count, ["hour", "hours"], ["час", "часа", "часов"])
    }

    pub fn minute_count(&self, count: u64) -> String {
        self.counted(count, ["minute", "minutes"], ["минута", "минуты", "минут"])
    }

    pub fn second_count(&self, count: u64) -> String {
        let russian = ["секунда", "секунды", "секунд"];
        self.counted(count, ["second", "seconds"], russian)
    }

    /// The days that some of them are counted out of.
    pub fn of_days(&self, count: u64) -> String {
        match self.language {
            Language::English => format!("of {}", self.day_count(count)),
            Language::Russian => {
                let word = russian_form(count, ["дня", "дней", "дней"]);
                format!("из {count} {word}")
            }
        }
    }

    /// The sessions that one of them is picked out of.
    pub fn of_sessions(&self, count: usize) -> String {
        let count = count as u64;
        match self.language {
            Language::English => {
                let word = if count == 1 { "session" } else { "sessions" };
                format!("of {count} {word}")
            }
            Language::Russian => {
                let word = russian_form(count, ["сессии", "сессий", "сессий"]);
                format!("из {count} {word}")
            }
        }
    }

    /// Which of the entries a page shows.
    pub fn entries_shown(&self, first: usize, last: usize, total: usize) -> String {
        match self.language {
            Language::English => format!("{first}–{last} of {total}"),
            Language::Russian => format!("{first}–{last} из {total}"),
        }
    }

    /// Stands for the rest of a list that is cut short.
    pub fn more(&self, count: usize) -> String {
        match self.language {
            Language::English => format!("+{count} more"),
            Language::Russian => format!("и ещё {count}"),
        }
    }

    pub fn show_all(&self, count: usize) -> String {
        match self.language {
            Language::English => format!("Show all {count}"),
            Language::Russian => format!("Показать все {count}"),
        }
    }

    /// The names of the months, from January.
    fn months(&self) -> [&'static str; 12] {
        match self.language {
            Language::English => [
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ],
            Language::Russian => [
                "Январь",
                "Февраль",
                "Март",
                "Апрель",
                "Май",
                "Июнь",
                "Июль",
                "Август",
                "Сентябрь",
                "Октябрь",
                "Ноябрь",
                "Декабрь",
            ],
        }
    }

    /// The months in short where they stand alone, from January.
    pub fn short_months(&self) -> [&'static str; 12] {
        match self.language {
            Language::English => [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ],
            Language::Russian => [
                "Янв", "Фев", "Мар", "Апр", "Май", "Июн", "Июл", "Авг", "Сен", "Окт", "Ноя", "Дек",
            ],
        }
    }

    /// The months in short after the number of a day. Russian writes them
    /// without a capital there and puts May into another case.
    fn months_in_dates(&self) -> [&'static str; 12] {
        match self.language {
            Language::English => self.short_months(),
            Language::Russian => [
                "янв", "фев", "мар", "апр", "мая", "июн", "июл", "авг", "сен", "окт", "ноя", "дек",
            ],
        }
    }

    /// The names of the weekdays, from Monday.
    pub fn weekdays(&self) -> [&'static str; 7] {
        match self.language {
            Language::English => [
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
                "Sunday",
            ],
            Language::Russian => [
                "Понедельник",
                "Вторник",
                "Среда",
                "Четверг",
                "Пятница",
                "Суббота",
                "Воскресенье",
            ],
        }
    }

    /// The weekdays in short, from Monday.
    pub fn short_weekdays(&self) -> [&'static str; 7] {
        match self.language {
            Language::English => ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
            Language::Russian => ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"],
        }
    }

    /// The month of a date, in short and standing alone: "Oct".
    pub fn short_month(&self, date: NaiveDate) -> &'static str {
        self.short_months()[date.month0() as usize]
    }

    /// The weekday of a date in short: "Mon".
    pub fn short_weekday(&self, date: NaiveDate) -> &'static str {
        self.short_weekdays()[weekday_index(date)]
    }

    /// A date without its year: "5 Oct".
    pub fn day_and_month(&self, date: NaiveDate) -> String {
        let month = self.months_in_dates()[date.month0() as usize];
        format!("{} {month}", date.day())
    }

    /// A date in day-month-year order: "5 Oct 2026".
    pub fn date(&self, date: NaiveDate) -> String {
        format!("{} {}", self.day_and_month(date), date.year())
    }

    /// A date after its weekday: "Monday, 5 Oct 2026".
    pub fn date_with_weekday(&self, date: NaiveDate) -> String {
        let weekday = self.weekdays()[weekday_index(date)];
        format!("{weekday}, {}", self.date(date))
    }

    /// A date after its weekday in short: "Mon, 5 Oct 2026".
    pub fn date_with_short_weekday(&self, date: NaiveDate) -> String {
        format!("{}, {}", self.short_weekday(date), self.date(date))
    }

    /// The month of a date as a title: "October 2026".
    pub fn month_and_year(&self, date: NaiveDate) -> String {
        format!("{} {}", self.months()[date.month0() as usize], date.year())
    }

    /// The title of a week that is neither this one nor the last.
    pub fn week_of(&self, monday: NaiveDate) -> String {
        let date = self.date(monday);
        match self.language {
            Language::English => format!("Week of {date}"),
            Language::Russian => format!("Неделя с {date}"),
        }
    }

    /// What the date button of an entry does, with the date it has now.
    pub fn change_date(&self, current: NaiveDate) -> String {
        let date = self.date(current);
        match self.language {
            Language::English => format!("Change date ({date})"),
            Language::Russian => format!("Изменить дату ({date})"),
        }
    }

    pub fn date_picker(&self) -> date_picker::Labels {
        date_picker::Labels {
            months: self.short_months(),
            weekdays: self.short_weekdays(),
            previous_year: self.previous_year,
            previous_month: self.previous_month,
            next_month: self.next_month,
            next_year: self.next_year,
            today: self.today,
            save: self.save,
            cancel: self.cancel,
        }
    }

    pub fn calendar(&self) -> github_activity::Labels {
        github_activity::Labels {
            months: self.short_months(),
            weekdays: self.short_weekdays(),
        }
    }

    /// Says that there is time now and was none in the time before.
    pub fn nothing_before(&self, before: Before) -> String {
        match self.language {
            Language::English => format!("nothing {}", english_before(before)),
            Language::Russian => format!("ничего за {}", russian_before(before).1),
        }
    }

    /// How much more or less time there is than in the time before, in
    /// whole percent.
    pub fn change_since(&self, percent: i64, before: Before) -> String {
        let sign = if percent > 0 { '+' } else { '−' };
        let amount = percent.unsigned_abs();
        match self.language {
            Language::English => {
                let before = english_before(before);
                if percent == 0 {
                    format!("same as {before}")
                } else {
                    format!("{sign}{amount}% vs {before}")
                }
            }
            Language::Russian => {
                let (compared_to, during) = russian_before(before);
                if percent == 0 {
                    format!("как за {during}")
                } else {
                    format!("{sign}{amount}% к {compared_to}")
                }
            }
        }
    }

    /// A duration and the days it was tracked in.
    pub fn time_in_days(&self, time: &str, days: &str) -> String {
        match self.language {
            Language::English => format!("{time} in {days}"),
            Language::Russian => format!("{time} за {days}"),
        }
    }

    /// A duration per day when the days without work count too.
    pub fn over_all_days(&self, time: &str) -> String {
        match self.language {
            Language::English => format!("{time} over all days"),
            Language::Russian => format!("{time} по всем дням"),
        }
    }

    pub fn on_average(&self, time: &str) -> String {
        match self.language {
            Language::English => format!("{time} on average"),
            Language::Russian => format!("{time} в среднем"),
        }
    }

    /// On how many of the days that a weekday came up there was work.
    pub fn worked_on(&self, worked: u32, days: u32) -> String {
        match self.language {
            Language::English => format!("worked on {worked} of {days}"),
            Language::Russian => format!("отработано {worked} из {days}"),
        }
    }

    /// A share of the whole time, such as "12%".
    pub fn of_the_time(&self, share: &str) -> String {
        match self.language {
            Language::English => format!("{share} of the time"),
            Language::Russian => format!("{share} времени"),
        }
    }

    pub fn model_error(&self, error: &ModelError) -> String {
        if self.language == Language::English {
            return error.to_string();
        }
        match *error {
            ModelError::UnsupportedFormat(version) => {
                format!("неподдерживаемая версия формата данных: {version}")
            }
            ModelError::InvalidEntryId(id) => {
                format!("идентификатор записи {id} повторяется или зарезервирован")
            }
            ModelError::EmptyName => "названия проектов и занятий не могут быть пустыми".to_owned(),
            ModelError::MissingActivity(index) => format!("занятие {index} не существует"),
            ModelError::MissingProject(index) => format!("проект {index} не существует"),
            ModelError::NegativeDuration => {
                "запись заканчивается раньше, чем начинается".to_owned()
            }
            ModelError::EntryTooLong => "запись не может быть длиннее 24 часов".to_owned(),
            ModelError::TimerAlreadyRunning => "таймер уже запущен".to_owned(),
            ModelError::NoActiveTimer => "таймер не запущен".to_owned(),
            ModelError::ClockMovedBackwards => {
                "системные часы показывают время раньше начала таймера".to_owned()
            }
            ModelError::TimerStartInFuture => "начало таймера не может быть в будущем".to_owned(),
            ModelError::TimerStartTooOld => {
                "начало таймера должно быть меньше 24 часов назад".to_owned()
            }
            ModelError::EntryIdExhausted => "идентификаторы записей закончились".to_owned(),
            ModelError::TimestampOutOfRange => {
                "метка времени вне поддерживаемого диапазона".to_owned()
            }
        }
    }

    pub fn time_error(&self, error: TimeError) -> String {
        if self.language == Language::English {
            return error.to_string();
        }
        match error {
            TimeError::NoSuchLocalTime => "Такого местного времени на этом компьютере нет",
            TimeError::TimeOutOfRange => "Время вне допустимого диапазона",
            TimeError::DateOutOfRange => "Дата вне допустимого диапазона",
            TimeError::SameStartAndStop => "Время начала и окончания не могут совпадать",
            TimeError::EntryLength => "Запись должна длиться от 1 секунды до 24 часов",
            TimeError::Format => "Время нужно вводить как ЧЧ:ММ или ЧЧММ",
        }
        .to_owned()
    }

    pub fn load_error(&self, error: &LoadError) -> String {
        if self.language == Language::English {
            return error.to_string();
        }
        match error {
            LoadError::Read { path, source } => {
                format!("не удалось прочитать {}: {source}", path.display())
            }
            LoadError::Decode { path, source } => {
                format!("не удалось разобрать {}: {source}", path.display())
            }
            LoadError::UnsupportedVersion { path, version } => format!(
                "в {} неподдерживаемая версия формата данных: {version}",
                path.display()
            ),
            LoadError::Invalid { path, reason } => format!(
                "некорректные данные в {}: {}",
                path.display(),
                self.model_error(reason)
            ),
        }
    }

    pub fn save_error(&self, error: &SaveError) -> String {
        if self.language == Language::English {
            return error.to_string();
        }
        match error {
            SaveError::Invalid(reason) => format!(
                "некорректные данные не сохранены: {}",
                self.model_error(reason)
            ),
            SaveError::Encode(source) => format!("не удалось закодировать данные: {source}"),
            SaveError::Write { path, source } => {
                format!("не удалось сохранить {}: {source}", path.display())
            }
        }
    }

    pub fn backup_error(&self, error: &BackupError) -> String {
        if self.language == Language::English {
            return error.to_string();
        }
        match error {
            BackupError::CreateFolder(source) => {
                format!("не удалось создать папку резервных копий: {source}")
            }
            BackupError::CreateFile(source) => {
                format!("не удалось создать еженедельную резервную копию: {source}")
            }
            BackupError::ReadData(source) => {
                format!("не удалось прочитать данные для еженедельной резервной копии: {source}")
            }
            BackupError::Finish(source) => {
                format!("не удалось завершить еженедельную резервную копию: {source}")
            }
        }
    }
}

/// The form of a Russian word that goes with a number: the one for 1, the
/// one for 2 to 4 and the one for everything else, as in "1 день", "2 дня"
/// and "5 дней".
fn russian_form(count: u64, forms: [&str; 3]) -> &str {
    match (count % 10, count % 100) {
        (_, 11..=14) => forms[2],
        (1, _) => forms[0],
        (2..=4, _) => forms[1],
        _ => forms[2],
    }
}

fn english_before(before: Before) -> &'static str {
    match before {
        Before::Week => "the week before",
        Before::Month => "the month before",
        Before::Year => "the year before",
        Before::Days => "the days before",
    }
}

/// The time before in the two cases it is needed in: the one to compare
/// with ("к предыдущей неделе") and the one that something happened in ("за
/// предыдущую неделю").
fn russian_before(before: Before) -> (&'static str, &'static str) {
    match before {
        Before::Week => ("предыдущей неделе", "предыдущую неделю"),
        Before::Month => ("предыдущему месяцу", "предыдущий месяц"),
        Before::Year => ("предыдущему году", "предыдущий год"),
        Before::Days => ("предыдущим дням", "предыдущие дни"),
    }
}

/// The place of a date's weekday in a week that starts on Monday.
fn weekday_index(date: NaiveDate) -> usize {
    date.weekday().num_days_from_monday() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn dates_use_day_month_year_order() {
        // A Tuesday.
        let day = date(2026, 9, 15);
        let english = Language::English.texts();
        assert_eq!(english.date(day), "15 Sep 2026");
        assert_eq!(english.date_with_weekday(day), "Tuesday, 15 Sep 2026");
        assert_eq!(english.date_with_short_weekday(day), "Tue, 15 Sep 2026");
        assert_eq!(english.month_and_year(day), "September 2026");

        let russian = Language::Russian.texts();
        assert_eq!(russian.date(day), "15 сен 2026");
        assert_eq!(russian.date_with_weekday(day), "Вторник, 15 сен 2026");
        assert_eq!(russian.date_with_short_weekday(day), "Вт, 15 сен 2026");
        assert_eq!(russian.month_and_year(day), "Сентябрь 2026");
    }

    #[test]
    fn russian_may_differs_alone_and_in_a_date() {
        let russian = Language::Russian.texts();
        let day = date(2026, 5, 9);
        assert_eq!(russian.short_month(day), "Май");
        assert_eq!(russian.date(day), "9 мая 2026");
    }

    #[test]
    fn words_agree_with_their_numbers() {
        let english = Language::English.texts();
        assert_eq!(english.day_count(1), "1 day");
        assert_eq!(english.day_count(0), "0 days");
        assert_eq!(english.of_days(1), "of 1 day");
        assert_eq!(english.of_sessions(12), "of 12 sessions");

        let russian = Language::Russian.texts();
        let days = |count| russian.day_count(count);
        assert_eq!(days(1), "1 день");
        assert_eq!(days(3), "3 дня");
        assert_eq!(days(5), "5 дней");
        assert_eq!(days(11), "11 дней");
        assert_eq!(days(12), "12 дней");
        assert_eq!(days(21), "21 день");
        assert_eq!(days(104), "104 дня");
        assert_eq!(days(111), "111 дней");
        assert_eq!(russian.of_days(1), "из 1 дня");
        assert_eq!(russian.of_days(21), "из 21 дня");
        assert_eq!(russian.of_days(30), "из 30 дней");
        assert_eq!(russian.of_sessions(41), "из 41 сессии");
        assert_eq!(russian.of_sessions(4), "из 4 сессий");
    }

    #[test]
    fn a_change_is_worded_by_its_direction() {
        let english = Language::English.texts();
        assert_eq!(
            english.change_since(12, Before::Week),
            "+12% vs the week before"
        );
        assert_eq!(
            english.change_since(-5, Before::Month),
            "−5% vs the month before"
        );
        assert_eq!(
            english.change_since(0, Before::Year),
            "same as the year before"
        );
        assert_eq!(
            english.nothing_before(Before::Days),
            "nothing the days before"
        );

        let russian = Language::Russian.texts();
        assert_eq!(
            russian.change_since(12, Before::Week),
            "+12% к предыдущей неделе"
        );
        assert_eq!(
            russian.change_since(-5, Before::Month),
            "−5% к предыдущему месяцу"
        );
        assert_eq!(
            russian.change_since(0, Before::Year),
            "как за предыдущий год"
        );
        assert_eq!(
            russian.nothing_before(Before::Days),
            "ничего за предыдущие дни"
        );
    }

    #[test]
    fn errors_are_worded_in_the_language() {
        let error = ModelError::TimerAlreadyRunning;
        assert_eq!(
            Language::English.texts().model_error(&error),
            "a timer is already running"
        );
        assert_eq!(
            Language::Russian.texts().model_error(&error),
            "таймер уже запущен"
        );
        let invalid = SaveError::Invalid(ModelError::EmptyName);
        assert_eq!(
            Language::Russian.texts().save_error(&invalid),
            "некорректные данные не сохранены: названия проектов и занятий не могут быть пустыми"
        );
    }

    #[test]
    fn each_language_has_its_own_texts() {
        for language in Language::ALL {
            assert_eq!(language.texts().language, language);
        }
    }
}
