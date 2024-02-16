extern crate ffmpeg_next as ffmpeg;

use cosmic::widget;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, SizedSample,
};
use ffmpeg::{
    codec, ffi,
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
    cmp,
    collections::VecDeque,
    error::Error,
    path::{Path, PathBuf},
    ptr, slice,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use crate::config::Config;

//TODO: calculate presentation time of end of queue
pub struct AudioQueue {
    pub channels: usize,
    pub rate: f64,
    pub data: VecDeque<f32>,
    // Delay for data to hit speakers, used to sync with video
    pub delay: Duration,
}

impl AudioQueue {
    pub fn new(channels: cpal::ChannelCount, rate: cpal::SampleRate) -> Self {
        Self {
            channels: channels as usize,
            rate: rate.0 as f64,
            data: VecDeque::new(),
            delay: Duration::default(),
        }
    }

    pub fn duration(&self) -> Duration {
        self.duration_for_samples(self.data.len())
    }

    pub fn duration_for_samples(&self, samples: usize) -> Duration {
        let frames = samples / self.channels;
        let seconds = (frames as f64) / self.rate;
        Duration::from_secs_f64(seconds)
    }
}

#[derive(Clone, Debug)]
pub enum PlayerMessage {
    SeekRelative(f64),
}

pub struct VideoFrame(pub Video, pub Option<Instant>);

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

pub struct VideoQueue {
    pub data: VecDeque<VideoFrame>,
    // Delay to add to each frame to sync with audio
    pub delay: Duration,
}

impl VideoQueue {
    pub fn new() -> Self {
        Self {
            data: VecDeque::new(),
            delay: Duration::default(),
        }
    }

    pub fn push(&mut self, frame: VideoFrame) {
        // Discard all frames that are newer than frame to fix seeking and duration calculation
        self.data
            .retain(|other| other.1.map_or(true, |x| x <= frame.1.unwrap_or(x)));
        self.data.push_back(frame);
    }

    pub fn duration(&self) -> Duration {
        //TODO: can accurate duration actually be calculated since one frame would count as zero?
        let mut start_end_opt = None;
        for frame in self.data.iter() {
            if let Some(frame_time) = frame.1 {
                start_end_opt = Some(match start_end_opt {
                    Some((start, end)) => (cmp::min(start, frame_time), cmp::max(end, frame_time)),
                    None => (frame_time, frame_time),
                });
            }
        }
        if let Some((start, end)) = start_end_opt {
            end.duration_since(start)
        } else {
            Duration::default()
        }
    }
}

fn cpal() -> (
    cpal::SupportedStreamConfig,
    Box<dyn StreamTrait>,
    Arc<Mutex<AudioQueue>>,
) {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("failed to get default audio output device");
    let config = device
        .default_output_config()
        .expect("failed to get default audio output config");
    println!("{:?}: {:?}", device.name(), config);

    let audio_queue_lock = Arc::new(Mutex::new(AudioQueue::new(
        config.channels(),
        config.sample_rate(),
    )));
    let stream = {
        let config = config.clone();
        let audio_queue_lock = audio_queue_lock.clone();
        match config.sample_format() {
            cpal::SampleFormat::I8 => cpal_stream::<i8>(device, config.into(), audio_queue_lock),
            cpal::SampleFormat::I16 => cpal_stream::<i16>(device, config.into(), audio_queue_lock),
            // cpal::SampleFormat::I24 => cpal_stream::<I24>(device, config.into(), audio_queue_lock),
            cpal::SampleFormat::I32 => cpal_stream::<i32>(device, config.into(), audio_queue_lock),
            // cpal::SampleFormat::I48 => cpal_stream::<I48>(device, config.into(), audio_queue_lock),
            cpal::SampleFormat::I64 => cpal_stream::<i64>(device, config.into(), audio_queue_lock),
            cpal::SampleFormat::U8 => cpal_stream::<u8>(device, config.into(), audio_queue_lock),
            cpal::SampleFormat::U16 => cpal_stream::<u16>(device, config.into(), audio_queue_lock),
            // cpal::SampleFormat::U24 => cpal_stream::<U24>(device, config.into(), audio_queue_lock),
            cpal::SampleFormat::U32 => cpal_stream::<u32>(device, config.into(), audio_queue_lock),
            // cpal::SampleFormat::U48 => cpal_stream::<U48>(device, config.into(), audio_queue_lock),
            cpal::SampleFormat::U64 => cpal_stream::<u64>(device, config.into(), audio_queue_lock),
            cpal::SampleFormat::F32 => cpal_stream::<f32>(device, config.into(), audio_queue_lock),
            cpal::SampleFormat::F64 => cpal_stream::<f64>(device, config.into(), audio_queue_lock),
            sample_format => panic!("unsupported sample format '{sample_format}'"),
        }
        .unwrap()
    };

    (config, stream, audio_queue_lock)
}

fn cpal_stream<T>(
    device: cpal::Device,
    config: cpal::StreamConfig,
    audio_queue_lock: Arc<Mutex<AudioQueue>>,
) -> Result<Box<dyn StreamTrait>, Box<dyn Error>>
where
    T: SizedSample + FromSample<f32>,
{
    let data_fn = {
        move |samples: &mut [T], info: &cpal::OutputCallbackInfo| {
            let timestamp = info.timestamp();
            let delay = timestamp.playback.duration_since(&timestamp.callback);

            let mut underrun = 0;
            {
                //TODO: buffer audio
                let mut audio_queue = audio_queue_lock.lock().unwrap();
                //TODO: also add samples time?
                audio_queue.delay = delay.unwrap_or_default();
                for sample in samples {
                    let float = match audio_queue.data.pop_front() {
                        Some(some) => some,
                        None => {
                            underrun += 1;
                            0.0
                        }
                    };
                    *sample = T::from_sample(float);
                }
            };
            if underrun > 0 {
                log::error!("audio underrun {}", underrun);
            }
        }
    };
    let err_fn = |err| eprintln!("an error occurred on stream: {}", err);
    let stream = device.build_output_stream(&config, data_fn, err_fn, None)?;
    Ok(Box::new(stream))
}

fn ffmpeg_thread<P: AsRef<Path>>(
    path: P,
    player_rx: mpsc::Receiver<PlayerMessage>,
    video_queue_lock: Arc<Mutex<VideoQueue>>,
    config: Config,
) -> Result<(), Box<dyn Error>> {
    let (audio_config, cpal_stream, audio_queue_lock) = cpal();

    let mut ictx = input(&path)?;

    let video_stream = ictx
        .streams()
        .best(Type::Video)
        .ok_or(ffmpeg::Error::StreamNotFound)?;
    let video_stream_index = video_stream.index();
    let video_time_base = f64::from(video_stream.time_base());

    let mut video_decoder = {
        let mut video_decoder_context =
            codec::context::Context::from_parameters(video_stream.parameters())?;

        //TODO: safe wrappers
        let mut hw_device_ctx = ptr::null_mut();
        unsafe {
            //TODO: support other types
            let hw_device_kind = config.hw_decoder;
            if ffi::av_hwdevice_ctx_create(
                &mut hw_device_ctx,
                hw_device_kind.into(),
                ptr::null(),
                ptr::null_mut(),
                0,
            ) == 0
            {
                log::info!("using {hw_device_kind} decoding");
                (&mut *video_decoder_context.as_mut_ptr()).hw_device_ctx =
                    ffi::av_buffer_ref(hw_device_ctx);
            } else {
                //TODO: support other hardware devices
                log::warn!(
                    "failed to use {hw_device_kind} decoding, falling back to software decoding"
                );
            }
        }

        video_decoder_context.decoder().video()?
    };

    let (cpu_frame_tx, cpu_frame_rx) = mpsc::channel::<(Video, Option<Instant>)>();
    {
        let video_format = video_decoder.format();
        let video_width = video_decoder.width();
        let video_height = video_decoder.height();
        let video_queue_lock = video_queue_lock.clone();
        thread::Builder::new()
            .name("video_scale".to_string())
            .spawn(move || {
                let mut video_scaler = scaling::context::Context::get(
                    video_format,
                    video_width,
                    video_height,
                    Pixel::RGBA,
                    video_width,
                    video_height,
                    scaling::Flags::FAST_BILINEAR,
                )
                .unwrap();

                loop {
                    let mut recv_opt: Option<(Video, Option<Instant>)> = None;
                    /*TODO: SKIP
                    while let Ok(recv) = cpu_frame_rx.try_recv() {
                        if let Some((old_frame, _)) = recv_opt {
                            //TODO: only skip if behind (frames come in weird timing from codecs)
                            log::warn!("skipping cpu video frame at {:?}", old_frame.pts());
                        }
                        recv_opt = Some(recv);
                    }
                    */
                    let (cpu_frame, sync_time_opt) = match recv_opt {
                        Some(some) => some,
                        None => cpu_frame_rx.recv().unwrap(),
                    };
                    let pts_opt = cpu_frame.pts();

                    // Start count after blocking recv
                    let start = Instant::now();

                    video_scaler.cached(
                        cpu_frame.format(),
                        cpu_frame.width(),
                        cpu_frame.height(),
                        Pixel::RGBA,
                        cpu_frame.width(),
                        cpu_frame.height(),
                        scaling::Flags::FAST_BILINEAR,
                    );

                    let mut scaled_frame = Video::empty();
                    video_scaler.run(&cpu_frame, &mut scaled_frame).unwrap();
                    scaled_frame.set_pts(pts_opt);

                    let present_time_opt = if let Some(pts) = pts_opt {
                        let expected_float = pts as f64 * video_time_base;
                        let expected = Duration::from_secs_f64(expected_float);
                        if let Some(sync_time) = sync_time_opt {
                            Some(sync_time + expected)
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let video_frame = VideoFrame(scaled_frame, present_time_opt);
                    {
                        let mut video_queue = video_queue_lock.lock().unwrap();
                        video_queue.push(video_frame);
                    }

                    let duration = start.elapsed();
                    log::debug!("scaled video frame at {:?} in {:?}", pts_opt, duration,);
                }
            })?
    };

    // Sync channel to prevent allocation issues and falling behind
    let (gpu_frame_tx, gpu_frame_rx) = mpsc::sync_channel::<(Video, Option<Instant>)>(2);
    thread::Builder::new()
        .name("video_map_gpu_cpu".to_string())
        .spawn(move || {
            loop {
                let mut recv_opt: Option<(Video, Option<Instant>)> = None;
                /*TODO: SKIP
                while let Ok(recv) = gpu_frame_rx.try_recv() {
                    if let Some((old_frame, _)) = recv_opt {
                        //TODO: only skip if behind (frames come in weird timing from codecs)
                        log::warn!("skipping gpu video frame at {:?}", old_frame.pts());
                    }
                    recv_opt = Some(recv);
                }
                */
                let (gpu_frame, sync_time_opt) = match recv_opt {
                    Some(some) => some,
                    None => gpu_frame_rx.recv().unwrap(),
                };
                let pts = gpu_frame.pts();

                // Start timer after blocking recv
                let start = Instant::now();

                let mut cpu_frame = Video::empty();
                unsafe {
                    if (&*gpu_frame.as_ptr()).hw_frames_ctx.is_null() {
                        cpu_frame = gpu_frame;
                    } else {
                        if ffi::av_hwframe_transfer_data(
                            cpu_frame.as_mut_ptr(),
                            gpu_frame.as_ptr(),
                            0,
                        ) < 0
                        {
                            panic!("av_hwframe_transfer_data failed");
                        }
                        /*TODO: MAP OR TRANSFER?
                        if ffi::av_hwframe_map(
                            cpu_frame.as_mut_ptr(),
                            gpu_frame.as_ptr(),
                            ffi::AV_HWFRAME_MAP_READ as i32,
                        ) < 0
                        {
                            panic!("av_hwframe_map failed");
                        }
                        */
                    }
                }
                cpu_frame.set_pts(pts);
                cpu_frame_tx.send((cpu_frame, sync_time_opt)).unwrap();

                let duration = start.elapsed();
                log::debug!("map gpu video frame to cpu at {:?} in {:?}", pts, duration);
            }
        })?;

    // Sync channel to prevent getting too far behind
    let (video_packet_tx, video_packet_rx) = mpsc::sync_channel::<(Packet, Option<Instant>)>(2);
    thread::Builder::new()
        .name("video_decode".to_string())
        .spawn(move || {
            let mut eof = false;
            while !eof {
                let mut sync_time_opt = None;

                {
                    let packet_res = video_packet_rx.recv();

                    // Start timer after blocking recv
                    let start = Instant::now();

                    let mut packet_pts = None;
                    match packet_res {
                        Ok((packet, time_opt)) => {
                            packet_pts = packet.pts();
                            sync_time_opt = time_opt;
                            video_decoder.send_packet(&packet).unwrap();
                        }
                        Err(_err) => {
                            video_decoder.send_eof().unwrap();
                            eof = true;
                        }
                    }

                    let duration = start.elapsed();
                    log::debug!("sent packet at {:?} in {:?}", packet_pts, duration);
                }

                let start = Instant::now();

                let mut pts = None;
                let mut video_frames = 0;
                loop {
                    let mut gpu_frame = Video::empty();
                    if video_decoder.receive_frame(&mut gpu_frame).is_ok() {
                        pts = gpu_frame.pts();
                        gpu_frame_tx.send((gpu_frame, sync_time_opt)).unwrap();
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
        })?;

    let audio_stream = ictx
        .streams()
        .best(Type::Audio)
        .ok_or(ffmpeg::Error::StreamNotFound)?;
    let audio_stream_index = audio_stream.index();
    let audio_time_base = f64::from(audio_stream.time_base());

    let audio_context_decoder =
        codec::context::Context::from_parameters(audio_stream.parameters())?;
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

    let min_sleep = Duration::from_millis(1);
    let min_skip = Duration::from_millis(1);
    let mut receive_and_process_decoded_audio_frames = |decoder: &mut ffmpeg::decoder::Audio,
                                                        sync_time_opt: &mut Option<Instant>|
     -> Result<(), ffmpeg::Error> {
        let mut decoded = Audio::empty();
        let mut resampled = Audio::empty();
        let mut pts_opt = None;
        while decoder.receive_frame(&mut decoded).is_ok() {
            pts_opt = decoded.pts();

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
                    audio_queue.data.extend(plane);
                }
            }
        }

        if let Some(pts) = pts_opt {
            let expected_float = pts as f64 * audio_time_base;
            let expected = Duration::from_secs_f64(expected_float);
            if let Some(sync_time) = &sync_time_opt {
                // Sync with audio
                let actual = sync_time.elapsed();
                if expected > actual {
                    let sleep = expected - actual;
                    if sleep > min_sleep {
                        // We leave min_sleep of buffer room
                        log::debug!("audio ahead {:?}", sleep);
                    }
                } else {
                    let skip = actual - expected;
                    if skip > min_skip {
                        //TODO: handle frame skipping
                        log::debug!("audio behind {:?}", skip);
                    }
                }
            } else {
                // Set up sync
                *sync_time_opt = Some(Instant::now() - expected);
            }
        }
        Ok(())
    };

    //TODO: dynamically choose this
    let buffer_duration = Duration::from_millis(250);

    // Start CPAL stream
    cpal_stream.play()?;

    let mut sync_time_opt = None;
    let mut seconds_opt = None;
    loop {
        let mut packet = Packet::empty();
        match packet.read(&mut ictx) {
            Ok(()) => {
                if packet.stream() == video_stream_index {
                    video_packet_tx.send((packet, sync_time_opt)).unwrap();
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

        let (audio_queue_duration, audio_queue_delay) = {
            let audio_queue = audio_queue_lock.lock().unwrap();
            (audio_queue.duration(), audio_queue.delay)
        };

        let (video_queue_duration, video_queue_delay) = {
            let mut video_queue = video_queue_lock.lock().unwrap();
            let video_queue_duration = video_queue.duration();
            if video_queue_duration < buffer_duration {
                // If we do not have enough video queued, delay the video output
                video_queue.delay = buffer_duration - video_queue_duration;
            } else {
                video_queue.delay = Duration::default();
            }
            // Add audio queue delay to sync with audio
            video_queue.delay += audio_queue_delay;
            (video_queue_duration, video_queue.delay)
        };

        log::debug!(
            "video: {:?}, {:?} audio: {:?}, {:?}",
            video_queue_duration,
            video_queue_delay,
            audio_queue_duration,
            audio_queue_delay
        );

        let min_queue_duration = cmp::min(video_queue_duration, audio_queue_duration);
        if min_queue_duration > buffer_duration {
            // If we have enough queued, we can sleep
            let sleep = min_queue_duration - buffer_duration;
            log::debug!("sleep {:?}", sleep);
            thread::sleep(sleep);
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

                        // Clear audio sync time
                        sync_time_opt = None;
                        // Clear audio and video queues
                        {
                            let mut audio_queue = audio_queue_lock.lock().unwrap();
                            audio_queue.data.clear();
                        }
                        {
                            //TODO: clear pending data stuck in channels
                            let mut video_queue = video_queue_lock.lock().unwrap();
                            video_queue.data.clear();
                        }
                    }
                }
            }
        }
    }

    audio_decoder.send_eof()?;
    receive_and_process_decoded_audio_frames(&mut audio_decoder, &mut sync_time_opt)?;

    Ok(())
}

pub fn run(path: PathBuf, config: Config) -> (mpsc::Sender<PlayerMessage>, Arc<Mutex<VideoQueue>>) {
    ffmpeg::init().unwrap();

    let (player_tx, player_rx) = mpsc::channel();
    let video_queue_lock = Arc::new(Mutex::new(VideoQueue::new()));
    {
        let video_queue_lock = video_queue_lock.clone();
        thread::Builder::new()
            .name("ffmpeg".to_string())
            .spawn(move || {
                ffmpeg_thread(path, player_rx, video_queue_lock, config).unwrap();
            })
            .unwrap();
    }

    (player_tx, video_queue_lock)
}
