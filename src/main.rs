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

fn show_err(msg: &str) {
    let l_msg: Vec<u16> = format!("{msg}\0").encode_utf16().collect();
    let l_title: Vec<u16> = "Error\0".encode_utf16().collect();

    unsafe {
        winuser::MessageBoxW(NULL(), l_msg.as_ptr(), l_title.as_ptr(), winuser::MB_OK | winuser::MB_ICONERROR);
    }
}

fn start_audio(host: &cpal::Host, input: &cpal::Device) -> Result<(cpal::Stream, cpal::Stream), Box<dyn std::error::Error>> {
    let output = host.default_output_device().ok_or("No output device")?;
    let supported = input.default_input_config()?;
    let mut config = supported.config();
    if let cpal::SupportedBufferSize::Range { min, max } = *supported.buffer_size() {
        config.buffer_size = cpal::BufferSize::Fixed(64.clamp(min, max));
    }

    // ~50ms of buffering so latency can't build up from clock drift
    let ring = HeapRb::<f32>::new(config.sample_rate as usize * config.channels as usize / 20);
    let (mut prod, mut cons) = ring.split();

    let input_stream = input.build_input_stream(
        &config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            prod.push_slice(data);
        },
        |e| eprintln!("input error: {e}"),
        None,
    )?;

    let output_stream = output.build_output_stream(
        &config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            for s in data {
                *s = cons.try_pop().unwrap_or(0.0);
            }
        },
        |e| eprintln!("output error: {e}"),
        None,
    )?;

    input_stream.play()?;
    output_stream.play()?;
    Ok((input_stream, output_stream))
}

fn screen(index: CameraIndex, mut rl: RaylibHandle, thread: RaylibThread) {
    let target_format = CameraFormat::new(Resolution::new(1920, 1080), FrameFormat::MJPEG, 60);
    let pixel_format = RequestedFormat::new::<RgbAFormat>(RequestedFormatType::Closest(target_format));
    let mut cam = match nokhwa::Camera::new(index, pixel_format) {
        Ok(c) => c,
        Err(e) => return show_err(&format!("Failed to open camera: {e}")),
    };
    if let Err(e) = cam.open_stream() {
        return show_err(&format!("Failed to start camera: {e}"));
    }

    let empty_img = Image::gen_image_color(cam.resolution().width() as i32, cam.resolution().height() as i32, Color::BLANK);
    let mut texture = rl.load_texture_from_image(&thread, &empty_img).unwrap();
    texture.set_texture_filter(&thread, TextureFilter::TEXTURE_FILTER_BILINEAR);
    let mut failed_frames = 0u32;

    let mut cooldown = 0.0;
    while !rl.window_should_close() {
        cooldown -= rl.get_frame_time();
        if cooldown <= 0.0 && rl.is_mouse_button_pressed(MouseButton::MOUSE_BUTTON_RIGHT) {
            rl.toggle_borderless_windowed();
            cooldown = 1.0;
        }

        match cam.frame().and_then(|f| f.decode_image::<RgbAFormat>()) {
            Ok(decoded) => {
                let _ = texture.update_texture(decoded.as_raw());
            }
            Err(e) => {
                rl.trace_log(TraceLogLevel::LOG_WARNING, &format!("Skipping frame: {e}"));
                failed_frames += 1;
            }
        }

        let sw = rl.get_screen_width() as f32;
        let sh = rl.get_screen_height() as f32;
        let dpress = rl.is_key_down(KeyboardKey::KEY_TAB);

        let tex_w = texture.width() as f32;
        let tex_h = texture.height() as f32;

        let scale = (sw / tex_w).min(sh / tex_h);

        let dest_w = tex_w * scale;
        let dest_h = tex_h * scale;

        let offset_x = (sw - dest_w) / 2.0;
        let offset_y = (sh - dest_h) / 2.0;

        let src_rec = Rectangle::new(0.0, 0.0, tex_w, tex_h);
        let dst_rec = Rectangle::new(offset_x, offset_y, dest_w, dest_h);

        let mut d = rl.begin_drawing(&thread);
        d.clear_background(Color::BLACK);
        d.draw_texture_pro(&texture, src_rec, dst_rec, Vector2::zero(), 0.0, Color::WHITE);

        if dpress {
            d.draw_fps(10, 10);
            d.draw_text(&format!("w: {} h: {}", texture.width(), texture.height()), 10, 30, 20, Color::RED);
            d.draw_text(&format!("failed: {failed_frames}"), 10, 50, 20, Color::RED);
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

    let cameras = query(ApiBackend::Auto).unwrap_or_default();
    if cameras.is_empty() {
        return show_err("No camera available");
    }

    let mics: Vec<cpal::Device> = host.input_devices().map(|d| d.collect()).unwrap_or_default();
    let mic_names: Vec<String> = mics
        .iter()
        .map(|m| match m.description() {
            Ok(desc) => format!("{} ({})", desc.driver().unwrap_or("?"), desc.name()),
            Err(_) => "Unknown".to_string(),
        })
        .collect();

    let mut selected_camera: Option<usize> = None;
    let mut selected_mic: Option<usize> = None;

    while !rl.window_should_close() {
        if selected_camera.is_some() && rl.is_key_pressed(KeyboardKey::KEY_ENTER) {
            break;
        }

        if let Some(ch) = rl.get_char_pressed() {
            let cam = (ch as usize).wrapping_sub('1' as usize);
            let mic = (ch as usize).wrapping_sub('a' as usize);
            if cam < cameras.len().min(9) {
                selected_camera = Some(cam);
            }
            if mic < mics.len().min(26) {
                selected_mic = Some(mic);
            }
        }

        let sw = rl.get_screen_width();
        let mut d = rl.begin_drawing(&thread);
        d.clear_background(Color::BLACK);
        let txt = "Press Enter to confirm";
        d.draw_text(txt, sw / 2 - d.measure_text(txt, 50) / 2, 0, 50, Color::RED);

        for (i, camera) in cameras.iter().enumerate() {
            let mut color = Color::WHITE;
            if selected_camera == Some(i) {
                color = Color::GREEN;
            }
            d.draw_text(&format!("{}: {}", i + 1, camera.human_name()), 20, 50 + 50 * i as i32, 50, color);
        }

        for (i, name) in mic_names.iter().enumerate() {
            let mut color = Color::WHITE;
            if selected_mic == Some(i) {
                color = Color::GREEN;
            }
            let label = (i as u8 + b'a') as char;
            d.draw_text(&format!("{label}: {name}"), sw / 2, 50 + 50 * i as i32, 50, color);
        }
    }

    let Some(camera) = selected_camera.filter(|_| !rl.window_should_close()) else {
        return;
    };

    // Streams must stay alive while the screen is shown
    let _audio = selected_mic.and_then(|i| {
        start_audio(&host, &mics[i])
            .inspect_err(|e| show_err(&format!("Audio unavailable: {e}")))
            .ok()
    });

    screen(cameras[camera].index().clone(), rl, thread);
}
