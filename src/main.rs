// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use cosmic::{
    app::{message, Core, Settings, Task},
    cosmic_config::{self, CosmicConfigEntry},
    cosmic_theme, executor, font,
    iced::{
        event::{self, Event},
        keyboard::{Event as KeyEvent, Key, Modifiers},
        mouse::Event as MouseEvent,
        window, Alignment, Color, Length, Limits, Subscription,
    },
    theme,
    widget::{self, Slider},
    Application, ApplicationExt, Element,
};
use iced_video_player::{
    gst::{self, prelude::*},
    gst_pbutils, Video, VideoPlayer,
};
use std::{
    any::TypeId,
    collections::HashMap,
    fs,
    time::{Duration, Instant},
};

use crate::{
    config::{Config, CONFIG_VERSION},
    key_bind::{key_binds, KeyBind},
};

mod config;
mod key_bind;
mod localize;

static CONTROLS_TIMEOUT: Duration = Duration::new(2, 0);

const GST_PLAY_FLAG_VIDEO: i32 = 1 << 0;
const GST_PLAY_FLAG_AUDIO: i32 = 1 << 1;
const GST_PLAY_FLAG_TEXT: i32 = 1 << 2;

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
                        log::warn!("failed to parse argument {:?}", arg);
                        None
                    }
                },
                Err(_) => {
                    log::warn!("failed to parse argument {:?}", arg);
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
    Fullscreen,
    PlayPause,
    SeekBackward,
    SeekForward,
}

impl Action {
    pub fn message(&self) -> Message {
        match self {
            Self::Fullscreen => Message::Fullscreen,
            Self::PlayPause => Message::PlayPause,
            Self::SeekBackward => Message::SeekRelative(-10.0),
            Self::SeekForward => Message::SeekRelative(10.0),
        }
    }
}

#[derive(Clone)]
pub struct Flags {
    config_handler: Option<cosmic_config::Config>,
    config: Config,
    url_opt: Option<url::Url>,
}

/// Messages that are used specifically by our [`App`].
#[derive(Clone, Debug)]
pub enum Message {
    Config(Config),
    Fullscreen,
    Key(Modifiers, Key),
    AudioCode(usize),
    TextCode(usize),
    PlayPause,
    Seek(f64),
    SeekRelative(f64),
    SeekRelease,
    EndOfStream,
    MissingPlugin(gst::Message),
    NewFrame,
    Reload,
    ShowControls,
    SystemThemeModeChange(cosmic_theme::ThemeMode),
}

/// The [`App`] stores application-specific state.
pub struct App {
    core: Core,
    flags: Flags,
    controls: bool,
    controls_time: Instant,
    fullscreen: bool,
    key_binds: HashMap<KeyBind, Action>,
    video_opt: Option<Video>,
    position: f64,
    duration: f64,
    dragging: bool,
    audio_codes: Vec<String>,
    current_audio: i32,
    text_codes: Vec<String>,
    current_text: i32,
}

impl App {
    fn close(&mut self) {
        self.video_opt = None;
        self.position = 0.0;
        self.duration = 0.0;
        self.dragging = false;
        self.audio_codes = Vec::new();
        self.current_audio = -1;
        self.text_codes = Vec::new();
        self.current_text = -1;
    }

