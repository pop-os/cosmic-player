// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use cosmic::{
    Application, ApplicationExt, Element,
    app::{Command, Core, Settings, command, message},
    cosmic_config::{self, CosmicConfigEntry},
    cosmic_theme, executor, font,
    iced::{
        Alignment, Background, Border, Color, ContentFit, Length, Limits,
        event::{self, Event},
        keyboard::{Event as KeyEvent, Key, Modifiers},
        mouse::{Event as MouseEvent, ScrollDelta},
        subscription::Subscription,
        window,
    },
    iced_style, theme,
    widget::{self, Slider, menu::action::MenuAction, nav_bar, segmented_button},
};
use iced_video_player::{
    Video, VideoPlayer,
    gst::{self, prelude::*},
    gst_pbutils,
};
use std::{
    any::TypeId,
    collections::HashMap,
    ffi::{CStr, CString},
    fs,
    path::{Path, PathBuf},
    process, thread,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

use crate::{
    config::{CONFIG_VERSION, Config, ConfigState},
    key_bind::{KeyBind, key_binds},
    project::ProjectNode,
};

mod argparse;
mod config;
mod key_bind;
mod localize;
mod menu;
#[cfg(feature = "mpris-server")]
mod mpris;
mod project;
mod thumbnail;
mod video;
#[cfg(feature = "xdg-portal")]
mod xdg_portals;

static CONTROLS_TIMEOUT: Duration = Duration::new(2, 0);

const GST_PLAY_FLAG_VIDEO: i32 = 1 << 0;
const GST_PLAY_FLAG_AUDIO: i32 = 1 << 1;
const GST_PLAY_FLAG_TEXT: i32 = 1 << 2;

use std::error::Error;

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
fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let args = argparse::parse();

    if let Some(output) = args.thumbnail_opt {
        let Some(input) = args.url_opt else {
            log::error!("thumbnailer can only handle exactly one URL");
            process::exit(1);
        };

        match thumbnail::main(&input, &output, args.size_opt) {
            Ok(()) => process::exit(0),
            Err(err) => {
                log::error!("failed to thumbnail '{}': {}", input, err);
                process::exit(1);
            }
        }
    }

    #[cfg(all(unix, not(target_os = "redox")))]
    match fork::daemon(true, true) {
        Ok(fork::Fork::Child) => (),
        Ok(fork::Fork::Parent(_child_pid)) => process::exit(0),
        Err(err) => {
            eprintln!("failed to daemonize: {:?}", err);
            process::exit(1);
        }
    }

    localize::localize();

    let config = match cosmic_config::Config::new(App::APP_ID, CONFIG_VERSION) {
        Ok(config_handler) => {
            match Config::get_entry(&config_handler) {
                Ok(ok) => ok,
                Err((errs, config)) => {
                    log::error!("errors loading config: {:?}", errs);
                    config
                }
            }
        }
        Err(err) => {
            log::error!("failed to create config handler: {}", err);
            Config::default()
        }
    };

    let (config_state_handler, config_state) =
        match cosmic_config::Config::new_state(App::APP_ID, CONFIG_VERSION) {
            Ok(config_state_handler) => {
                let config_state = ConfigState::get_entry(&config_state_handler).unwrap_or_else(
                    |(errs, config_state)| {
                        log::info!("errors loading config_state: {:?}", errs);
                        config_state
                    },
                );
                (Some(config_state_handler), config_state)
            }
            Err(err) => {
                log::error!("failed to create config_state handler: {}", err);
                (None, ConfigState::default())
            }
        };

    let mut settings = Settings::default();
    settings = settings.theme(config.app_theme.theme());
    settings = settings.size_limits(Limits::NONE.min_width(360.0).min_height(180.0));

   
    let flags = Flags {
        config,
        config_state_handler,
        config_state,
        url_opt: args.url_opt,
        urls: args.urls,
    };
    cosmic::app::run::<App>(settings, flags)?;

    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    FileClose,
    FileOpen,
    FileClearRecents,
    FileOpenRecent(usize),
    FolderClose(usize),
    FolderOpen,
    FolderClearRecents,
    FolderOpenRecent(usize),
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
            Self::FileClearRecents => Message::FileClearRecents,
            Self::FileOpenRecent(index) => Message::FileOpenRecent(*index),
            Self::FolderClose(index) => Message::FolderClose(*index),
            Self::FolderOpen => Message::FolderOpen,
            Self::FolderClearRecents => Message::FolderClearRecents,
            Self::FolderOpenRecent(index) => Message::FolderOpenRecent(*index),
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
    config: Config,
    config_state_handler: Option<cosmic_config::Config>,
    config_state: ConfigState,
    url_opt: Option<url::Url>,
    urls: Option<Vec<url::Url>>,
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
    album_year_opt: Option<i32>,
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

#[derive(Clone, Debug)]
pub struct TextCode {
    pub id: Option<i32>,
    pub name: String,
}

impl AsRef<str> for TextCode {
    fn as_ref(&self) -> &str {
        self.name.as_str()
    }
}

/// Messages that are used specifically by our [`App`].
#[derive(Clone, Debug)]
pub enum Message {
    None,
    Config(Config),
    ConfigState(ConfigState),
    DropdownToggle(DropdownKind),
    DurationChanged(Duration),
    FileClose,
    FileLoad(url::Url),
    FileOpen,
    FileClearRecents,
    FileOpenRecent(usize),
    FolderClose(usize),
    FolderLoad(PathBuf),
    FolderOpen,
    FolderClearRecents,
    FolderOpenRecent(usize),
    MultipleLoad(Vec<url::Url>),
    Fullscreen,
    Key(Modifiers, Key),
    AudioCode(usize),
    AudioToggle,
    AudioVolume(f64),
    TextCode(usize),
    Pause,
    Play,
    PlayPause,
    Scrolled(ScrollDelta),
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
    mpris_meta: MprisMeta,
    mpris_opt: Option<(MprisMeta, MprisState, mpsc::UnboundedSender<MprisEvent>)>,
    nav_model: segmented_button::SingleSelectModel,
    projects: Vec<(String, PathBuf)>,
    video_opt: Option<Video>,
    position: f64,
    duration: f64,
    dragging: bool,
    paused_on_scrub: bool,
    audio_codes: Vec<String>,
    audio_tags: Vec<gst::TagList>,
    current_audio: i32,
    text_codes: Vec<TextCode>,
    current_text: Option<i32>,
    #[cfg(feature = "xdg-portal")]
    inhibit: tokio::sync::watch::Sender<bool>,
}

impl App {
    fn close(&mut self) -> bool {
        self.album_art_opt = None;
        //TODO: drop does not work well
        let was_open = if let Some(mut video) = self.video_opt.take() {
            log::info!("pausing video");
            video.set_paused(true);
            log::info!("dropping video");
            drop(video);
            log::info!("dropped video");
            true
        } else {
            false
        };
        self.position = 0.0;
        self.duration = 0.0;
        self.dragging = false;
        self.audio_codes.clear();
        self.audio_tags.clear();
        self.current_audio = -1;
        self.text_codes.clear();
        self.current_text = None;
        self.update_mpris_meta();
        self.update_nav_bar_active();
        self.allow_idle();
        was_open
    }

    fn load(&mut self) -> Command<Message> {
        if self.close() {
            // Allow a redraw before trying to load again, to prevent deadlock
            return Command::perform(async { message::app(Message::Reload) }, |x| x);
        }

        let url = match &self.flags.url_opt {
            Some(some) => some.clone(),
            None => return Command::none(),
        };

        log::info!("Loading {}", url);

        // Add to recent files, ensuring only one entry
        self.flags.config_state.recent_files.retain(|x| x != &url);
        self.flags.config_state.recent_files.push_front(url.clone());
        self.flags.config_state.recent_files.truncate(10);
        self.save_config_state();

        let video = match video::new_video(&url) {
            Ok(ok) => ok,
            Err(err) => return err,
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
        self.text_codes = Vec::with_capacity(n_text as usize + 1);
        self.text_codes.push(TextCode {
            id: None,
            name: fl!("off"),
        });
        for i in 0..n_text {
            let tags: gst::TagList = pipeline.emit_by_name("get-text-tags", &[&i]);
            log::info!("text stream {i}: {tags:#?}");
            let name = if let Some(title) = tags.get::<gst::tags::Title>() {
                title.get().to_string()
            } else if let Some(language_code) = tags.get::<gst::tags::LanguageCode>() {
                let language_code = language_code.get();
                language_name(language_code).unwrap_or_else(|| language_code.to_string())
            } else {
                format!("Subtitle #{i}")
            };
            self.text_codes.push(TextCode { id: Some(i), name });
        }
        let current_text = pipeline.property::<i32>("current-text");
        if current_text >= 0 {
            self.current_text = Some(current_text);
        } else {
            self.current_text = None;
        }

        self.inhibit_idle();
        self.update_flags();
        self.update_mpris_meta();
        self.update_title()
    }

    fn open_folder<P: AsRef<Path>>(&mut self, path: P, mut position: u16, indent: u16) {
        let read_dir = match fs::read_dir(&path) {
            Ok(ok) => ok,
            Err(err) => {
                log::error!("failed to read directory {:?}: {}", path.as_ref(), err);
                return;
            }
        };

        let mut nodes = Vec::new();
        for entry_res in read_dir {
            let entry = match entry_res {
                Ok(ok) => ok,
                Err(err) => {
                    log::error!(
                        "failed to read entry in directory {:?}: {}",
                        path.as_ref(),
                        err
                    );
                    continue;
                }
            };

            let entry_path = entry.path();
            let node = match ProjectNode::new(&entry_path) {
                Ok(ok) => ok,
                Err(err) => {
                    log::error!(
                        "failed to open directory {:?} entry {:?}: {}",
                        path.as_ref(),
                        entry_path,
                        err
                    );
                    continue;
                }
            };
            nodes.push(node);
        }

        nodes.sort();

        for node in nodes {
            let mut entity = self
                .nav_model
                .insert()
                .position(position)
                .indent(indent)
                .text(node.name().to_string());
            if let Some(icon) = node.icon(16) {
                entity = entity.icon(icon);
            }
            entity.data(node);

            position += 1;
        }
    }

    pub fn open_project<P: AsRef<Path>>(&mut self, path: P) {
        let path = path.as_ref();
        let node = match ProjectNode::new(path) {
            Ok(mut node) => {
                match &mut node {
                    ProjectNode::Folder {
                        name,
                        path,
                        open,
                        root,
                    } => {
                        *open = true;
                        *root = true;

                        for (_project_name, project_path) in self.projects.iter() {
                            if project_path == path {
                                // Project already open
                                return;
                            }
                        }

                        // Save the absolute path
                        self.projects.push((name.to_string(), path.to_path_buf()));

                        // Add to recent projects, ensuring only one entry
                        self.flags
                            .config_state
                            .recent_projects
                            .retain(|x| x != path);
                        self.flags
                            .config_state
                            .recent_projects
                            .push_front(path.to_path_buf());
                        self.flags.config_state.recent_projects.truncate(10);
                        self.save_config_state();

                        // Open nav bar
                        self.core.nav_bar_set_toggled(true);
                    }
                    _ => {
                        log::error!("failed to open project {:?}: not a directory", path);
                        return;
                    }
                }
                node
            }
            Err(err) => {
                log::error!("failed to open project {:?}: {}", path, err);
                return;
            }
        };

        let mut entity = self.nav_model.insert().text(node.name().to_string());
        if let Some(icon) = node.icon(16) {
            entity = entity.icon(icon);
        }
        entity = entity.data(node);

        let id = entity.id();

        let position = self.nav_model.position(id).unwrap_or(0);

        self.open_folder(path, position + 1, 1);
    }

    fn add_file_to_project(&mut self, path: impl AsRef<Path>) {
        let path = path.as_ref();
        let node = match ProjectNode::new(path) {
            Ok(node) if matches!(node, ProjectNode::File { .. }) => node,
            Err(e) => {
                log::error!("failed to open project {} {}", path.display(), e);
                return;
            }
            _ => {
                log::error!(
                    "failed to open project: expected {} to be a file path",
                    path.display()
                );
                return;
            }
        };

        let mut entity = self.nav_model.insert().text(node.name().to_owned());
        if let Some(icon) = node.icon(16) {
            entity = entity.icon(icon);
        }
        entity.data(node);
    }

    fn save_config_state(&mut self) {
        if let Some(ref config_state_handler) = self.flags.config_state_handler {
            if let Err(err) = self.flags.config_state.write_entry(config_state_handler) {
                log::error!("failed to save config_state: {}", err);
            }
        }
    }

    fn update_controls(&mut self, in_use: bool) {
        if in_use
            || !self
                .video_opt
                .as_ref()
                .map_or(false, |video| video.has_video())
        {
            self.core.window.show_headerbar = true && !self.fullscreen;
            self.controls = true;
            self.controls_time = Instant::now();
        } else if self.controls && self.controls_time.elapsed() > CONTROLS_TIMEOUT {
            self.core.window.show_headerbar = false;
            self.controls = false;
        }
        self.update_mpris_state();
    }

    fn update_config(&mut self) -> Command<Message> {
        cosmic::app::command::set_theme(self.flags.config.app_theme.theme())
    }

    fn update_flags(&mut self) {
        let Some(video) = &mut self.video_opt else {
            return;
        };
        let pipeline = video.pipeline();
        let flags_value = pipeline.property_value("flags");
        match flags_value.transform::<i32>() {
            Ok(flags_transform) => match flags_transform.get::<i32>() {
                Ok(mut flags) => {
                    flags |= GST_PLAY_FLAG_VIDEO | GST_PLAY_FLAG_AUDIO;
                    if self.current_text.is_some() {
                        flags |= GST_PLAY_FLAG_TEXT;
                    } else {
                        flags &= !GST_PLAY_FLAG_TEXT;
                    }
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
    }

    fn update_mpris_meta(&mut self) {
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
            if let Some(tag) = tags.get::<gst::tags::DateTime>() {
                new.album_year_opt = Some(tag.get().year());
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
                                                image::codecs::png::PngEncoder::new(&mut album_art),
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
                                            log::warn!("failed to create temporary image: {}", err);
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
        if let Some((old, _, tx)) = &mut self.mpris_opt {
            if new != *old {
                *old = new.clone();
                let _ = tx.send(MprisEvent::Meta(new.clone()));
            }
        }
        self.mpris_meta = new;
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

    fn update_nav_bar_active(&mut self) {
        let tab_path_opt = match &self.flags.url_opt {
            Some(url) => url.to_file_path().ok(),
            None => None,
        };

        // Locate tree node to activate
        let mut active_id = segmented_button::Entity::default();

        if let Some(tab_path) = tab_path_opt {
            // Automatically expand tree to find and select active file
            loop {
                let mut expand_opt = None;
                for id in self.nav_model.iter() {
                    if let Some(node) = self.nav_model.data(id) {
                        match node {
                            ProjectNode::Folder { path, open, .. } => {
                                if tab_path.starts_with(path) && !*open {
                                    expand_opt = Some(id);
                                    break;
                                }
                            }
                            ProjectNode::File { path, .. } => {
                                if path == &tab_path {
                                    active_id = id;
                                    break;
                                }
                            }
                        }
                    }
                }
                match expand_opt {
                    Some(id) => {
                        //TODO: can this be optimized?
                        // Task not used becuase opening a folder just returns Task::none
                        let _ = self.on_nav_select(id);
                    }
                    None => {
                        break;
                    }
                }
            }
        }
        self.nav_model.activate(active_id);
    }

    fn update_title(&mut self) -> Command<Message> {
        //TODO: filename?
        let title = "COSMIC Media Player";
        self.set_window_title(title.to_string())
    }

    /// Allow screen to dim or turn off if there is no input from the user.
    ///
    /// Basically, undo [`Self::inhibit_idle`].
    fn allow_idle(&self) {
        #[cfg(feature = "xdg-portal")]
        let _ = self.inhibit.send(false);
    }

    /// Prevent screen from dimming or turning off if there is no keyboard/mouse input.
    fn inhibit_idle(&self) {
        #[cfg(feature = "xdg-portal")]
        let _ = self.inhibit.send(true);
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

        #[cfg(feature = "xdg-portal")]
        let inhibit = {
            let (tx, rx) = tokio::sync::watch::channel(false);
            std::mem::drop(tokio::spawn(crate::xdg_portals::inhibit(rx)));
            tx
        };

        let mut app = App {
            core,
            flags,
            album_art_opt: None,
            controls: true,
            controls_time: Instant::now(),
            dropdown_opt: None,
            fullscreen: false,
            key_binds: key_binds(),
            mpris_meta: MprisMeta::default(),
            mpris_opt: None,
            nav_model: nav_bar::Model::builder().build(),
            projects: Vec::new(),
            video_opt: None,
            position: 0.0,
            duration: 0.0,
            dragging: false,
            paused_on_scrub: false,
            audio_codes: Vec::new(),
            audio_tags: Vec::new(),
            current_audio: -1,
            text_codes: Vec::new(),
            current_text: None,
            #[cfg(feature = "xdg-portal")]
            inhibit,
        };

        // Do not show nav bar by default. Will be opened by open_project if needed
        app.core.nav_bar_set_toggled(false);
        //TODO: handle command line arguments that are folders?

        // Add button to open a project
        //TODO: remove and show this based on open projects?
        app.nav_model
            .insert()
            .icon(widget::icon::from_name("folder-open-symbolic").size(16))
            .text(fl!("open-folder"));

        // TODO: This is kind of ugly and may be handled better in Arguments
        let maybe_path = app
            .flags
            .url_opt
            .as_ref()
            .and_then(|url| url.to_file_path().ok());
        let command = match (app.flags.urls.take(), maybe_path) {
            (Some(urls), _) => command::message::app(Message::MultipleLoad(urls)),
            (None, Some(path)) if path.is_dir() => command::message::app(Message::FolderLoad(path)),
            _ => app.load(),
        };
        (app, command)
    }

    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav_model)
    }

    fn on_escape(&mut self) -> Command<Self::Message> {
        if self.fullscreen {
            return self.update(Message::Fullscreen);
        } else {
            Command::none()
        }
    }

    fn on_nav_select(&mut self, id: nav_bar::Id) -> Command<Message> {
        // Toggle open state and get clone of node data
        let node_opt = match self.nav_model.data_mut::<ProjectNode>(id) {
            Some(node) => {
                if let ProjectNode::Folder { open, .. } = node {
                    *open = !*open;
                }
                Some(node.clone())
            }
            None => None,
        };

        match node_opt {
            Some(node) => {
                // Update icon
                if let Some(icon) = node.icon(16) {
                    self.nav_model.icon_set(id, icon);
                } else {
                    self.nav_model.icon_remove(id);
                }

                match node {
                    ProjectNode::Folder { path, open, .. } => {
                        let position = self.nav_model.position(id).unwrap_or(0);
                        let indent = self.nav_model.indent(id).unwrap_or(0);
                        if open {
                            // Open folder
                            self.open_folder(path, position + 1, indent + 1);
                        } else {
                            // Close folder
                            while let Some(child_id) = self.nav_model.entity_at(position + 1) {
                                if self.nav_model.indent(child_id).unwrap_or(0) > indent {
                                    self.nav_model.remove(child_id);
                                } else {
                                    break;
                                }
                            }
                        }

                        // Prevent nav bar from closing when selecting a
                        // folder in condensed mode.
                        self.core_mut().nav_bar_set_toggled(true);

                        Command::none()
                    }
                    ProjectNode::File { path, .. } => match url::Url::from_file_path(&path) {
                        Ok(url) => self.update(Message::FileLoad(url)),
                        Err(()) => {
                            log::warn!("failed to convert {:?} to url", path);
                            Command::none()
                        }
                    },
                }
            }
            None => {
                // Open folder
                self.update(Message::FolderOpen)
            }
        }
    }

    fn style(&self) -> Option<theme::Application> {
        // This ensures we have a solid background color even when using no content container
        Some(theme::Application::Custom(Box::new(|theme| {
            iced_style::application::Appearance {
                background_color: theme.cosmic().bg_color().into(),
                icon_color: theme.cosmic().on_bg_color().into(),
                text_color: theme.cosmic().on_bg_color().into(),
            }
        })))
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
            Message::ConfigState(config_state) => {
                if config_state != self.flags.config_state {
                    log::info!("update config state");
                    self.flags.config_state = config_state;
                }
            }
            Message::DropdownToggle(menu_kind) => {
                if self.dropdown_opt.take() != Some(menu_kind) {
                    self.dropdown_opt = Some(menu_kind);
                }
            }
            Message::DurationChanged(duration) => {
                self.duration = duration.as_secs_f64();
                self.update_mpris_meta();
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
            Message::FileClearRecents => {
                self.flags.config_state.recent_files.clear();
                self.save_config_state();
            }
            Message::FileOpenRecent(index) => {
                if let Some(url) = self.flags.config_state.recent_files.get(index) {
                    return self.update(Message::FileLoad(url.clone()));
                }
            }
            Message::FolderClose(project_i) => {
                if project_i < self.projects.len() {
                    let (_project_name, project_path) = self.projects.remove(project_i);
                    let mut position = 0;
                    let mut closing = false;
                    while let Some(id) = self.nav_model.entity_at(position) {
                        match self.nav_model.data::<ProjectNode>(id) {
                            Some(node) => {
                                if let ProjectNode::Folder { path, root, .. } = node {
                                    if path == &project_path {
                                        // Found the project root node, closing
                                        closing = true;
                                    } else if *root && closing {
                                        // Found another project root node after closing, breaking
                                        break;
                                    }
                                }
                            }
                            None => {
                                if closing {
                                    break;
                                }
                            }
                        }
                        if closing {
                            self.nav_model.remove(id);
                        } else {
                            position += 1;
                        }
                    }
                }
            }
            Message::FolderLoad(path) => {
                self.open_project(path);
            }
            Message::FolderOpen => {
                //TODO: embed cosmic-files dialog (after libcosmic rebase works)
                #[cfg(feature = "xdg-portal")]
                return Command::perform(
                    async move {
                        let dialog = cosmic::dialog::file_chooser::open::Dialog::new()
                            .title(fl!("open-media-folder"));
                        match dialog.open_folder().await {
                            Ok(response) => {
                                let url = response.url();
                                match url.to_file_path() {
                                    Ok(path) => message::app(Message::FolderLoad(path)),
                                    Err(()) => {
                                        log::warn!("unsupported folder URL {:?}", url);
                                        message::none()
                                    }
                                }
                            }
                            Err(err) => {
                                log::warn!("failed to open folder: {}", err);
                                message::none()
                            }
                        }
                    },
                    |x| x,
                );
            }
            Message::FolderOpenRecent(index) => {
                if let Some(path) = self.flags.config_state.recent_projects.get(index) {
                    return self.update(Message::FolderLoad(path.clone()));
                }
            }
            Message::FolderClearRecents => {
                self.flags.config_state.recent_projects.clear();
                self.save_config_state();
            }
            Message::MultipleLoad(urls) => {
                log::trace!("Loading multiple URLs: {urls:?}");
                let paths: Vec<_> = urls
                    .into_iter()
                    .flat_map(|url| url.to_file_path())
                    .collect();

                for path in paths {
                    if path.is_file() {
                        log::trace!("Appending file to playlist: {}", path.display());
                        self.add_file_to_project(path);
                    } else if path.is_dir() {
                        log::trace!("Appending directory to playlist: {}", path.display());
                        self.open_project(path);
                    } else {
                        log::warn!(
                            "Tried to add unsupported path to playlist: {}",
                            path.display()
                        );
                    }
                }

                self.core.nav_bar_set_toggled(true);
            }
            Message::Fullscreen => {
                //TODO: cleanest way to close dropdowns
                self.dropdown_opt = None;

                self.fullscreen = !self.fullscreen;
                self.core.window.show_headerbar = !self.fullscreen;
                self.controls = !self.fullscreen;
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
            Message::TextCode(index) => {
                if let Some(text_code) = self.text_codes.get(index) {
                    if let Some(id) = text_code.id {
                        if let Some(video) = &self.video_opt {
                            let pipeline = video.pipeline();
                            pipeline.set_property("current-text", id);
                            self.current_text = Some(pipeline.property("current-text"));
                        }
                    } else {
                        self.current_text = None;
                    }
                    self.update_flags();
                }
            }
            Message::Pause | Message::Play | Message::PlayPause => {
                //TODO: cleanest way to close dropdowns
                self.dropdown_opt = None;

                if let Some(video) = &mut self.video_opt {
                    let pause = match message {
                        Message::Play => false,
                        Message::Pause => true,
                        _ => !video.paused(),
                    };
                    video.set_paused(pause);
                    self.update_controls(true);
                    if pause {
                        self.allow_idle();
                    } else {
                        self.inhibit_idle();
                    }
                }
            }
            Message::Scrolled(delta) => {
                let nav_bar_toggled = self.core.nav_bar_active();
                if let Some(video) = &mut self.video_opt {
                    let mut volume = video.volume();
                    match delta {
                        ScrollDelta::Pixels { x, y } => {
                            if y < 0.0 {
                                // scrolling down, lower volume
                                volume -= 0.0125;
                            } else if y > 0.0 {
                                // scrolling up, increase volume
                                volume += 0.0125;
                            }

                            if x > 0.0 {
                                // scrolling left, lower volume
                                volume -= 0.0125;
                            } else if x < 0.0 {
                                // scrolling right, increase volume
                                volume += 0.0125;
                            }
                        }
                        ScrollDelta::Lines { x, y } => {
                            if y < 0.0 {
                                // scrolling down, lower volume
                                volume -= 0.0125;
                            } else if y > 0.0 {
                                // scrolling up, increase volume
                                volume += 0.0125;
                            }

                            if x > 0.0 {
                                // scrolling left, lower volume
                                volume -= 0.0125;
                            } else if x < 0.0 {
                                // scrolling right, increase volume
                                volume += 0.0125;
                            }
                        }
                    }

                    if (volume >= 0.0 && volume <= 1.0) && !nav_bar_toggled {
                        video.set_volume(volume);
                        self.update_controls(true);
                    }
                }
            }
            Message::Seek(secs) => {
                //TODO: cleanest way to close dropdowns
                self.dropdown_opt = None;

                if let Some(video) = &mut self.video_opt {
                    self.dragging = true;
                    self.position = secs;
                    self.paused_on_scrub = video.paused();
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
                    video.set_paused(self.paused_on_scrub);
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

    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        vec![menu::menu_bar(
            &self.flags.config,
            &self.flags.config_state,
            &self.key_binds,
            &self.projects,
        )]
    }

    /// Creates a view after each update.
    fn view(&self) -> Element<'_, Self::Message> {
        let theme = theme::active();
        let cosmic_theme::Spacing {
            space_xxs,
            space_xs,
            space_s,
            space_m,
            ..
        } = theme.cosmic().spacing;

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
            .on_duration_changed(Message::DurationChanged)
            .on_end_of_stream(Message::EndOfStream)
            .on_missing_plugin(Message::MissingPlugin)
            .on_new_frame(Message::NewFrame)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        let mut background_color = Color::BLACK;
        let mut text_color_opt = None;
        if !video.has_video() {
            background_color = theme.cosmic().bg_component_color().into();
            text_color_opt = Some(Color::from(theme.cosmic().on_bg_component_color()));

            let mut col = widget::column();
            col = col.push(widget::vertical_space(Length::Fill));
            if let Some(album_art) = &self.album_art_opt {
                col = col.push(
                    widget::image(widget::image::Handle::from_path(album_art.path()))
                        .content_fit(ContentFit::ScaleDown)
                        .width(Length::Fill),
                );
            } else {
                col = col.push(widget::icon::from_name("audio-x-generic-symbolic").size(256));
            }
            col = col.push(widget::vertical_space(space_s));
            if self.mpris_meta.title.is_empty() {
                col = col.push(widget::text::title4(fl!("untitled")));
            } else {
                col = col.push(widget::text::title4(&self.mpris_meta.title));
            }
            if self.mpris_meta.artists.is_empty() {
                col = col.push(widget::text::body(fl!("unknown-author")));
            } else {
                for artist in self.mpris_meta.artists.iter() {
                    col = col.push(widget::text::body(artist));
                }
            }
            col = col.push(widget::vertical_space(space_s));
            if !self.mpris_meta.album.is_empty() {
                col = col.push(widget::text::body(fl!(
                    "album",
                    album = self.mpris_meta.album.as_str()
                )));
            }
            if let Some(year) = &self.mpris_meta.album_year_opt {
                col = col.push(widget::text::body(format!("{}", year)));
            }
            col = col.push(widget::vertical_space(Length::Fill));

            // Space to keep from going under control overlay
            let mut control_height = space_xxs + 32 + space_xxs;
            if self.core.is_condensed() {
                control_height += space_xxs + 32;
            }

            // This is a hack to have the video player running but not visible (since the controls will cover it as an overlay)
            video_player = widget::row::with_children(vec![
                widget::horizontal_space(Length::Fill).into(),
                widget::container(col.push(widget::container(video_player).height(control_height)))
                    .width(320)
                    .into(),
                widget::horizontal_space(Length::Fill).into(),
            ])
            .into();
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
                        items.push(widget::text::heading(fl!("subtitles")).into());
                        items.push(
                            widget::dropdown(
                                &self.text_codes,
                                self.text_codes
                                    .iter()
                                    .position(|x| x.id == self.current_text),
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
            .style(theme::Container::Custom(Box::new(move |_theme| {
                let mut appearance =
                    widget::container::Appearance::default().with_background(background_color);
                if let Some(text_color) = text_color_opt {
                    appearance.text_color = Some(text_color);
                }
                appearance
            })))
            .into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        struct ConfigSubscription;
        struct ConfigStateSubscription;
        struct ThemeSubscription;

        let mut subscriptions = vec![
            event::listen_with(|event, _status| match event {
                Event::Keyboard(KeyEvent::KeyPressed { key, modifiers, .. }) => {
                    Some(Message::Key(modifiers, key))
                }
                Event::Mouse(MouseEvent::CursorMoved { .. }) => Some(Message::ShowControls),
                Event::Mouse(MouseEvent::WheelScrolled { delta }) => Some(Message::Scrolled(delta)),
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
                Message::Config(update.config)
            }),
            cosmic_config::config_state_subscription(
                TypeId::of::<ConfigStateSubscription>(),
                Self::APP_ID.into(),
                CONFIG_VERSION,
            )
            .map(|update| {
                if !update.errors.is_empty() {
                    log::debug!("errors loading config state: {:?}", update.errors);
                }
                Message::ConfigState(update.config)
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
