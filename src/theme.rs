//! The colors of the interface, once for the light theme and once for the
//! dark one. egui decides which theme is shown; everything that paints with
//! a color of its own asks for the palette of the visuals it is drawn with.

use eframe::egui::{self, Color32, Stroke, Theme};

pub struct Palette {
    pub text: Color32,
    /// Explanations and status messages.
    pub muted: Color32,
    /// The small print of charts and legends.
    pub caption: Color32,
    /// Text that reports what a setting just did.
    pub notice: Color32,

    /// The page lies behind the cards and differs from them only slightly,
    /// so cards are told apart by their borders and their tinted headers.
    pub page: Color32,
    pub card: Color32,
    pub header: Color32,
    pub border: Color32,
    pub hairline: Color32,
    pub stripe: Color32,
    pub row_hover: Color32,
    /// What a bar that shows a share fills up.
    pub track: Color32,

    /// Buttons and fields have an outline, which gets stronger under the
    /// pointer.
    pub control: Color32,
    pub outline: Color32,
    pub outline_hovered: Color32,
    pub outline_pressed: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub selection: Color32,
    pub selection_text: Color32,

    /// The fills of the timer's button. Both are dark enough for a white
    /// label to stay readable.
    pub start: Color32,
    pub stop: Color32,
    /// Icons are gray until the pointer gives them their own color.
    pub icon: Color32,
    pub icon_disabled: Color32,
    pub restart: Color32,
    pub delete: Color32,
    pub empty_swatch: Color32,
}

const LIGHT: Palette = Palette {
    text: Color32::from_rgb(34, 40, 47),
    muted: Color32::from_gray(105),
    caption: Color32::from_rgb(87, 96, 106),
    notice: Color32::from_rgb(0, 118, 200),

    page: Color32::from_rgb(249, 250, 252),
    card: Color32::WHITE,
    header: Color32::from_rgb(235, 241, 246),
    border: Color32::from_rgb(203, 212, 220),
    hairline: Color32::from_rgb(226, 232, 237),
    stripe: Color32::from_rgb(246, 249, 251),
    row_hover: Color32::from_rgb(244, 248, 251),
    track: Color32::from_gray(235),

    control: Color32::WHITE,
    outline: Color32::from_rgb(196, 205, 213),
    outline_hovered: Color32::from_gray(150),
    outline_pressed: Color32::from_gray(110),
    hover: Color32::from_rgb(238, 243, 247),
    pressed: Color32::from_rgb(224, 231, 237),
    selection: Color32::from_rgb(214, 232, 248),
    selection_text: Color32::from_rgb(0, 84, 150),

    start: Color32::from_rgb(0, 118, 200),
    stop: Color32::from_rgb(211, 47, 47),
    icon: Color32::from_gray(140),
    icon_disabled: Color32::from_gray(210),
    restart: Color32::from_rgb(56, 142, 60),
    delete: Color32::from_rgb(211, 47, 47),
    empty_swatch: Color32::from_gray(175),
};

const DARK: Palette = Palette {
    text: Color32::from_rgb(222, 227, 233),
    muted: Color32::from_rgb(146, 154, 163),
    caption: Color32::from_rgb(139, 148, 158),
    notice: Color32::from_rgb(110, 176, 255),

    page: Color32::from_rgb(16, 19, 23),
    card: Color32::from_rgb(24, 28, 33),
    header: Color32::from_rgb(35, 41, 49),
    border: Color32::from_rgb(52, 59, 68),
    hairline: Color32::from_rgb(38, 44, 52),
    stripe: Color32::from_rgb(29, 34, 40),
    row_hover: Color32::from_rgb(32, 38, 45),
    track: Color32::from_rgb(43, 49, 57),

    control: Color32::from_rgb(36, 42, 50),
    outline: Color32::from_rgb(64, 72, 83),
    outline_hovered: Color32::from_rgb(110, 120, 133),
    outline_pressed: Color32::from_rgb(140, 150, 163),
    hover: Color32::from_rgb(46, 53, 63),
    pressed: Color32::from_rgb(56, 65, 77),
    selection: Color32::from_rgb(30, 64, 102),
    selection_text: Color32::from_rgb(132, 190, 255),

    start: Color32::from_rgb(0, 118, 200),
    stop: Color32::from_rgb(211, 47, 47),
    icon: Color32::from_rgb(118, 126, 137),
    icon_disabled: Color32::from_rgb(58, 64, 73),
    restart: Color32::from_rgb(102, 187, 106),
    delete: Color32::from_rgb(240, 98, 92),
    empty_swatch: Color32::from_rgb(96, 104, 115),
};

impl Palette {
    pub fn of(visuals: &egui::Visuals) -> &'static Self {
        if visuals.dark_mode {
            &DARK
        } else {
            &LIGHT
        }
    }
}

/// The palette of the theme that `ui` is drawn in.
pub fn palette(ui: &egui::Ui) -> &'static Palette {
    Palette::of(ui.visuals())
}

