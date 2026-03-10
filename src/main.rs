#![windows_subsystem = "windows"]

use raylib::prelude::*;
use nokhwa::{pixel_format::*, utils::*, *};

fn main() {
    nokhwa_initialize(|_| {
        app();
    });
}

fn app() {
    let (mut rl, thread) = raylib::init()
        .size(1600, 900)
        .title("Projector")
        .resizable()
        .log_level(TraceLogLevel::LOG_WARNING)
        .build();
    rl.maximize_window();

    let cameras = query(utils::ApiBackend::Auto).unwrap();
    let mut selected_camera_id = -1i32;

    while !rl.window_should_close() {
        let max_num = cameras.len();
        if let Some(ch) = rl.get_char_pressed() {
            let cc = ch as u32 - '0' as u32;
            if cc > 0 && cc <= 9 && cc <= max_num as u32 {
                selected_camera_id = cc as i32;
                break;
            }
        }

        let mut d = rl.begin_drawing(&thread);
        d.clear_background(Color::BLACK);
        for i in 0..=cameras.len()-1 {
            d.draw_text(format!("{}: {}", i + 1, cameras.get(i).unwrap().human_name().as_str()).as_str(), 20, (50 + 50 * i).try_into().unwrap(), 50, Color::WHITE);
        }
    }

    if selected_camera_id == -1 {
        return;
    }

    let target_format = CameraFormat::new(Resolution::new(1920, 1080), FrameFormat::MJPEG, 60);
    let pixel_format = RequestedFormat::new::<RgbAFormat>(RequestedFormatType::Closest(target_format));
    let index = cameras.get((selected_camera_id - 1) as usize).unwrap().index();
    let mut cam = nokhwa::Camera::new(index.clone(), pixel_format).unwrap();
    cam.open_stream().unwrap();

    let empty_img = Image::gen_image_color(1920, 1080, Color::BLACK);
    let mut texture = rl.load_texture_from_image(&thread, &empty_img).unwrap();
    texture.set_texture_filter(&thread, TextureFilter::TEXTURE_FILTER_BILINEAR);
    
    while !rl.window_should_close() {
        let frame = match cam.frame() {
            Ok(f) => f,
            Err(e) => {
                rl.trace_log(TraceLogLevel::LOG_WARNING, format!("Skipping frame - Failed to capture: {}", e).as_str());
                continue;
            }
        };

        let decoded = match frame.decode_image::<RgbAFormat>() {
            Ok(d) => d,
            Err(e) => {
                rl.trace_log(TraceLogLevel::LOG_WARNING, format!("Skipping frame - Failed to decode: {}", e).as_str());
                continue;
            }
        };

        let sw = rl.get_screen_width() as f32;
        let sh = rl.get_screen_height() as f32;
        let dpress = rl.is_key_down(KeyboardKey::KEY_TAB);

        let raw_pixels = decoded.as_raw();
        texture.update_texture(raw_pixels).unwrap();
        
        let tex_w = texture.width() as f32;
        let tex_h = texture.height() as f32;

        let scale = (sw / tex_w).min(sh / tex_h);
        
        let dest_w = tex_w * scale;
        let dest_h = tex_h * scale;
        
        let offset_x = (sw - dest_w) / 2.0;
        let offset_y = (sh - dest_h) / 2.0;

        let src_rec = Rectangle::new(0.0, 0.0, tex_w, tex_h);
        let dst_rec = Rectangle::new(offset_x, offset_y, dest_w, dest_h);
        let origin = Vector2::new(0.0, 0.0);
        
        let mut d = rl.begin_drawing(&thread);
        d.clear_background(Color::BLACK);
        d.draw_texture_pro(&texture, src_rec, dst_rec, origin, 0.0, Color::WHITE);
        
        if dpress {
            d.draw_fps(10, 10);
        }
    }
}