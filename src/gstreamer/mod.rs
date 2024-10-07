// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use cosmic::{
    app::{message, Command, Core, Settings},
    cosmic_config::{self, CosmicConfigEntry},
    cosmic_theme, executor, font,
    iced::{
        event::{self, Event},
        keyboard::{Event as KeyEvent, Key, Modifiers},
        subscription::{self, Subscription},
        window, Alignment, Color, Length, Limits, Size,
    },
    theme,
    widget::{self, Slider},
    Application, ApplicationExt, Element,
};
use iced_video_player::{
    gst::{self, prelude::*},
    gst_pbutils, Video, VideoPlayer,
};
use std::{any::TypeId, collections::HashMap, time::Duration};

use crate::{
    config::{Config, CONFIG_VERSION},
    key_bind::{key_binds, KeyBind},
    localize,
};

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

            let url = url::Url::from_file_path(
                std::env::args().nth(1).unwrap()
            )
            .unwrap();
    let flags = Flags {
        config_handler,
        config,
        url,
    };
    cosmic::app::run::<App>(settings, flags)?;

    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    PlayPause,
    SeekBackward,
    SeekForward,
}

impl Action {
    pub fn message(&self) -> Message {
        match self {
            Self::PlayPause => Message::TogglePause,
            Self::SeekBackward => Message::SeekRelative(-10.0),
            Self::SeekForward => Message::SeekRelative(10.0),
        }
    }
}

#[derive(Clone)]
pub struct Flags {
    config_handler: Option<cosmic_config::Config>,
    config: Config,
    url: url::Url,
}

/// Messages that are used specifically by our [`App`].
#[derive(Clone, Debug)]
pub enum Message {
    Config(Config),
    Fullscreen,
    Key(Modifiers, Key),
    AudioCode(usize),
    TextCode(usize),
    TogglePause,
    ToggleLoop,
    Seek(f64),
    SeekRelative(f64),
    SeekRelease,
    EndOfStream,
    MissingPlugin(gst::Message),
    NewFrame,
    Reload,
    SystemThemeModeChange(cosmic_theme::ThemeMode),
}

/// The [`App`] stores application-specific state.
pub struct App {
    core: Core,
    flags: Flags,
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

    fn load(&mut self) -> Command<Message> {
        self.close();

        let video = match Video::new(&self.flags.url) {
            Ok(ok) => ok,
            Err(err) => {
                log::warn!("failed to open {:?}: {err}", self.flags.url);
                return Command::none();
            }
        };
        self.duration = video.duration().as_secs_f64();
        let pipeline = video.pipeline();
        self.video_opt = Some(video);

        let n_audio = pipeline.property::<i32>("n-audio");
        self.audio_codes = Vec::with_capacity(n_audio as usize);
        for i in 0..n_audio {
            let tags: gst::TagList = pipeline.emit_by_name("get-audio-tags", &[&i]);
            println!("audio stream {}: {:?}", i, tags);
            self.audio_codes.push(
                if let Some(language_code) = tags.get::<gst::tags::LanguageCode>() {
                    language_code.get().to_string()
                } else {
                    String::new()
                },
            );
        }
        self.current_audio = pipeline.property::<i32>("current-audio");

        let n_text = pipeline.property::<i32>("n-text");
        self.text_codes = Vec::with_capacity(n_text as usize);
        for i in 0..n_text {
            let tags: gst::TagList = pipeline.emit_by_name("get-text-tags", &[&i]);
            println!("text stream {}: {:?}", i, tags);
            self.text_codes.push(
                if let Some(language_code) = tags.get::<gst::tags::LanguageCode>() {
                    language_code.get().to_string()
                } else {
                    String::new()
                },
            );
        }
        self.current_text = pipeline.property::<i32>("current-text");

        //TODO: Flags can be used to enable/disable subtitles
        println!("flags {:?}", pipeline.property_value("flags"));

        self.update_title()
    }

    fn update_config(&mut self) -> Command<Message> {
        cosmic::app::command::set_theme(self.flags.config.app_theme.theme())
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
            Message::Config(config) => {
                if config != self.flags.config {
                    log::info!("update config");
                    self.flags.config = config;
                    return self.update_config();
                }
            }
            Message::Fullscreen => {
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
            Message::TextCode(code) => {
                if let Ok(code) = i32::try_from(code) {
                    if let Some(video) = &self.video_opt {
                        let pipeline = video.pipeline();
                        pipeline.set_property("current-text", code);
                        self.current_text = pipeline.property("current-text");
                    }
                }
            }
            Message::TogglePause => {
                if let Some(video) = &mut self.video_opt {
                    video.set_paused(!video.paused());
                }
            }
            Message::ToggleLoop => {
                if let Some(video) = &mut self.video_opt {
                    video.set_looping(!video.looping());
                }
            }
            Message::Seek(secs) => {
                if let Some(video) = &mut self.video_opt {
                    self.dragging = true;
                    self.position = secs;
                    video.set_paused(true);
                    let duration = Duration::try_from_secs_f64(self.position).unwrap_or_default();
                    video.seek(duration, true).expect("seek");
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
                if !self.dragging {
                    if let Some(video) = &self.video_opt {
                        self.position = video.position().as_secs_f64();
                    }
                }
            }
            Message::Reload => {
                return self.load();
            }
            Message::SystemThemeModeChange(_theme_mode) => {
                return self.update_config();
            }
        }
        Command::none()
    }

    fn header_start(&self) -> Vec<Element<Self::Message>> {
        vec![widget::row::with_children(vec![
            //TODO: allow mute/unmute/change volume
            widget::icon::from_name("audio-volume-high-symbolic")
                .size(16)
                .into(),
            widget::dropdown(
                &self.audio_codes,
                usize::try_from(self.current_audio).ok(),
                Message::AudioCode,
            )
            .into(),
            //TODO: allow toggling subtitles
            widget::icon::from_name("media-view-subtitles-symbolic")
                .size(16)
                .into(),
            widget::dropdown(
                &self.text_codes,
                usize::try_from(self.current_text).ok(),
                Message::TextCode,
            )
            .into(),
            widget::button::icon(widget::icon::from_name("view-fullscreen-symbolic").size(16))
                .on_press(Message::Fullscreen)
                .into(),
        ])
        .align_items(Alignment::Center)
        .spacing(8)
        .into()]
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
                .style(theme::Container::WindowBackground)
                .into();
        };

        let video_player = VideoPlayer::new(video)
            .on_end_of_stream(Message::EndOfStream)
            .on_missing_plugin(Message::MissingPlugin)
            .on_new_frame(Message::NewFrame)
            .width(Length::Fill)
            .height(Length::Fill);

        let mut popover = widget::popover(video_player).position(widget::popover::Position::Bottom);
        if !self.fullscreen {
            popover = popover.popup(
                widget::container(
                    widget::row::with_capacity(4)
                        .align_items(Alignment::Center)
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
                            .on_press(Message::TogglePause),
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
                        ),
                )
                .style(theme::Container::WindowBackground),
            );
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

        Subscription::batch([
            event::listen_with(|event, _status| match event {
                Event::Keyboard(KeyEvent::KeyPressed { key, modifiers, .. }) => {
                    Some(Message::Key(modifiers, key))
                }
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
