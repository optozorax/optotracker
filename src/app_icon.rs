use eframe::egui::{self, Color32, CornerRadius, IconData, Pos2, Rect, Shape, Stroke};

const TILE: Color32 = Color32::from_rgb(8, 169, 225);
const RUNNING: Color32 = Color32::from_rgb(244, 67, 54);

pub fn paint(painter: &egui::Painter, rect: Rect, running: bool) {
    let radius = rect.width() * 0.24;
    painter.rect_filled(rect, CornerRadius::from(radius), TILE);

    let point = |x: f32, y: f32| {
        Pos2::new(
            rect.left() + rect.width() * x,
            rect.top() + rect.height() * y,
        )
    };
    let stroke = Stroke::new((rect.width() * 0.065).max(1.5), Color32::WHITE);
    painter.line_segment([point(0.31, 0.27), point(0.69, 0.27)], stroke);
    painter.line_segment([point(0.31, 0.73), point(0.69, 0.73)], stroke);
    painter.line_segment([point(0.34, 0.30), point(0.66, 0.70)], stroke);
    painter.line_segment([point(0.66, 0.30), point(0.34, 0.70)], stroke);
    painter.add(Shape::convex_polygon(
        vec![
            point(0.39, 0.62),
            point(0.61, 0.62),
            point(0.67, 0.70),
            point(0.33, 0.70),
        ],
        Color32::WHITE,
        Stroke::NONE,
    ));

    if running {
        let center = point(0.84, 0.16);
        painter.circle_filled(center, rect.width() * 0.22, Color32::WHITE);
        painter.circle_filled(center, rect.width() * 0.17, RUNNING);
    }
}

/// The sides of the squares that the two icons are designed in.
const ICON_DESIGN: f32 = 64.0;
const TRAY_DESIGN: f32 = 32.0;

pub fn icon_data(running: bool) -> IconData {
    render(64, ICON_DESIGN, |x, y| sample_icon(x, y, running))
}

/// A simplified, edge-to-edge variant that stays legible after it is scaled
/// down to the 16–24 pixels of a notification area or a menu bar.
pub fn tray_icon_data(running: bool) -> IconData {
    // The menu bar of macOS shows it 18 points high, which is 36 pixels of
    // most of its screens.
    let size = if cfg!(target_os = "macos") { 64 } else { 32 };
    render(size, TRAY_DESIGN, |x, y| sample_tray_icon(x, y, running))
}

