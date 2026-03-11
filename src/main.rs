#![windows_subsystem = "windows"]

use raylib::prelude::*;
use nokhwa::{pixel_format::*, utils::*, *};
use std::ptr::null_mut as NULL;
use winapi::um::winuser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{
    traits::{Consumer, Producer, Split},
    HeapRb,
};

fn main() {
    nokhwa_initialize(|_| {
        app();
    });
}

fn no_cam_err() {
    let l_msg: Vec<u16> = "No camera available\0".encode_utf16().collect();
    let l_title: Vec<u16> = "Error\0".encode_utf16().collect();

    unsafe {
        winuser::MessageBoxW(NULL(), l_msg.as_ptr(), l_title.as_ptr(), winuser::MB_OK | winuser::MB_ICONERROR);
    }
}

fn screen(selected_camera_id: i32, mut rl: RaylibHandle, thread: RaylibThread) {
    let cameras = query(utils::ApiBackend::Auto).unwrap();
    let target_format = CameraFormat::new(Resolution::new(1920, 1080), FrameFormat::MJPEG, 60);
    let pixel_format = RequestedFormat::new::<RgbAFormat>(RequestedFormatType::Closest(target_format));
    let index = cameras.get((selected_camera_id - 1) as usize).unwrap().index();
    let mut cam = nokhwa::Camera::new(index.clone(), pixel_format).unwrap();
    cam.open_stream().unwrap();

    let empty_img = Image::gen_image_color(cam.resolution().width() as i32, cam.resolution().height() as i32, Color::BLANK);
    let mut texture = rl.load_texture_from_image(&thread, &empty_img).unwrap();
    texture.set_texture_filter(&thread, TextureFilter::TEXTURE_FILTER_BILINEAR);
    let mut failed_frames = 0 as u32;
    
    while !rl.window_should_close() {
        if rl.is_gesture_detected(Gesture::GESTURE_DOUBLETAP) {
            rl.toggle_fullscreen();
        }

        let frame = match cam.frame() {
            Ok(f) => f,
            Err(e) => {
                rl.trace_log(TraceLogLevel::LOG_WARNING, format!("Skipping frame - Failed to capture: {}", e).as_str());
                failed_frames = failed_frames + 1;
                continue;
            }
        };

        let decoded = match frame.decode_image::<RgbAFormat>() {
            Ok(d) => d,
            Err(e) => {
                rl.trace_log(TraceLogLevel::LOG_WARNING, format!("Skipping frame - Failed to decode: {}", e).as_str());
                failed_frames = failed_frames + 1;
                continue;
            }
        };

        let sw = rl.get_screen_width() as f32;
        let sh = rl.get_screen_height() as f32;
        let dpress = rl.is_key_down(KeyboardKey::KEY_TAB);

        let raw_pixels = decoded.as_raw();
        let _ = texture.update_texture(raw_pixels);
        
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
            d.draw_text(format!("w: {} h: {}", texture.width(), texture.height()).as_str(), 10, 30, 20, Color::RED);
            d.draw_text(format!("failed: {}", failed_frames).as_str(), 10, 50, 20, Color::RED);
        }
    }
}

fn app() {
    let host = cpal::default_host();
    let (mut rl, thread) = raylib::init()
        .size(1600, 900)
        .title("Projector")
        .resizable()
        .log_level(TraceLogLevel::LOG_WARNING)
        .build();
    rl.maximize_window();

    let cameras = query(utils::ApiBackend::Auto).unwrap();
    let mut selected_camera_id = -1i32;
    let mut selected_mic_id = -1i32;

    if cameras.len() == 0 {
        no_cam_err();
        return;
    }

    while !rl.window_should_close() {
        if rl.is_key_down(KeyboardKey::KEY_ENTER) {
            break;
        }

        let sw = rl.get_screen_width();
        let max_num = cameras.len();
        let input_devices = host.input_devices().unwrap().into_iter();
        let max_mic = host.input_devices().unwrap().into_iter().count();
        if let Some(ch) = rl.get_char_pressed() {
            let mut cc = ch as i32 - '0' as i32;
            if cc > 0 && cc <= 9 && cc <= max_num as i32 {
                selected_camera_id = cc as i32;
            } else {
                cc = ch as i32 - 'a' as i32;
                if cc >= 0 && cc < 'z' as i32 - 'a' as i32 && cc < max_mic as i32 {
                    selected_mic_id = cc as i32 + 1;
                }
            }
        }

        if selected_mic_id > max_mic as i32 {
            selected_mic_id = -1;
        }

        let mut d = rl.begin_drawing(&thread);
        d.clear_background(Color::BLACK);
        let txt = "Press Enter to confirm";
        d.draw_text(txt, sw / 2 - d.measure_text(txt, 50)/2, 0, 50, Color::RED);
        
        for i in 0..=cameras.len()-1 {
            let mut color = Color::WHITE;

            if i as i32 == selected_camera_id - 1 {
                color = Color::GREEN;
            }

            d.draw_text(
                format!("{}: {}", i + 1, cameras.get(i).unwrap().human_name().as_str()).as_str(), 
                20, 
                (50 + 50 * i) as i32,
                50,
                color
            );
        }

        for (i, device) in input_devices.enumerate() {
            let mut color = Color::WHITE;
            
            if i as i32 == selected_mic_id - 1 {
                color = Color::GREEN;
            }

            let y_pos = 50 + (i as i32 * 50);
            let label = (i as u8 + b'a') as char;

            d.draw_text(
                &format!("{}: {} ({})", label, device.description().unwrap().driver().unwrap(), device.description().unwrap().name()),
                sw / 2, 
                y_pos, 
                50,
                color
            );
        }
    }

    if selected_camera_id == -1 {
        return;
    }

    let mic_input_devices = host.input_devices();
    let max_mic = host.input_devices().unwrap().into_iter().count();
    
    if selected_mic_id > 0 && selected_mic_id <= max_mic as i32 {
        let mut input = host.default_input_device().expect("no output device");
        let mut i = 0;
        for device in mic_input_devices.unwrap() {
            if i as i32 == selected_mic_id - 1 {
                input = device;
                break;
            }
            i = i+1;
        }
        let output = host.default_output_device().expect("no output device");
        let mut config: cpal::StreamConfig = input.default_input_config().unwrap().into();
        config.buffer_size = cpal::BufferSize::Fixed(64);

        let ring = HeapRb::<f32>::new(config.sample_rate as usize * config.channels as usize);
        let (mut prod, mut cons) = ring.split();

        let input_stream = input
            .build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    for &s in data {
                        let _ = prod.try_push(s);
                    }
                },
                |e| eprintln!("input error: {e}"),
                None,
            )
            .unwrap();

        let output_stream = output
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    for s in data {
                        *s = cons.try_pop().unwrap_or(0.0);
                    }
                },
                |e| eprintln!("output error: {e}"),
                None,
            )
            .unwrap();

        input_stream.play().unwrap();
        output_stream.play().unwrap();

        screen(selected_camera_id, rl, thread);
    } else {
        screen(selected_camera_id, rl, thread);
    }
}