    fn load(&mut self) -> Task<Message> {
        self.close();

        let url = match &self.flags.url_opt {
            Some(some) => some,
            None => return Task::none(),
        };

        let video = match Video::new(&url) {
            Ok(ok) => ok,
            Err(err) => {
                log::warn!("failed to open {:?}: {err}", url);
                return Task::none();
            }
        };
        self.duration = video.duration().as_secs_f64();
        let pipeline = video.pipeline();
        self.video_opt = Some(video);

        let n_audio = pipeline.property::<i32>("n-audio");
        self.audio_codes = Vec::with_capacity(n_audio as usize);
        for i in 0..n_audio {
            let tags: gst::TagList = pipeline.emit_by_name("get-audio-tags", &[&i]);
            log::info!("audio stream {i}: {tags:?}");
            self.audio_codes.push(
                if let Some(language_code) = tags.get::<gst::tags::LanguageCode>() {
                    language_code.get().to_string()
                } else {
                    format!("Audio #{i}")
                },
            );
        }
        self.current_audio = pipeline.property::<i32>("current-audio");

        let n_text = pipeline.property::<i32>("n-text");
        self.text_codes = Vec::with_capacity(n_text as usize);
        for i in 0..n_text {
            let tags: gst::TagList = pipeline.emit_by_name("get-text-tags", &[&i]);
            log::info!("text stream {i}: {tags:?}");
            self.text_codes.push(
                if let Some(language_code) = tags.get::<gst::tags::LanguageCode>() {
                    language_code.get().to_string()
                } else {
                    format!("Subtitle #{i}")
                },
            );
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

        self.update_title()
    }

    fn update_controls(&mut self, in_use: bool) {
        if in_use {
            self.controls = true;
            self.controls_time = Instant::now();
        } else if self.controls && self.controls_time.elapsed() > CONTROLS_TIMEOUT {
            self.controls = false;
        }
    }

    fn update_config(&mut self) -> Task<Message> {
        cosmic::app::command::set_theme(self.flags.config.app_theme.theme())
    }

    fn update_title(&mut self) -> Task<Message> {
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
    fn init(mut core: Core, flags: Self::Flags) -> (Self, Task<Self::Message>) {
        core.window.content_container = false;

        let mut app = App {
            core,
            flags,
            controls: true,
            controls_time: Instant::now(),
            fullscreen: false,
            key_binds: key_binds(),
            video_opt: None,
            position: 0.0,
            duration: 0.0,
            dragging: false,
            audio_codes: Vec::new(),
            current_audio: -1,
            text_codes: Vec::new(),
            current_text: -1,
        };

        let command = app.load();
        (app, command)
    }

    fn on_escape(&mut self) -> Task<Self::Message> {
        if self.fullscreen {
            return self.update(Message::Fullscreen);
        } else {
            Task::none()
        }
    }

    /// Handle application events here.
    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::Config(config) => {
                if config != self.flags.config {
                    log::info!("update config");
                    self.flags.config = config;
                    return self.update_config();
                }
            }
            Message::Fullscreen => {
                if let Some(window_id) = self.core.main_window_id() {
                    self.fullscreen = !self.fullscreen;
                    self.core.window.show_headerbar = !self.fullscreen;
                    return window::change_mode(
                        window_id,
                        if self.fullscreen {
                            window::Mode::Fullscreen
                        } else {
                            window::Mode::Windowed
                        },
                    );
                }
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
            Message::TextCode(code) => {
                if let Ok(code) = i32::try_from(code) {
                    if let Some(video) = &self.video_opt {
                        let pipeline = video.pipeline();
                        pipeline.set_property("current-text", code);
                        self.current_text = pipeline.property("current-text");
                    }
                }
            }
            Message::PlayPause => {
                if let Some(video) = &mut self.video_opt {
                    video.set_paused(!video.paused());
                    self.update_controls(true);
                }
            }
            Message::Seek(secs) => {
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
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            match gst_pbutils::MissingPluginMessage::parse(&element) {
                                Ok(missing_plugin) => {
                                    let mut install_ctx = gst_pbutils::InstallPluginsContext::new();
                                    install_ctx
                                        .set_desktop_id(&format!("{}.desktop", Self::APP_ID));
                                    let install_detail = missing_plugin.installer_detail();
                                    println!("installing plugins: {}", install_detail);
                                    let status = gst_pbutils::missing_plugins::install_plugins_sync(
                                        &[&install_detail],
                                        Some(&install_ctx),
                                    );
                                    log::info!("plugin install status: {}", status);
                                    log::info!(
                                        "gstreamer registry update: {:?}",
                                        gst::Registry::update()
                                    );
                                }
                                Err(err) => {
                                    log::warn!("failed to parse missing plugin message: {err}");
                                }
                            }
                            message::app(Message::Reload)
                        })
                        .await
                        .unwrap()
                    },
                    |x| x,
                );
            }
            Message::NewFrame => {
                if let Some(video) = &self.video_opt {
                    if !self.dragging {
                        self.position = video.position().as_secs_f64();
                        self.update_controls(false);
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
        }
        Task::none()
    }

    fn header_start(&self) -> Vec<Element<Self::Message>> {
        let mut row = widget::row::with_capacity(4)
            .align_y(Alignment::Center)
            .spacing(8);
        if !self.audio_codes.is_empty() {
            //TODO: allow mute/unmute/change volume
            row = row.push(widget::icon::from_name("audio-volume-high-symbolic").size(16));
            row = row.push(widget::dropdown(
                &self.audio_codes,
                usize::try_from(self.current_audio).ok(),
                Message::AudioCode,
            ));
        }
        if !self.text_codes.is_empty() {
            //TODO: allow toggling subtitles
            row = row.push(widget::icon::from_name("media-view-subtitles-symbolic").size(16));
            row = row.push(widget::dropdown(
                &self.text_codes,
                usize::try_from(self.current_text).ok(),
                Message::TextCode,
            ));
        }
        vec![row.into()]
    }

    /// Creates a view after each update.
    fn view(&self) -> Element<Self::Message> {
        let format_time = |time_float: f64| -> String {
            let time = time_float.floor() as i64;
            let seconds = time % 60;
            let minutes = (time / 60) % 60;
            let hours = (time / 60) / 60;
            format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
        };

        let Some(video) = &self.video_opt else {
            //TODO: open button if no video?
            return widget::container(widget::text("No video open"))
                .width(Length::Fill)
                .height(Length::Fill)
                .class(theme::Container::WindowBackground)
                .into();
        };

        let video_player = VideoPlayer::new(video)
            .mouse_hidden(!self.controls)
            .on_end_of_stream(Message::EndOfStream)
            .on_missing_plugin(Message::MissingPlugin)
            .on_new_frame(Message::NewFrame)
            .width(Length::Fill)
            .height(Length::Fill);

        let mouse_area = widget::mouse_area(video_player).on_double_press(Message::Fullscreen);

        let mut popover = widget::popover(mouse_area).position(widget::popover::Position::Bottom);
        if self.controls {
            popover = popover.popup(
                widget::container(
                    widget::row::with_capacity(5)
                        .align_y(Alignment::Center)
                        .spacing(8)
                        .padding([0, 8])
                        .push(
                            widget::button::icon(
                                if self.video_opt.as_ref().map_or(true, |video| video.paused()) {
                                    widget::icon::from_name("media-playback-start-symbolic")
                                        .size(16)
                                } else {
                                    widget::icon::from_name("media-playback-pause-symbolic")
                                        .size(16)
                                },
                            )
                            .on_press(Message::PlayPause),
                        )
                        .push(widget::text(format_time(self.position)).font(font::mono()))
                        .push(
                            Slider::new(0.0..=self.duration, self.position, Message::Seek)
                                .step(0.1)
                                .on_release(Message::SeekRelease),
                        )
                        .push(
                            widget::text(format_time(self.duration - self.position))
                                .font(font::mono()),
                        )
                        .push(
                            widget::button::icon(
                                widget::icon::from_name("view-fullscreen-symbolic").size(16),
                            )
                            .on_press(Message::Fullscreen),
                        ),
                )
                .class(theme::Container::WindowBackground),
            );
        }

        widget::container(popover)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme| widget::container::Style::default().background(Color::BLACK))
            .into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        struct ConfigSubscription;
        struct ThemeSubscription;

        Subscription::batch([
            event::listen_with(|event, _status, _window_id| match event {
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
        ])
    }
}