/// The icon as a picture of any size, for the files that a desktop shows it
/// from.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn application_icon(size: u32) -> IconData {
    // The icons of macOS leave about a tenth of their square empty on every
    // side, so the tile is drawn smaller there to match its neighbours.
    let zoom = if cfg!(target_os = "macos") { 0.92 } else { 1.0 };
    let center = ICON_DESIGN / 2.0;
    let zoomed = |coordinate: f32| (coordinate - center) / zoom + center;
    render(size as usize, ICON_DESIGN, |x, y| {
        sample_icon(zoomed(x), zoomed(y), false)
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn png(icon: &IconData) -> Result<Vec<u8>, String> {
    let mut file = Vec::new();
    let mut encoder = png::Encoder::new(&mut file, icon.width, icon.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&icon.rgba))
        .map_err(|error| error.to_string())?;
    Ok(file)
}

/// Draws a square icon of `size` pixels. `sample` colors the points of the
/// square of `design` units that the icon was designed in.
fn render(size: usize, design: f32, sample: impl Fn(f32, f32) -> [u8; 4]) -> IconData {
    const SAMPLES: usize = 4;
    let scale = design / size as f32;
    let mut rgba = vec![0; size * size * 4];

    for y in 0..size {
        for x in 0..size {
            let mut sum = [0_u32; 4];
            for sample_y in 0..SAMPLES {
                for sample_x in 0..SAMPLES {
                    let px = (x as f32 + (sample_x as f32 + 0.5) / SAMPLES as f32) * scale;
                    let py = (y as f32 + (sample_y as f32 + 0.5) / SAMPLES as f32) * scale;
                    let color = sample(px, py);
                    for channel in 0..4 {
                        sum[channel] += color[channel] as u32;
                    }
                }
            }
            let offset = (y * size + x) * 4;
            let divisor = (SAMPLES * SAMPLES) as u32;
            for channel in 0..4 {
                rgba[offset + channel] = (sum[channel] / divisor) as u8;
            }
        }
    }

    IconData {
        rgba,
        width: size as u32,
        height: size as u32,
    }
}

fn sample_tray_icon(x: f32, y: f32, running: bool) -> [u8; 4] {
    let mut color = if inside_rounded_rect(x, y, 0.5, 0.5, 31.5, 31.5, 6.5) {
        [TILE.r(), TILE.g(), TILE.b(), 255]
    } else {
        [0, 0, 0, 0]
    };

    let white = [255, 255, 255, 255];
    for (from, to) in [
        ((8.5, 7.0), (23.5, 7.0)),
        ((8.5, 25.0), (23.5, 25.0)),
        ((10.0, 8.5), (22.0, 23.5)),
        ((22.0, 8.5), (10.0, 23.5)),
    ] {
        if distance_to_segment((x, y), from, to) <= 1.55 {
            color = white;
        }
    }
    if (20.5..=25.0).contains(&y) {
        let half_width = 1.5 + (y - 20.5) * 1.15;
        if (x - 16.0).abs() <= half_width {
            color = white;
        }
    }

    if running {
        let distance = ((x - 25.5).powi(2) + (y - 6.5).powi(2)).sqrt();
        if distance <= 6.9 {
            color = white;
        }
        if distance <= 5.4 {
            color = [RUNNING.r(), RUNNING.g(), RUNNING.b(), 255];
        }
    }
    color
}

fn sample_icon(x: f32, y: f32, running: bool) -> [u8; 4] {
    let mut color = if inside_rounded_rect(x, y, 4.0, 4.0, 60.0, 60.0, 13.0) {
        [TILE.r(), TILE.g(), TILE.b(), 255]
    } else {
        [0, 0, 0, 0]
    };

    let white = [255, 255, 255, 255];
    for (from, to) in [
        ((20.0, 18.0), (44.0, 18.0)),
        ((20.0, 46.0), (44.0, 46.0)),
        ((22.0, 20.0), (42.0, 44.0)),
        ((42.0, 20.0), (22.0, 44.0)),
    ] {
        if distance_to_segment((x, y), from, to) <= 2.2 {
            color = white;
        }
    }
    if (38.0..=44.0).contains(&y) {
        let half_width = 2.0 + (y - 38.0) * 1.5;
        if (x - 32.0).abs() <= half_width {
            color = white;
        }
    }

    if running {
        let distance = ((x - 52.0).powi(2) + (y - 12.0).powi(2)).sqrt();
        if distance <= 14.0 {
            color = white;
        }
        if distance <= 10.8 {
            color = [RUNNING.r(), RUNNING.g(), RUNNING.b(), 255];
        }
    }
    color
}

fn inside_rounded_rect(
    x: f32,
    y: f32,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    radius: f32,
) -> bool {
    let nearest_x = x.clamp(left + radius, right - radius);
    let nearest_y = y.clamp(top + radius, bottom - radius);
    (x - nearest_x).powi(2) + (y - nearest_y).powi(2) <= radius.powi(2)
}

fn distance_to_segment(point: (f32, f32), from: (f32, f32), to: (f32, f32)) -> f32 {
    let segment = (to.0 - from.0, to.1 - from.1);
    let length_squared = segment.0 * segment.0 + segment.1 * segment.1;
    let projection = (((point.0 - from.0) * segment.0 + (point.1 - from.1) * segment.1)
        / length_squared)
        .clamp(0.0, 1.0);
    let nearest = (
        from.0 + projection * segment.0,
        from.1 + projection * segment.1,
    );
    ((point.0 - nearest.0).powi(2) + (point.1 - nearest.1).powi(2)).sqrt()
}

#[cfg(target_os = "windows")]
pub fn set_taskbar_running(frame: &eframe::Frame, running: bool) {
    use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{ITaskbarList3, TaskbarList};
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};

    let Ok(window_handle) = frame.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = window_handle.as_raw() else {
        return;
    };
    let hwnd = HWND(handle.hwnd.get() as *mut core::ffi::c_void);

    // SAFETY: the HWND is owned by eframe for the duration of this call. COM and
    // icon handles are used according to their Windows ownership contracts.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let Ok(taskbar) =
            CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER)
        else {
            return;
        };
        if taskbar.HrInit().is_err() {
            return;
        }

        if running {
            if let Ok(icon) = create_running_overlay_icon() {
                let description = HSTRING::from(crate::i18n::tr().timer_running);
                let _ = taskbar.SetOverlayIcon(hwnd, icon, &description);
                let _ = DestroyIcon(icon);
            }
        } else {
            let _ = taskbar.SetOverlayIcon(hwnd, HICON::default(), PCWSTR::null());
        }
    }
}

/// The Dock has no icon to lay over another, so the running timer is a badge
/// on the icon of the application there.
#[cfg(target_os = "macos")]
pub fn set_taskbar_running(_frame: &eframe::Frame, running: bool) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;

    // The interface is drawn on the main thread, the only one that may talk
    // to the application.
    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    let badge = running.then(|| NSString::from_str("●"));
    NSApplication::sharedApplication(main_thread)
        .dockTile()
        .setBadgeLabel(badge.as_deref());
}

/// Elsewhere the icon of the window is all that a taskbar shows.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn set_taskbar_running(_frame: &eframe::Frame, _running: bool) {}

#[cfg(target_os = "windows")]
unsafe fn create_running_overlay_icon(
) -> windows::core::Result<windows::Win32::UI::WindowsAndMessaging::HICON> {
    use windows::Win32::UI::WindowsAndMessaging::CreateIcon;

    const SIZE: usize = 32;
    let mut and_mask = [0xff_u8; SIZE * SIZE / 8];
    let mut xor = vec![0_u8; SIZE * SIZE * 4];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let distance =
                (((x as f32 + 0.5) - 16.0).powi(2) + ((y as f32 + 0.5) - 16.0).powi(2)).sqrt();
            if distance <= 14.5 {
                let mask_byte = y * (SIZE / 8) + x / 8;
                and_mask[mask_byte] &= !(0x80 >> (x % 8));
                let offset = (y * SIZE + x) * 4;
                let (red, green, blue) = if distance <= 10.8 {
                    (RUNNING.r(), RUNNING.g(), RUNNING.b())
                } else {
                    (255, 255, 255)
                };
                xor[offset] = blue;
                xor[offset + 1] = green;
                xor[offset + 2] = red;
                xor[offset + 3] = 255;
            }
        }
    }

    // SAFETY: both buffers have the exact dimensions required for a 32x32 icon.
    unsafe {
        CreateIcon(
            None,
            SIZE as i32,
            SIZE as i32,
            1,
            32,
            and_mask.as_ptr(),
            xor.as_ptr(),
        )
    }
}
