extern crate ffmpeg_next as ffmpeg;

use cosmic::widget;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, SizedSample,
};
use ffmpeg::{
    format::{input, Pixel},
    media::Type,
    software::{resampling, scaling},
    util::{
        channel_layout, error,
        format::sample,
        frame::{audio::Audio, video::Video},
    },
    Packet,
};
use std::{
    collections::VecDeque,
    error::Error,
    path::{Path, PathBuf},
    slice,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub enum PlayerMessage {
    SeekRelative(f64),
}

pub struct VideoFrame(pub Video);

impl VideoFrame {
    pub fn into_handle(self) -> widget::image::Handle {
        let width = self.0.width();
        let height = self.0.height();
        widget::image::Handle::from_pixels(width, height, self)
    }
}

impl AsRef<[u8]> for VideoFrame {
    fn as_ref(&self) -> &[u8] {
        self.0.data(0)
    }
}

fn cpal(audio_queue_lock: Arc<Mutex<VecDeque<f32>>>) -> cpal::SupportedStreamConfig {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("failed to get default audio output device");
    let config = device
        .default_output_config()
        .expect("failed to get default audio output config");
    println!("{:?}: {:?}", device.name(), config);

    {
        let config = config.clone();
        thread::spawn(move || {
            match config.sample_format() {
                cpal::SampleFormat::I8 => {
                    cpal_thread::<i8>(device, config.into(), audio_queue_lock)
                }
                cpal::SampleFormat::I16 => {
                    cpal_thread::<i16>(device, config.into(), audio_queue_lock)
                }
                // cpal::SampleFormat::I24 => cpal_thread::<I24>(device, config.into(), audio_queue_lock),
                cpal::SampleFormat::I32 => {
                    cpal_thread::<i32>(device, config.into(), audio_queue_lock)
                }
                // cpal::SampleFormat::I48 => cpal_thread::<I48>(device, config.into(), audio_queue_lock),
                cpal::SampleFormat::I64 => {
                    cpal_thread::<i64>(device, config.into(), audio_queue_lock)
                }
                cpal::SampleFormat::U8 => {
                    cpal_thread::<u8>(device, config.into(), audio_queue_lock)
                }
                cpal::SampleFormat::U16 => {
                    cpal_thread::<u16>(device, config.into(), audio_queue_lock)
                }
                // cpal::SampleFormat::U24 => cpal_thread::<U24>(device, config.into(), audio_queue_lock),
                cpal::SampleFormat::U32 => {
                    cpal_thread::<u32>(device, config.into(), audio_queue_lock)
                }
                // cpal::SampleFormat::U48 => cpal_thread::<U48>(device, config.into(), audio_queue_lock),
                cpal::SampleFormat::U64 => {
                    cpal_thread::<u64>(device, config.into(), audio_queue_lock)
                }
                cpal::SampleFormat::F32 => {
                    cpal_thread::<f32>(device, config.into(), audio_queue_lock)
                }
                cpal::SampleFormat::F64 => {
                    cpal_thread::<f64>(device, config.into(), audio_queue_lock)
                }
                sample_format => panic!("unsupported sample format '{sample_format}'"),
            }
            .unwrap();
        });
    }

    config
}

fn cpal_thread<T>(
    device: cpal::Device,
    config: cpal::StreamConfig,
    audio_queue_lock: Arc<Mutex<VecDeque<f32>>>,
) -> Result<(), Box<dyn Error>>
where
    T: SizedSample + FromSample<f32>,
{
    let data_fn = {
        let audio_queue_lock = audio_queue_lock.clone();
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let mut underrun = 0;
            {
                //TODO: buffer audio
                let mut audio_queue = audio_queue_lock.lock().unwrap();
                for sample in data {
                    let float = match audio_queue.pop_front() {
                        Some(some) => some,
                        None => {
                            underrun += 1;
                            0.0
                        }
                    };
                    *sample = T::from_sample(float);
                }
            }
            if underrun > 0 {
                log::error!("audio underrun {}", underrun);
            }
        }
    };

    let err_fn = |err| eprintln!("an error occurred on stream: {}", err);

    let stream = device.build_output_stream(&config, data_fn, err_fn, None)?;
    stream.play()?;

    loop {
        //TODO: move this code to ffmpeg_thread so we don't have to sleep here?
        thread::sleep(Duration::from_millis(1000));
    }
}