/// A color that the user gave to an activity or a project, as it is drawn on
/// the cards of a theme.
///
/// These colors are chosen in the light theme, where the darkest ones stand
/// out the most. On a dark card they would all but disappear, so there they
/// are brightened: black becomes a dark gray, and the lift fades out towards
/// the colors that are bright enough as they are. The hue is kept, and of
/// two colors the darker one stays the darker.
pub fn data_color(visuals: &egui::Visuals, color: Color32) -> Color32 {
    // Relative luminance: what black is raised to, and where the lift ends.
    const FLOOR: f32 = 0.04;
    const ENOUGH: f32 = 0.16;
    if !visuals.dark_mode {
        return color;
    }
    let luminance = |[red, green, blue]: [f32; 3]| 0.2126 * red + 0.7152 * green + 0.0722 * blue;
    let [red, green, blue, _] = egui::Rgba::from(color).to_array();
    let before = luminance([red, green, blue]);
    if before >= ENOUGH {
        return color;
    }
    let wanted = FLOOR + (ENOUGH - FLOOR) * before / ENOUGH;
    // Scaling keeps the hue. The brightness that a channel at its limit
    // cannot take, which for black is all of it, is added as gray.
    let scale = if before > 0.0 { wanted / before } else { 0.0 };
    let scaled = [red, green, blue].map(|channel| (channel * scale).min(1.0));
    let gray = wanted - luminance(scaled);
    let [red, green, blue] = scaled.map(|channel| (channel + gray).min(1.0));
    egui::Rgba::from_rgb(red, green, blue).into()
}

/// egui's look for a theme, adjusted to its palette.
pub fn visuals(theme: Theme) -> egui::Visuals {
    let mut visuals = theme.default_visuals();
    let palette = Palette::of(&visuals);
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    visuals.selection.bg_fill = palette.selection;
    visuals.selection.stroke.color = palette.selection_text;

    // Buttons and fields are filled like a card and have an outline. egui's
    // own idle look is a gray fill for buttons and no outline at all for
    // text fields and checkboxes, which makes those disappear on the cards.
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.fg_stroke.color = palette.text;
    widgets.inactive.fg_stroke.color = palette.text;
    widgets.inactive.weak_bg_fill = palette.control;
    widgets.inactive.bg_stroke = Stroke::new(1.0, palette.outline);
    widgets.hovered.weak_bg_fill = palette.hover;
    widgets.hovered.bg_stroke = Stroke::new(1.0, palette.outline_hovered);
    widgets.active.weak_bg_fill = palette.pressed;
    widgets.active.bg_stroke = Stroke::new(1.0, palette.outline_pressed);
    widgets.open.weak_bg_fill = palette.hover;
    for state in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        state.corner_radius = egui::CornerRadius::same(4);
    }

    if theme == Theme::Dark {
        // egui's light defaults suit the light palette as they are. Its dark
        // defaults are neutral grays, which look dirty next to the blue-gray
        // of the dark palette.
        widgets.noninteractive.bg_fill = palette.card;
        widgets.noninteractive.weak_bg_fill = palette.card;
        widgets.noninteractive.bg_stroke.color = palette.border;
        // The fills of checkboxes and of the handles of scroll bars.
        widgets.inactive.bg_fill = Color32::from_rgb(50, 57, 67);
        widgets.hovered.bg_fill = Color32::from_rgb(62, 71, 83);
        widgets.active.bg_fill = Color32::from_rgb(76, 86, 100);
        widgets.open.bg_fill = palette.card;
        widgets.open.bg_stroke.color = palette.outline;
        widgets.hovered.fg_stroke.color = Color32::from_rgb(240, 243, 246);
        visuals.panel_fill = palette.page;
        // Menus, tooltips and other popups float above the cards.
        visuals.window_fill = Color32::from_rgb(31, 36, 43);
        visuals.window_stroke.color = palette.outline;
        visuals.extreme_bg_color = Color32::from_rgb(12, 14, 17);
        visuals.text_edit_bg_color = Some(Color32::from_rgb(17, 20, 24));
        visuals.code_bg_color = palette.control;
    }
    visuals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dark(red: u8, green: u8, blue: u8) -> Color32 {
        data_color(&egui::Visuals::dark(), Color32::from_rgb(red, green, blue))
    }

    fn brightness(color: Color32) -> u32 {
        color.to_array().into_iter().take(3).map(u32::from).sum()
    }

    #[test]
    fn data_colors_are_changed_only_where_they_would_disappear() {
        let purple = Color32::from_rgb(60, 23, 102);
        assert_eq!(data_color(&egui::Visuals::light(), purple), purple);
        // Bright enough for a dark card as it is.
        assert_eq!(dark(42, 120, 214), Color32::from_rgb(42, 120, 214));
        assert!(brightness(dark(60, 23, 102)) > brightness(purple));
    }

    #[test]
    fn brightened_data_colors_keep_their_hue_and_their_order() {
        let [red, green, blue, _] = dark(60, 23, 102).to_array();
        assert!(blue > red && red > green);
        // Black has no hue to keep and becomes a gray.
        let [red, green, blue, _] = dark(0, 0, 0).to_array();
        assert!(red == green && green == blue && red > 40);
        let shades = [
            dark(0, 0, 0),
            dark(30, 12, 51),
            dark(60, 23, 102),
            dark(98, 42, 161),
        ];
        assert!(shades.is_sorted_by_key(|&shade| brightness(shade)));
        assert!(brightness(shades[0]) < brightness(shades[3]));
    }
}
