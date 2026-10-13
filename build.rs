use std::env;
use std::fs;
use std::path::PathBuf;

const BLUE: [u8; 3] = [8, 169, 225];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let system = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    // Linux and FreeBSD share their desktops, which follow the freedesktop
    // specifications. What the sources do for both is under `cfg(freedesktop)`.
    println!("cargo:rustc-check-cfg=cfg(freedesktop)");
    if matches!(system.as_str(), "linux" | "freebsd") {
        println!("cargo:rustc-cfg=freedesktop");
    }
    if system != "windows" {
        return;
    }

    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    let icon_path = output.join("opto-tracker.ico");
    fs::write(&icon_path, make_ico()).expect("write generated application icon");

    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(icon_path.to_str().expect("icon path is UTF-8"))
        .set("ProductName", "OptoTracker")
        .set("FileDescription", "OptoTracker personal time tracker");
    resource
        .compile()
        .expect("compile Windows application resources");
}

fn make_ico() -> Vec<u8> {
    let sizes = [16_u32, 32, 48, 256];
    let images: Vec<_> = sizes.into_iter().map(make_dib).collect();
    let directory_size = 6 + images.len() * 16;
    let mut offset = directory_size as u32;
    let mut output = Vec::new();
    push_u16(&mut output, 0);
    push_u16(&mut output, 1);
    push_u16(&mut output, images.len() as u16);

    for (&size, image) in sizes.iter().zip(&images) {
        output.push(if size == 256 { 0 } else { size as u8 });
        output.push(if size == 256 { 0 } else { size as u8 });
        output.extend_from_slice(&[0, 0]);
        push_u16(&mut output, 1);
        push_u16(&mut output, 32);
        push_u32(&mut output, image.len() as u32);
        push_u32(&mut output, offset);
        offset += image.len() as u32;
    }
    for image in images {
        output.extend_from_slice(&image);
    }
    output
}

fn make_dib(size: u32) -> Vec<u8> {
    let mask_stride = size.div_ceil(32) * 4;
    let pixel_bytes = size * size * 4;
    let mask_bytes = mask_stride * size;
    let mut output = Vec::with_capacity((40 + pixel_bytes + mask_bytes) as usize);
    push_u32(&mut output, 40);
    push_i32(&mut output, size as i32);
    push_i32(&mut output, (size * 2) as i32);
    push_u16(&mut output, 1);
    push_u16(&mut output, 32);
    push_u32(&mut output, 0);
    push_u32(&mut output, pixel_bytes);
    push_i32(&mut output, 0);
    push_i32(&mut output, 0);
    push_u32(&mut output, 0);
    push_u32(&mut output, 0);

    let mut alpha = vec![0_u8; (size * size) as usize];
    for output_y in 0..size {
        let y = size - 1 - output_y;
        for x in 0..size {
            let color = sample_pixel(x, y, size);
            alpha[(y * size + x) as usize] = color[3];
            output.extend_from_slice(&[color[2], color[1], color[0], color[3]]);
        }
    }

    for output_y in 0..size {
        let y = size - 1 - output_y;
        let row_start = output.len();
        output.resize(row_start + mask_stride as usize, 0);
        for x in 0..size {
            if alpha[(y * size + x) as usize] == 0 {
                output[row_start + (x / 8) as usize] |= 0x80 >> (x % 8);
            }
        }
    }
    output
}

fn sample_pixel(x: u32, y: u32, size: u32) -> [u8; 4] {
    const SAMPLES: u32 = 4;
    let mut channels = [0_u32; 4];
    for sample_y in 0..SAMPLES {
        for sample_x in 0..SAMPLES {
            let px = (x as f32 + (sample_x as f32 + 0.5) / SAMPLES as f32) / size as f32;
            let py = (y as f32 + (sample_y as f32 + 0.5) / SAMPLES as f32) / size as f32;
            let color = sample_logo(px, py);
            for channel in 0..4 {
                channels[channel] += color[channel] as u32;
            }
        }
    }
    let divisor = SAMPLES * SAMPLES;
    channels.map(|channel| (channel / divisor) as u8)
}

fn sample_logo(x: f32, y: f32) -> [u8; 4] {
    let mut color = if inside_rounded_rect(x, y, 0.03, 0.03, 0.97, 0.97, 0.21) {
        [BLUE[0], BLUE[1], BLUE[2], 255]
    } else {
        [0, 0, 0, 0]
    };
    for (from, to) in [
        ((0.27, 0.25), (0.73, 0.25)),
        ((0.27, 0.75), (0.73, 0.75)),
        ((0.31, 0.29), (0.69, 0.71)),
        ((0.69, 0.29), (0.31, 0.71)),
    ] {
        if distance_to_segment((x, y), from, to) <= 0.045 {
            color = [255, 255, 255, 255];
        }
    }
    if (0.62..=0.75).contains(&y) {
        let half_width = 0.04 + (y - 0.62) * 0.9;
        if (x - 0.5).abs() <= half_width {
            color = [255, 255, 255, 255];
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

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_i32(output: &mut Vec<u8>, value: i32) {
    output.extend_from_slice(&value.to_le_bytes());
}
