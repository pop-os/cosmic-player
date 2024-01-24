extern crate ffmpeg_next as ffmpeg;

use ffmpeg::format::{input, Pixel};
use ffmpeg::media::Type;
use ffmpeg::software::scaling::{context::Context, flag::Flags};
use ffmpeg::util::frame::video::Video;
use std::cmp;
use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::thread;
use winit::event::{Event, KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowBuilder;

fn ffmpeg(event_loop_proxy: EventLoopProxy<Video>) -> Result<(), ffmpeg::Error> {
    ffmpeg::init().unwrap();

    if let Ok(mut ictx) = input(&env::args().nth(1).expect("Cannot open file.")) {
        let input = ictx
            .streams()
            .best(Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?;
        let video_stream_index = input.index();

        let context_decoder = ffmpeg::codec::context::Context::from_parameters(input.parameters())?;
        let mut decoder = context_decoder.decoder().video()?;

        let mut scaler = Context::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            Pixel::RGB24,
            1280, //TODO decoder.width(),
            720,  //TODO decoder.height(),
            Flags::BILINEAR,
        )?;

        let mut receive_and_process_decoded_frames =
            |decoder: &mut ffmpeg::decoder::Video| -> Result<(), ffmpeg::Error> {
                let mut decoded = Video::empty();
                while decoder.receive_frame(&mut decoded).is_ok() {
                    let mut rgb_frame = Video::empty();
                    scaler.run(&decoded, &mut rgb_frame)?;
                    match event_loop_proxy.send_event(rgb_frame) {
                        Ok(()) => {}
                        Err(_err) => {
                            panic!("event loop closed");
                        }
                    }
                }
                Ok(())
            };

        for (stream, packet) in ictx.packets() {
            if stream.index() == video_stream_index {
                decoder.send_packet(&packet)?;
                receive_and_process_decoded_frames(&mut decoder)?;
            }
        }
        decoder.send_eof()?;
        receive_and_process_decoded_frames(&mut decoder)?;
    }

    Ok(())
}

fn main() {
    let event_loop = EventLoopBuilder::<Video>::with_user_event()
        .build()
        .unwrap();
    let event_loop_proxy = event_loop.create_proxy();
    thread::spawn(move || {
        ffmpeg(event_loop_proxy).unwrap();
    });

    let window = Rc::new(WindowBuilder::new().build(&event_loop).unwrap());
    let context = softbuffer::Context::new(window.clone()).unwrap();
    let mut surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

    let mut rgb_frame_opt: Option<Video> = None;
    event_loop
        .run(move |event, elwt| {
            elwt.set_control_flow(ControlFlow::Wait);

            match event {
                Event::WindowEvent {
                    window_id,
                    event: WindowEvent::RedrawRequested,
                } if window_id == window.id() => {
                    if let (Some(width), Some(height)) = {
                        let size = window.inner_size();
                        (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                    } {
                        surface.resize(width, height).unwrap();
                        //TODO: send size back to ffmpeg thread

                        let mut buffer = surface.buffer_mut().unwrap();
                        let buffer_width = width.get() as usize;
                        let buffer_height = height.get() as usize;
                        if let Some(rgb_frame) = &rgb_frame_opt {
                            let data = rgb_frame.data(0);
                            let data_width = rgb_frame.width() as usize;
                            let data_height = rgb_frame.height() as usize;
                            //TODO: stride?
                            for y in 0..cmp::min(buffer_height, data_height) {
                                for x in 0..cmp::min(buffer_width, data_width) {
                                    let data_index = (y * data_width + x) * 3;
                                    let red = data[data_index] as u32;
                                    let green = data[data_index + 1] as u32;
                                    let blue = data[data_index + 2] as u32;
                                    let buffer_index = y * buffer_width + x;
                                    buffer[buffer_index] = blue | (green << 8) | (red << 16);
                                }
                            }
                        }

                        buffer.present().unwrap();
                    }
                }
                Event::WindowEvent {
                    event:
                        WindowEvent::CloseRequested
                        | WindowEvent::KeyboardInput {
                            event:
                                KeyEvent {
                                    logical_key: Key::Named(NamedKey::Escape),
                                    ..
                                },
                            ..
                        },
                    window_id,
                } if window_id == window.id() => {
                    elwt.exit();
                }
                Event::UserEvent(rgb_frame) => {
                    rgb_frame_opt = Some(rgb_frame);
                    window.request_redraw();
                }
                _ => {}
            }
        })
        .unwrap();
}
