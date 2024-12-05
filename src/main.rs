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
        window, Alignment, Color, Length, Limits,
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
    fs, process,
    time::{Duration, Instant},
};

use crate::{
    config::{Config, CONFIG_VERSION},
    key_bind::{key_binds, KeyBind},
};

mod config;
mod key_bind;
mod localize;
mod menu;

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

/// Messages that are used specifically by our [`App`].
#[derive(Clone, Debug)]
pub enum Message {
    Config(Config),
    FileClose,
    FileLoad(url::Url),
    FileOpen,
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
    WindowClose,
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
        self.audio_codes = Vec::new();
        self.current_audio = -1;
        self.text_codes = Vec::new();
        self.current_text = -1;
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

        let n_audio = pipeline.property::<i32>("n-audio");
        self.audio_codes = Vec::with_capacity(n_audio as usize);
        for i in 0..n_audio {
            let tags: gst::TagList = pipeline.emit_by_name("get-audio-tags", &[&i]);
            log::info!("audio stream {i}: {tags:?}");
            self.audio_codes
                .push(if let Some(title) = tags.get::<gst::tags::Title>() {
                    title.get().to_string()
                } else if let Some(language_code) = tags.get::<gst::tags::LanguageCode>() {
                    let language_code = language_code.get();
                    language_name(language_code).unwrap_or_else(|| language_code.to_string())
                } else {
                    format!("Audio #{i}")
                });
        }
        self.current_audio = pipeline.property::<i32>("current-audio");

        let n_text = pipeline.property::<i32>("n-text");
        self.text_codes = Vec::with_capacity(n_text as usize);
        for i in 0..n_text {
            let tags: gst::TagList = pipeline.emit_by_name("get-text-tags", &[&i]);
            log::info!("text stream {i}: {tags:?}");
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
            Message::WindowClose => {
                process::exit(0);
            }
        }
        Command::none()
    }

    fn header_start(&self) -> Vec<Element<Self::Message>> {
        let mut row = widget::row::with_capacity(5)
            .align_items(Alignment::Center)
            .spacing(8);
        row = row.push(menu::menu_bar(&self.flags.config, &self.key_binds));
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
        let cosmic_theme::Spacing {
            space_xxs,
            space_xs,
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
            //TODO: open button if no video?
            return widget::container(widget::text("No video open"))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(theme::Container::WindowBackground)
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
                        .align_items(Alignment::Center)
                        .spacing(space_xxs)
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
                .padding([space_xxs, space_xs])
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
