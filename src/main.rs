// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use cosmic::{
    app::{message, Command, Core, Settings},
    cosmic_config::{self, CosmicConfigEntry},
    cosmic_theme, executor, font,
    iced::{
        event::{self, Event},
        keyboard::{Event as KeyEvent, Key, Modifiers},
        mouse::Event as MouseEvent,
        subscription::Subscription,
        window, Alignment, Background, Border, Color, ContentFit, Length, Limits,
    },
    theme,
    widget::{self, menu::action::MenuAction, Slider},
    Application, ApplicationExt, Element,
};
use iced_video_player::{
    gst::{self, prelude::*},
    gst_app, gst_pbutils, Video, VideoPlayer,
};
use std::{
    any::TypeId,
    collections::HashMap,
    ffi::{CStr, CString},
    fs, process, thread,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

use crate::{
    config::{Config, CONFIG_VERSION},
    key_bind::{key_binds, KeyBind},
};

mod config;
mod key_bind;
mod localize;
mod menu;
#[cfg(feature = "mpris-server")]
mod mpris;

static CONTROLS_TIMEOUT: Duration = Duration::new(2, 0);

const GST_PLAY_FLAG_VIDEO: i32 = 1 << 0;
const GST_PLAY_FLAG_AUDIO: i32 = 1 << 1;
const GST_PLAY_FLAG_TEXT: i32 = 1 << 2;

fn language_name(code: &str) -> Option<String> {
    let code_c = CString::new(code).ok()?;
    let name_c = unsafe {
        //TODO: export this in gstreamer_tag
        let name_ptr = gstreamer_tag::ffi::gst_tag_get_language_name(code_c.as_ptr());
        if name_ptr.is_null() {
            return None;
        }
        CStr::from_ptr(name_ptr)
    };
    let name = name_c.to_str().ok()?;
    Some(name.to_string())
}

/// Runs application with these settings
#[rustfmt::skip]
pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    localize::localize();

    let (config_handler, config) = match cosmic_config::Config::new(App::APP_ID, CONFIG_VERSION) {
        Ok(config_handler) => {
            let config = match Config::get_entry(&config_handler) {
                Ok(ok) => ok,
                Err((errs, config)) => {
                    log::info!("errors loading config: {:?}", errs);
                    config
                }
            };
            (Some(config_handler), config)
        }
        Err(err) => {
            log::error!("failed to create config handler: {}", err);
            (None, Config::default())
        }
    };

    let mut settings = Settings::default();
    settings = settings.theme(config.app_theme.theme());
    settings = settings.size_limits(Limits::NONE.min_width(360.0).min_height(180.0));

    let url_opt = match std::env::args().nth(1) {
        Some(arg) => match url::Url::parse(&arg) {
            Ok(url) => Some(url),
            Err(_) => match fs::canonicalize(&arg) {
                Ok(path) => match url::Url::from_file_path(&path) {
                    Ok(url) => Some(url),
                    Err(()) => {
                        log::warn!("failed to parse path {:?}", path);
                        None
                    }
                },
                Err(err) => {
                    log::warn!("failed to parse argument {:?}: {}", arg, err);
                    None
                }
            },
        },
        None => None,
    };

    let flags = Flags {
        config_handler,
        config,
        url_opt,
    };
    cosmic::app::run::<App>(settings, flags)?;

    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    FileClose,
    FileOpen,
    Fullscreen,
    PlayPause,
    SeekBackward,
    SeekForward,
    WindowClose,
}

impl MenuAction for Action {
    type Message = Message;

    fn message(&self) -> Message {
        match self {
            Self::FileClose => Message::FileClose,
            Self::FileOpen => Message::FileOpen,
            Self::Fullscreen => Message::Fullscreen,
            Self::PlayPause => Message::PlayPause,
            Self::SeekBackward => Message::SeekRelative(-10.0),
            Self::SeekForward => Message::SeekRelative(10.0),
            Self::WindowClose => Message::WindowClose,
        }
    }
}

