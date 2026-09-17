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

pub fn icon_data(running: bool) -> IconData {
    const SIZE: usize = 64;
    const SAMPLES: usize = 4;
    let mut rgba = vec![0; SIZE * SIZE * 4];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut sum = [0_u32; 4];
            for sample_y in 0..SAMPLES {
                for sample_x in 0..SAMPLES {
                    let px = x as f32 + (sample_x as f32 + 0.5) / SAMPLES as f32;
                    let py = y as f32 + (sample_y as f32 + 0.5) / SAMPLES as f32;
                    let color = sample_icon(px, py, running);
                    for channel in 0..4 {
                        sum[channel] += color[channel] as u32;
                    }
                }
            }
            let offset = (y * SIZE + x) * 4;
            let divisor = (SAMPLES * SAMPLES) as u32;
            for channel in 0..4 {
                rgba[offset + channel] = (sum[channel] / divisor) as u8;
            }
        }
    }

    IconData {
        rgba,
        width: SIZE as u32,
        height: SIZE as u32,
    }
}

/// A simplified, edge-to-edge variant that stays legible after Windows scales
/// it down to the notification area's 16–20 physical pixels.
pub fn tray_icon_data(running: bool) -> IconData {
    const SIZE: usize = 32;
    const SAMPLES: usize = 4;
    let mut rgba = vec![0; SIZE * SIZE * 4];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut sum = [0_u32; 4];
            for sample_y in 0..SAMPLES {
                for sample_x in 0..SAMPLES {
                    let px = x as f32 + (sample_x as f32 + 0.5) / SAMPLES as f32;
                    let py = y as f32 + (sample_y as f32 + 0.5) / SAMPLES as f32;
                    let color = sample_tray_icon(px, py, running);
                    for channel in 0..4 {
                        sum[channel] += color[channel] as u32;
                    }
                }
            }
            let offset = (y * SIZE + x) * 4;
            let divisor = (SAMPLES * SAMPLES) as u32;
            for channel in 0..4 {
                rgba[offset + channel] = (sum[channel] / divisor) as u8;
            }
        }
    }

    IconData {
        rgba,
        width: SIZE as u32,
        height: SIZE as u32,
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
    use windows::core::{w, PCWSTR};
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
                let _ = taskbar.SetOverlayIcon(hwnd, icon, w!("Timer running"));
                let _ = DestroyIcon(icon);
            }
        } else {
            let _ = taskbar.SetOverlayIcon(hwnd, HICON::default(), PCWSTR::null());
        }
    }
}

#[cfg(not(target_os = "windows"))]
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