fn ffmpeg_thread<P: AsRef<Path>>(
    path: P,
    player_rx: mpsc::Receiver<PlayerMessage>,
    video_frame_lock: Arc<Mutex<Option<VideoFrame>>>,
    audio_config: cpal::SupportedStreamConfig,
    audio_queue_lock: Arc<Mutex<VecDeque<f32>>>,
) -> Result<(), ffmpeg::Error> {
    let mut ictx = input(&path)?;

    let video_stream = ictx
        .streams()
        .best(Type::Video)
        .ok_or(ffmpeg::Error::StreamNotFound)?;
    let video_stream_index = video_stream.index();

    let video_context_decoder =
        ffmpeg::codec::context::Context::from_parameters(video_stream.parameters())?;
    let mut video_decoder = video_context_decoder.decoder().video()?;

    let video_format = video_decoder.format();
    let video_width = video_decoder.width();
    let video_height = video_decoder.height();
    let (raw_frame_tx, raw_frame_rx) = mpsc::channel();
    thread::spawn(move || -> Result<(), ffmpeg::Error> {
        let mut video_scaler = scaling::context::Context::get(
            video_format,
            video_width,
            video_height,
            Pixel::RGBA,
            video_width,
            video_height,
            scaling::Flags::FAST_BILINEAR,
        )?;

        loop {
            let start = Instant::now();

            let mut raw_frame: Video = raw_frame_rx.recv().unwrap();
            while let Ok(extra_frame) = raw_frame_rx.try_recv() {
                log::warn!("missed raw video frame at {:?}", raw_frame.pts());
                raw_frame = extra_frame;
            }
            let pts = raw_frame.pts();

            let mut scaled_frame = Video::empty();
            video_scaler.run(&raw_frame, &mut scaled_frame)?;
            scaled_frame.set_pts(pts);
            let missed_pts_opt = {
                let mut video_frame_opt = video_frame_lock.lock().unwrap();
                let missed_pts_opt = match &*video_frame_opt {
                    Some(old_frame) => Some(old_frame.0.pts()),
                    None => None,
                };
                *video_frame_opt = Some(VideoFrame(scaled_frame));
                missed_pts_opt
            };
            if let Some(missed_pts) = missed_pts_opt {
                log::warn!("missed scaled video frame at {:?}", missed_pts);
            }

            let duration = start.elapsed();
            log::debug!("scaled video frame at {:?} in {:?}", pts, duration);
        }
    });

    let (video_packet_tx, video_packet_rx) = mpsc::channel();
    thread::spawn(move || -> Result<(), ffmpeg::Error> {
        let mut eof = false;
        while !eof {
            let start = Instant::now();

            match video_packet_rx.recv() {
                Ok(packet) => {
                    video_decoder.send_packet(&packet)?;
                }
                Err(_err) => {
                    video_decoder.send_eof()?;
                    eof = true;
                }
            }

            let mut pts = None;
            let mut video_frames = 0;
            loop {
                let mut raw_frame = Video::empty();
                if video_decoder.receive_frame(&mut raw_frame).is_ok() {
                    pts = raw_frame.pts();
                    raw_frame_tx.send(raw_frame).unwrap();
                    video_frames += 1;
                } else {
                    break;
                }
            }

            if video_frames > 0 {
                let duration = start.elapsed();
                log::debug!(
                    "received {} video frames at {:?} in {:?}",
                    video_frames,
                    pts,
                    duration
                );
            }
        }
        Ok(())
    });

    let audio_stream = ictx
        .streams()
        .best(Type::Audio)
        .ok_or(ffmpeg::Error::StreamNotFound)?;
    let audio_stream_index = audio_stream.index();
    let audio_time_base = f64::from(audio_stream.time_base());

    let audio_context_decoder =
        ffmpeg::codec::context::Context::from_parameters(audio_stream.parameters())?;
    let mut audio_decoder = audio_context_decoder.decoder().audio()?;

    let mut audio_resampler = resampling::Context::get(
        audio_decoder.format(),
        audio_decoder.channel_layout(),
        audio_decoder.rate(),
        //TODO: support other formats?
        sample::Sample::F32(sample::Type::Packed),
        match audio_config.channels() {
            1 => channel_layout::ChannelLayout::MONO,
            2 => channel_layout::ChannelLayout::STEREO,
            //TODO: more channel configs
            unsupported => {
                panic!("unsupported audio channels {:?}", unsupported);
            }
        },
        audio_config.sample_rate().0,
    )?;

    let mut sync_sample = 0;
    let mut current_sample = 0;
    let min_sleep = Duration::from_millis(1);
    let min_skip = Duration::from_millis(1);
    let mut receive_and_process_decoded_audio_frames = |decoder: &mut ffmpeg::decoder::Audio,
                                                        sync_time_opt: &mut Option<Instant>|
     -> Result<(), ffmpeg::Error> {
        let mut decoded = Audio::empty();
        let mut resampled = Audio::empty();
        while decoder.receive_frame(&mut decoded).is_ok() {
            audio_resampler.run(&decoded, &mut resampled)?;
            {
                // plane method doesn't work with packed samples, so do it manually
                let plane = unsafe {
                    slice::from_raw_parts(
                        (*resampled.as_ptr()).data[0] as *const f32,
                        resampled.samples() * resampled.channels() as usize,
                    )
                };
                {
                    let mut audio_queue = audio_queue_lock.lock().unwrap();
                    audio_queue.extend(plane);
                }
            }

            current_sample += decoded.samples();
            if sync_time_opt.is_none() {
                *sync_time_opt = Some(Instant::now());
                sync_sample = current_sample;
            }
        }

        // Sync with audio
        if let Some(sync_time) = &sync_time_opt {
            let samples = current_sample - sync_sample;
            let expected_float = (samples as f64) / f64::from(decoder.rate());
            let expected = Duration::from_secs_f64(expected_float);
            let actual = sync_time.elapsed();
            if expected > actual {
                let sleep = expected - actual;
                if sleep > min_sleep {
                    // We leave min_sleep of buffer room
                    thread::sleep(sleep - min_sleep);
                }
            } else {
                let skip = actual - expected;
                if skip > min_skip {
                    //TODO: handle frame skipping
                }
            }
        }
        Ok(())
    };

    let mut sync_time_opt = None;
    let mut seconds_opt = None;
    loop {
        let mut packet = Packet::empty();
        match packet.read(&mut ictx) {
            Ok(()) => {
                if packet.stream() == video_stream_index {
                    video_packet_tx.send(packet).unwrap();
                } else if packet.stream() == audio_stream_index {
                    audio_decoder.send_packet(&packet)?;
                    receive_and_process_decoded_audio_frames(
                        &mut audio_decoder,
                        &mut sync_time_opt,
                    )?;
                    if let Some(pts) = packet.pts() {
                        seconds_opt = Some(pts as f64 * audio_time_base);
                    }
                }
            }
            Err(error::Error::Eof) => break,
            Err(_err) => {}
        }

        while let Ok(message) = player_rx.try_recv() {
            match message {
                PlayerMessage::SeekRelative(seek_seconds) => {
                    if let Some(seconds) = seconds_opt {
                        //TODO: use time base instead of hardcoded values
                        let timestamp = ((seconds + seek_seconds) * 1000000.0) as i64;
                        if seek_seconds.is_sign_negative() {
                            println!(
                                "backwards {} from {} = {}",
                                seek_seconds, seconds, timestamp
                            );
                            ictx.seek(timestamp, ..timestamp)?;
                        } else {
                            println!("forwards {} from {} = {}", seek_seconds, seconds, timestamp);
                            ictx.seek(timestamp, timestamp..)?;
                        }

                        //TODO: improve sync when seeking
                        // Clear audio sync time
                        sync_time_opt = None;
                    }
                }
            }
        }
    }

    audio_decoder.send_eof()?;
    receive_and_process_decoded_audio_frames(&mut audio_decoder, &mut sync_time_opt)?;

    Ok(())
}

pub fn run(path: PathBuf) -> (mpsc::Sender<PlayerMessage>, Arc<Mutex<Option<VideoFrame>>>) {
    ffmpeg::init().unwrap();

    let audio_queue_lock = Arc::new(Mutex::new(VecDeque::new()));
    let audio_config = cpal(audio_queue_lock.clone());

    let (player_tx, player_rx) = mpsc::channel();
    let video_frame_lock = Arc::new(Mutex::new(None));
    {
        let video_frame_lock = video_frame_lock.clone();
        thread::spawn(move || {
            ffmpeg_thread(
                path,
                player_rx,
                video_frame_lock,
                audio_config,
                audio_queue_lock,
            )
            .unwrap();
        });
    }
    (player_tx, video_frame_lock)
}