#[derive(Clone)]
pub struct Flags {
    config_handler: Option<cosmic_config::Config>,
    config: Config,
    url_opt: Option<url::Url>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DropdownKind {
    Audio,
    Subtitle,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MprisMeta {
    url_opt: Option<url::Url>,
    album: String,
    album_art_opt: Option<url::Url>,
    album_artist: String,
    artists: Vec<String>,
    title: String,
    disc_number: i32,
    track_number: i32,
    duration_micros: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MprisState {
    fullscreen: bool,
    position_micros: i64,
    paused: bool,
    volume: f64,
}

#[derive(Clone, Debug)]
pub enum MprisEvent {
    Meta(MprisMeta),
    State(MprisState),
}

/// Messages that are used specifically by our [`App`].
#[derive(Clone, Debug)]
pub enum Message {
    None,
    Config(Config),
    DropdownToggle(DropdownKind),
    FileClose,
    FileLoad(url::Url),
    FileOpen,
    Fullscreen,
    Key(Modifiers, Key),
    AudioCode(usize),
    AudioToggle,
    AudioVolume(f64),
    TextCode(usize),
    Pause,
    Play,
    PlayPause,
    Seek(f64),
    SeekRelative(f64),
    SeekRelease,
    EndOfStream,
    MissingPlugin(gst::Message),
    MprisChannel(MprisMeta, MprisState, mpsc::UnboundedSender<MprisEvent>),
    NewFrame,
    Reload,
    ShowControls,
    SystemThemeModeChange(cosmic_theme::ThemeMode),
    WindowClose,
}

/// The [`App`] stores application-specific state.
pub struct App {
    core: Core,
    flags: Flags,
    album_art_opt: Option<tempfile::NamedTempFile>,
    controls: bool,
    controls_time: Instant,
    dropdown_opt: Option<DropdownKind>,
    fullscreen: bool,
    key_binds: HashMap<KeyBind, Action>,
    mpris_opt: Option<(MprisMeta, MprisState, mpsc::UnboundedSender<MprisEvent>)>,
    video_opt: Option<Video>,
    position: f64,
    duration: f64,
    dragging: bool,
    audio_codes: Vec<String>,
    audio_tags: Vec<gst::TagList>,
    current_audio: i32,
    text_codes: Vec<String>,
    current_text: i32,
}

impl App {
    fn close(&mut self) {
        self.album_art_opt = None;
        //TODO: drop does not work well
        if let Some(mut video) = self.video_opt.take() {
            log::info!("pausing video");
            video.set_paused(true);
            log::info!("dropping video");
            drop(video);
            log::info!("dropped video");
        }
        self.position = 0.0;
        self.duration = 0.0;
        self.dragging = false;
        self.audio_codes.clear();
        self.audio_tags.clear();
        self.current_audio = -1;
        self.text_codes.clear();
        self.current_text = -1;
        self.update_mpris_meta();
    }

    fn load(&mut self) -> Command<Message> {
        self.close();

        let url = match &self.flags.url_opt {
            Some(some) => some,
            None => return Command::none(),
        };

        log::info!("Loading {}", url);

        //TODO: this code came from iced_video_player::Video::new and has been modified to stop the pipeline on error
        //TODO: remove unwraps and enable playback of files with only audio.
        let video = {
            gst::init().unwrap();

            let pipeline = format!(
                "playbin uri=\"{}\" video-sink=\"videoscale ! videoconvert ! appsink name=iced_video drop=true caps=video/x-raw,format=NV12,pixel-aspect-ratio=1/1\"",
                url.as_str()
            );
            let pipeline = gst::parse::launch(pipeline.as_ref())
                .unwrap()
                .downcast::<gst::Pipeline>()
                .map_err(|_| iced_video_player::Error::Cast)
                .unwrap();

            let video_sink: gst::Element = pipeline.property("video-sink");
            let pad = video_sink.pads().first().cloned().unwrap();
            let pad = pad.dynamic_cast::<gst::GhostPad>().unwrap();
            let bin = pad
                .parent_element()
                .unwrap()
                .downcast::<gst::Bin>()
                .unwrap();
            let video_sink = bin.by_name("iced_video").unwrap();
            let video_sink = video_sink.downcast::<gst_app::AppSink>().unwrap();

            match Video::from_gst_pipeline(pipeline.clone(), video_sink, None) {
                Ok(ok) => ok,
                Err(err) => {
                    log::warn!("failed to open {}: {err}", url);
                    pipeline.set_state(gst::State::Null).unwrap();
                    return Command::none();
                }
            }
        };

        self.duration = video.duration().as_secs_f64();
        let pipeline = video.pipeline();
        self.video_opt = Some(video);

        let n_video = pipeline.property::<i32>("n-video");
        for i in 0..n_video {
            let tags: gst::TagList = pipeline.emit_by_name("get-video-tags", &[&i]);
            log::info!("video stream {i}: {tags:#?}");
        }

        let n_audio = pipeline.property::<i32>("n-audio");
        self.audio_codes = Vec::with_capacity(n_audio as usize);
        for i in 0..n_audio {
            let tags: gst::TagList = pipeline.emit_by_name("get-audio-tags", &[&i]);
            log::info!("audio stream {i}: {tags:#?}");
            self.audio_codes
                .push(if let Some(title) = tags.get::<gst::tags::Title>() {
                    title.get().to_string()
                } else if let Some(language_code) = tags.get::<gst::tags::LanguageCode>() {
                    let language_code = language_code.get();
                    language_name(language_code).unwrap_or_else(|| language_code.to_string())
                } else {
                    format!("Audio #{i}")
                });
            self.audio_tags.push(tags);
        }
        self.current_audio = pipeline.property::<i32>("current-audio");

        let n_text = pipeline.property::<i32>("n-text");
        self.text_codes = Vec::with_capacity(n_text as usize);
        for i in 0..n_text {
            let tags: gst::TagList = pipeline.emit_by_name("get-text-tags", &[&i]);
            log::info!("text stream {i}: {tags:#?}");
            self.text_codes
                .push(if let Some(title) = tags.get::<gst::tags::Title>() {
                    title.get().to_string()
                } else if let Some(language_code) = tags.get::<gst::tags::LanguageCode>() {
                    let language_code = language_code.get();
                    language_name(language_code).unwrap_or_else(|| language_code.to_string())
                } else {
                    format!("Subtitle #{i}")
                });
        }
        self.current_text = pipeline.property::<i32>("current-text");

        //TODO: Flags can be used to enable/disable subtitles
        let flags_value = pipeline.property_value("flags");
        println!("original flags {:?}", flags_value);
        match flags_value.transform::<i32>() {
            Ok(flags_transform) => match flags_transform.get::<i32>() {
                Ok(mut flags) => {
                    flags |= GST_PLAY_FLAG_VIDEO | GST_PLAY_FLAG_AUDIO | GST_PLAY_FLAG_TEXT;
                    match gst::glib::Value::from(flags).transform_with_type(flags_value.type_()) {
                        Ok(value) => pipeline.set_property("flags", value),
                        Err(err) => {
                            log::warn!("failed to transform int to flags: {err}");
                        }
                    }
                }
                Err(err) => {
                    log::warn!("failed to get flags as int: {err}");
                }
            },
            Err(err) => {
                log::warn!("failed to transform flags to int: {err}");
            }
        }
        println!("updated flags {:?}", pipeline.property_value("flags"));

        self.update_mpris_meta();
        self.update_title()
    }

    fn update_controls(&mut self, in_use: bool) {
        if in_use
            || !self
                .video_opt
                .as_ref()
                .map_or(false, |video| video.has_video())
        {
            self.controls = true;
            self.controls_time = Instant::now();
        } else if self.controls && self.controls_time.elapsed() > CONTROLS_TIMEOUT {
            self.controls = false;
        }
        self.update_mpris_state();
    }

    fn update_config(&mut self) -> Command<Message> {
        cosmic::app::command::set_theme(self.flags.config.app_theme.theme())
    }

    fn update_mpris_meta(&mut self) {
        if let Some((old, _, tx)) = &mut self.mpris_opt {
            let mut new = MprisMeta {
                //TODO: clear url_opt when file is closed
                url_opt: self.flags.url_opt.clone(),
                duration_micros: (self.duration * 1_000_000.0) as i64,
                ..Default::default()
            };
            //TODO: use any other stream tags?
            if let Some(tags) = self.audio_tags.get(0) {
                log::info!("{:#?}", tags);
                if let Some(tag) = tags.get::<gst::tags::Album>() {
                    new.album = tag.get().into();
                }
                if let Some(tag) = tags.get::<gst::tags::AlbumArtist>() {
                    new.album_artist = tag.get().into();
                }
                if let Some(tag) = tags.get::<gst::tags::Artist>() {
                    //TODO: how are multiple artists handled by gstreamer?
                    new.artists = vec![tag.get().into()];
                }
                if let Some(tag) = tags.get::<gst::tags::Title>() {
                    new.title = tag.get().into();
                }
                /*TODO: no gstreamer tag
                if let Some(tag) = tags.get::<gst::tags::DiscNumber>() {
                    new.disc_number = tag.get();
                }
                */
                if let Some(tag) = tags.get::<gst::tags::TrackNumber>() {
                    new.track_number = tag.get() as i32;
                }
                if self.album_art_opt.is_none() {
                    //TODO: run in thread or async to avoid blocking UI?
                    if let Some(tag) = tags.get::<gst::tags::Image>() {
                        let sample = tag.get();
                        if let Some(buffer) = sample.buffer() {
                            match buffer.map_readable() {
                                //TODO: use original format instead of converting to PNG?
                                Ok(buffer_map) => match image::load_from_memory(&buffer_map) {
                                    Ok(image) => {
                                        match tempfile::Builder::new()
                                            .prefix(&format!("cosmic-player.pid{}.", process::id()))
                                            .suffix(".png")
                                            .tempfile()
                                        {
                                            Ok(mut album_art) => {
                                                match image.write_with_encoder(
                                                    image::codecs::png::PngEncoder::new(
                                                        &mut album_art,
                                                    ),
                                                ) {
                                                    Ok(()) => self.album_art_opt = Some(album_art),
                                                    Err(err) => {
                                                        log::warn!(
                                                            "failed to write temporary image: {}",
                                                            err
                                                        );
                                                    }
                                                }
                                            }
                                            Err(err) => {
                                                log::warn!(
                                                    "failed to create temporary image: {}",
                                                    err
                                                );
                                            }
                                        }
                                    }
                                    Err(err) => {
                                        log::warn!("failed to load image from memory: {}", err);
                                    }
                                },
                                Err(err) => {
                                    log::warn!("failed to map image buffer: {}", err);
                                }
                            }
                        }
                    }
                }
                if let Some(album_art) = &self.album_art_opt {
                    new.album_art_opt = url::Url::from_file_path(album_art.path()).ok();
                }
            }
            if new != *old {
                *old = new.clone();
                let _ = tx.send(MprisEvent::Meta(new));
            }
        }
    }

    fn update_mpris_state(&mut self) {
        if let Some((_, old, tx)) = &mut self.mpris_opt {
            let mut new = MprisState {
                fullscreen: self.fullscreen,
                position_micros: (self.position * 1_000_000.0) as i64,
                paused: true,
                volume: 0.0,
            };
            if let Some(video) = &self.video_opt {
                new.paused = video.paused();
                new.volume = video.volume();
            }
            if new != *old {
                *old = new.clone();
                let _ = tx.send(MprisEvent::State(new));
            }
        }
    }

    fn update_title(&mut self) -> Command<Message> {
        //TODO: filename?
        let title = "COSMIC Media Player";
        self.set_window_title(title.to_string())
    }
}

/// Implement [`cosmic::Application`] to integrate with COSMIC.
impl Application for App {
    /// Default async executor to use with the app.
    type Executor = executor::Default;

    /// Argument received [`cosmic::Application::new`].
    type Flags = Flags;

    /// Message type specific to our [`App`].
    type Message = Message;

    /// The unique application ID to supply to the window manager.
    const APP_ID: &'static str = "com.system76.CosmicPlayer";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    /// Creates the application, and optionally emits command on initialize.
    fn init(mut core: Core, flags: Self::Flags) -> (Self, Command<Self::Message>) {
        core.window.content_container = false;

        let mut app = App {
            core,
            flags,
            album_art_opt: None,
            controls: true,
            controls_time: Instant::now(),
            dropdown_opt: None,
            fullscreen: false,
            key_binds: key_binds(),
            mpris_opt: None,
            video_opt: None,
            position: 0.0,
            duration: 0.0,
            dragging: false,
            audio_codes: Vec::new(),
            audio_tags: Vec::new(),
            current_audio: -1,
            text_codes: Vec::new(),
            current_text: -1,
        };

        let command = app.load();
        (app, command)
    }

    fn on_escape(&mut self) -> Command<Self::Message> {
        if self.fullscreen {
            return self.update(Message::Fullscreen);
        } else {
            Command::none()
        }
    }

    /// Handle application events here.
    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            Message::None => {}
            Message::Config(config) => {
                if config != self.flags.config {
                    log::info!("update config");
                    self.flags.config = config;
                    return self.update_config();
                }
            }
            Message::DropdownToggle(menu_kind) => {
                if self.dropdown_opt.take() != Some(menu_kind) {
                    self.dropdown_opt = Some(menu_kind);
                }
            }
            Message::FileClose => {
                self.close();
            }
            Message::FileLoad(url) => {
                self.flags.url_opt = Some(url);
                return self.load();
            }
            Message::FileOpen => {
                //TODO: embed cosmic-files dialog (after libcosmic rebase works)
                #[cfg(feature = "xdg-portal")]
                return Command::perform(
                    async move {
                        let dialog = cosmic::dialog::file_chooser::open::Dialog::new()
                            .title(fl!("open-media"));
                        match dialog.open_file().await {
                            Ok(response) => {
                                message::app(Message::FileLoad(response.url().to_owned()))
                            }
                            Err(err) => {
                                log::warn!("failed to open file: {}", err);
                                message::none()
                            }
                        }
                    },
                    |x| x,
                );
            }
            Message::Fullscreen => {
                //TODO: cleanest way to close dropdowns
                self.dropdown_opt = None;

                self.fullscreen = !self.fullscreen;
                self.core.window.show_headerbar = !self.fullscreen;
                return window::change_mode(
                    window::Id::MAIN,
                    if self.fullscreen {
                        window::Mode::Fullscreen
                    } else {
                        window::Mode::Windowed
                    },
                );
            }
            Message::Key(modifiers, key) => {
                for (key_bind, action) in self.key_binds.iter() {
                    if key_bind.matches(modifiers, &key) {
                        return self.update(action.message());
                    }
                }
            }
            Message::AudioCode(code) => {
                if let Ok(code) = i32::try_from(code) {
                    if let Some(video) = &self.video_opt {
                        let pipeline = video.pipeline();
                        pipeline.set_property("current-audio", code);
                        self.current_audio = pipeline.property("current-audio");
                    }
                }
            }
            Message::AudioToggle => {
                if let Some(video) = &mut self.video_opt {
                    video.set_muted(!video.muted());
                    self.update_controls(true);
                }
            }
            Message::AudioVolume(volume) => {
                if let Some(video) = &mut self.video_opt {
                    if volume >= 0.0 && volume <= 1.0 {
                        video.set_volume(volume);
                        self.update_controls(true);
                    }
                }
            }
            Message::TextCode(code) => {
                if let Ok(code) = i32::try_from(code) {
                    if let Some(video) = &self.video_opt {
                        let pipeline = video.pipeline();
                        pipeline.set_property("current-text", code);
                        self.current_text = pipeline.property("current-text");
                    }
                }
            }
            Message::Pause | Message::Play | Message::PlayPause => {
                //TODO: cleanest way to close dropdowns
                self.dropdown_opt = None;

                if let Some(video) = &mut self.video_opt {
                    video.set_paused(match message {
                        Message::Play => false,
                        Message::Pause => true,
                        _ => !video.paused(),
                    });
                    self.update_controls(true);
                }
            }
            Message::Seek(secs) => {
                //TODO: cleanest way to close dropdowns
                self.dropdown_opt = None;

                if let Some(video) = &mut self.video_opt {
                    self.dragging = true;
                    self.position = secs;
                    video.set_paused(true);
                    let duration = Duration::try_from_secs_f64(self.position).unwrap_or_default();
                    video.seek(duration, true).expect("seek");
                    self.update_controls(true);
                }
            }
            Message::SeekRelative(secs) => {
                if let Some(video) = &mut self.video_opt {
                    self.position = video.position().as_secs_f64();
                    let duration =
                        Duration::try_from_secs_f64(self.position + secs).unwrap_or_default();
                    video.seek(duration, true).expect("seek");
                }
            }
            Message::SeekRelease => {
                //TODO: cleanest way to close dropdowns
                self.dropdown_opt = None;

                if let Some(video) = &mut self.video_opt {
                    self.dragging = false;
                    let duration = Duration::try_from_secs_f64(self.position).unwrap_or_default();
                    video.seek(duration, true).expect("seek");
                    video.set_paused(false);
                    self.update_controls(true);
                }
            }
            Message::EndOfStream => {
                println!("end of stream");
            }
            Message::MissingPlugin(element) => {
                if let Some(video) = &mut self.video_opt {
                    video.set_paused(true);
                }
                return Command::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            match gst_pbutils::MissingPluginMessage::parse(&element) {
                                Ok(missing_plugin) => {
                                    let mut install_ctx = gst_pbutils::InstallPluginsContext::new();
                                    install_ctx
                                        .set_desktop_id(&format!("{}.desktop", Self::APP_ID));
                                    let install_detail = missing_plugin.installer_detail();
                                    loop {
                                        // Wait for any prior installations to finish
                                        while gst_pbutils::missing_plugins::install_plugins_installation_in_progress() {
                                            thread::sleep(Duration::from_millis(250));
                                        }

                                        println!("installing plugins: {}", install_detail);
                                        let status = gst_pbutils::missing_plugins::install_plugins_sync(
                                            &[&install_detail],
                                            Some(&install_ctx),
                                        );
                                        //TODO: why does the sync function return with install-in-progress?
                                        log::info!("plugin install status: {}", status);

                                        match status {
                                            gst_pbutils::InstallPluginsReturn::InstallInProgress => {
                                                // Try again until completed
                                                continue;
                                            },
                                            gst_pbutils::InstallPluginsReturn::Success => {
                                                // Update registry and reload video
                                                log::info!(
                                                    "gstreamer registry update: {:?}",
                                                    gst::Registry::update()
                                                );
                                                return message::app(Message::Reload);
                                            },
                                            _ => {
                                                log::warn!("failed to install plugins: {status}");
                                                break;
                                            }
                                        }
                                    }

                                }
                                Err(err) => {
                                    log::warn!("failed to parse missing plugin message: {err}");
                                }
                            }
                            message::none()
                        })
                        .await
                        .unwrap()
                    },
                    |x| x,
                );
            }
            Message::MprisChannel(meta, state, tx) => {
                self.mpris_opt = Some((meta, state, tx));
                self.update_mpris_meta();
                self.update_mpris_state();
            }
            Message::NewFrame => {
                if let Some(video) = &self.video_opt {
                    if !self.dragging {
                        self.position = video.position().as_secs_f64();
                        self.update_controls(self.dropdown_opt.is_some());
                    }
                }
            }
            Message::Reload => {
                return self.load();
            }
            Message::ShowControls => {
                self.update_controls(true);
            }
            Message::SystemThemeModeChange(_theme_mode) => {
                return self.update_config();
            }
            Message::WindowClose => {
                process::exit(0);
            }
        }
        Command::none()
    }

    fn header_start(&self) -> Vec<Element<Self::Message>> {
        vec![menu::menu_bar(&self.flags.config, &self.key_binds)]
    }

    /// Creates a view after each update.
    fn view(&self) -> Element<Self::Message> {
        let cosmic_theme::Spacing {
            space_xxs,
            space_xs,
            space_m,
            ..
        } = theme::active().cosmic().spacing;

        let format_time = |time_float: f64| -> String {
            let time = time_float.floor() as i64;
            let seconds = time % 60;
            let minutes = (time / 60) % 60;
            let hours = (time / 60) / 60;
            format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
        };

        let Some(video) = &self.video_opt else {
            //TODO: use space variables
            let column = widget::column::with_capacity(4)
                .align_items(Alignment::Center)
                .spacing(24)
                .width(Length::Fill)
                .height(Length::Fill)
                .push(widget::vertical_space(Length::Fill))
                .push(
                    widget::column::with_capacity(2)
                        .align_items(Alignment::Center)
                        .spacing(8)
                        .push(widget::icon::from_name("folder-symbolic").size(64))
                        .push(widget::text::body(fl!("no-video-or-audio-file-open"))),
                )
                .push(widget::button::suggested(fl!("open-file")).on_press(Message::FileOpen))
                .push(widget::vertical_space(Length::Fill));

            return widget::container(column)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(theme::Container::WindowBackground)
                .into();
        };

        let muted = video.muted();
        let volume = video.volume();

        let mut video_player: Element<_> = VideoPlayer::new(video)
            .mouse_hidden(!self.controls)
            .on_end_of_stream(Message::EndOfStream)
            .on_missing_plugin(Message::MissingPlugin)
            .on_new_frame(Message::NewFrame)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        if let Some(album_art) = &self.album_art_opt {
            if !video.has_video() {
                // This is a hack to have the video player running but not visible (since the controls will cover it as an overlay)
                video_player = widget::column::with_children(vec![
                    widget::image(widget::image::Handle::from_path(album_art.path()))
                        .content_fit(ContentFit::ScaleDown)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into(),
                    widget::container(video_player).height(space_m).into(),
                ])
                .into();
            }
        }

        let mouse_area = widget::mouse_area(video_player)
            .on_press(Message::PlayPause)
            .on_double_press(Message::Fullscreen);

        let mut popover = widget::popover(mouse_area).position(widget::popover::Position::Bottom);
        let mut popup_items = Vec::<Element<_>>::with_capacity(3);
        if let Some(dropdown) = self.dropdown_opt {
            let mut items = Vec::<Element<_>>::new();
            match dropdown {
                DropdownKind::Audio => {
                    items.push(
                        widget::row::with_children(vec![
                            widget::button::icon(
                                widget::icon::from_name({
                                    if muted {
                                        "audio-volume-muted-symbolic"
                                    } else {
                                        if volume >= (2.0 / 3.0) {
                                            "audio-volume-high-symbolic"
                                        } else if volume >= (1.0 / 3.0) {
                                            "audio-volume-medium-symbolic"
                                        } else {
                                            "audio-volume-low-symbolic"
                                        }
                                    }
                                })
                                .size(16),
                            )
                            .on_press(Message::AudioToggle)
                            .into(),
                            //TODO: disable slider when muted?
                            Slider::new(0.0..=1.0, volume, Message::AudioVolume)
                                .step(0.01)
                                .into(),
                        ])
                        .align_items(Alignment::Center)
                        .into(),
                    );
                }
                DropdownKind::Subtitle => {
                    if !self.audio_codes.is_empty() {
                        items.push(widget::text::heading(fl!("audio")).into());
                        items.push(
                            widget::dropdown(
                                &self.audio_codes,
                                usize::try_from(self.current_audio).ok(),
                                Message::AudioCode,
                            )
                            .into(),
                        );
                    }
                    if !self.text_codes.is_empty() {
                        //TODO: allow toggling subtitles
                        items.push(widget::text::heading(fl!("subtitles")).into());
                        items.push(
                            widget::dropdown(
                                &self.text_codes,
                                usize::try_from(self.current_text).ok(),
                                Message::TextCode,
                            )
                            .into(),
                        );
                    }
                }
            }

            let mut column = widget::column::with_capacity(items.len());
            for item in items {
                column = column.push(widget::container(item).padding([space_xxs, space_m]));
            }

            popup_items.push(
                widget::row::with_children(vec![
                    widget::horizontal_space(Length::Fill).into(),
                    widget::container(column)
                        .padding(1)
                        //TODO: move style to libcosmic
                        .style(theme::Container::custom(|theme| {
                            let cosmic = theme.cosmic();
                            let component = &cosmic.background.component;
                            widget::container::Appearance {
                                icon_color: Some(component.on.into()),
                                text_color: Some(component.on.into()),
                                background: Some(Background::Color(component.base.into())),
                                border: Border {
                                    radius: 8.0.into(),
                                    width: 1.0,
                                    color: component.divider.into(),
                                },
                                ..Default::default()
                            }
                        }))
                        .width(Length::Fixed(240.0))
                        .into(),
                ])
                .into(),
            );
        }
        if self.controls {
            let mut row = widget::row::with_capacity(7)
                .align_items(Alignment::Center)
                .spacing(space_xxs)
                .push(
                    widget::button::icon(
                        if self.video_opt.as_ref().map_or(true, |video| video.paused()) {
                            widget::icon::from_name("media-playback-start-symbolic").size(16)
                        } else {
                            widget::icon::from_name("media-playback-pause-symbolic").size(16)
                        },
                    )
                    .on_press(Message::PlayPause),
                );
            if self.core.is_condensed() {
                row = row.push(widget::horizontal_space(Length::Fill));
            } else {
                row = row
                    .push(widget::text(format_time(self.position)).font(font::mono()))
                    .push(
                        Slider::new(0.0..=self.duration, self.position, Message::Seek)
                            .step(0.1)
                            .on_release(Message::SeekRelease),
                    )
                    .push(
                        widget::text(format_time(self.duration - self.position)).font(font::mono()),
                    );
            }
            row = row
                .push(
                    widget::button::icon(
                        widget::icon::from_name("media-view-subtitles-symbolic").size(16),
                    )
                    .on_press(Message::DropdownToggle(DropdownKind::Subtitle)),
                )
                .push(
                    widget::button::icon(
                        widget::icon::from_name("view-fullscreen-symbolic").size(16),
                    )
                    .on_press(Message::Fullscreen),
                )
                .push(
                    //TODO: scroll up/down on icon to change volume
                    widget::button::icon(
                        widget::icon::from_name({
                            if muted {
                                "audio-volume-muted-symbolic"
                            } else {
                                if volume >= (2.0 / 3.0) {
                                    "audio-volume-high-symbolic"
                                } else if volume >= (1.0 / 3.0) {
                                    "audio-volume-medium-symbolic"
                                } else {
                                    "audio-volume-low-symbolic"
                                }
                            }
                        })
                        .size(16),
                    )
                    .on_press(Message::DropdownToggle(DropdownKind::Audio)),
                );
            popup_items.push(
                widget::container(row)
                    .padding([space_xxs, space_xs])
                    .style(theme::Container::WindowBackground)
                    .into(),
            );

            if self.core.is_condensed() {
                popup_items.push(
                    widget::container(
                        widget::row::with_capacity(3)
                            .align_items(Alignment::Center)
                            .spacing(space_xxs)
                            .push(widget::text(format_time(self.position)).font(font::mono()))
                            .push(
                                Slider::new(0.0..=self.duration, self.position, Message::Seek)
                                    .step(0.1)
                                    .on_release(Message::SeekRelease),
                            )
                            .push(
                                widget::text(format_time(self.duration - self.position))
                                    .font(font::mono()),
                            ),
                    )
                    .padding([space_xxs, space_xs])
                    .style(theme::Container::WindowBackground)
                    .into(),
                );
            }
        }
        if !popup_items.is_empty() {
            popover = popover.popup(widget::column::with_children(popup_items));
        }

        widget::container(popover)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::Container::Custom(Box::new(|_theme| {
                widget::container::Appearance::default().with_background(Color::BLACK)
            })))
            .into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        struct ConfigSubscription;
        struct ThemeSubscription;

        let mut subscriptions = vec![
            event::listen_with(|event, _status| match event {
                Event::Keyboard(KeyEvent::KeyPressed { key, modifiers, .. }) => {
                    Some(Message::Key(modifiers, key))
                }
                Event::Mouse(MouseEvent::CursorMoved { .. }) => Some(Message::ShowControls),
                _ => None,
            }),
            cosmic_config::config_subscription(
                TypeId::of::<ConfigSubscription>(),
                Self::APP_ID.into(),
                CONFIG_VERSION,
            )
            .map(|update| {
                if !update.errors.is_empty() {
                    log::debug!("errors loading config: {:?}", update.errors);
                }
                Message::SystemThemeModeChange(update.config)
            }),
            cosmic_config::config_subscription::<_, cosmic_theme::ThemeMode>(
                TypeId::of::<ThemeSubscription>(),
                cosmic_theme::THEME_MODE_ID.into(),
                cosmic_theme::ThemeMode::version(),
            )
            .map(|update| {
                if !update.errors.is_empty() {
                    log::debug!("errors loading theme mode: {:?}", update.errors);
                }
                Message::SystemThemeModeChange(update.config)
            }),
        ];

        #[cfg(feature = "mpris-server")]
        {
            subscriptions.push(mpris::subscription());
        }

        Subscription::batch(subscriptions)
    }
}
