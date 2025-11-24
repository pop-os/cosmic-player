use cosmic::iced::{
    futures::{self, SinkExt},
    subscription::{self, Subscription},
};
use mpris_server::{
    LoopStatus, Metadata, PlaybackRate, PlaybackStatus, PlayerInterface, Property, RootInterface,
    Server, Signal, Time, TrackId, Volume,
    zbus::{Result, fdo},
};
use std::{any::TypeId, future, process};
use tokio::sync::{Mutex, mpsc};

use crate::{Message, MprisEvent, MprisMeta, MprisState};

impl MprisMeta {
    fn metadata(&self) -> Metadata {
        let mut meta = Metadata::builder()
            //TODO: better track id
            .trackid(
                mpris_server::TrackId::try_from(format!(
                    "/com/system76/CosmicPlayer/pid{}/TrackList/0",
                    process::id()
                ))
                .unwrap(),
            )
            .length(Time::from_micros(self.duration_micros));
        if let Some(url) = &self.url_opt {
            meta = meta.url(url.clone());
        }
        if !self.album.is_empty() {
            meta = meta.album(&self.album);
        }
        if let Some(album_art) = &self.album_art_opt {
            meta = meta.art_url(album_art.clone());
        }
        if !self.artists.is_empty() {
            meta = meta.artist(&self.artists);
        }
        //TODO: content_created
        if !self.title.is_empty() {
            meta = meta.title(&self.title);
        }
        //TODO .disc_number(self.disc_number)
        if self.track_number > 0 {
            meta = meta.track_number(self.track_number);
        }
        //TODO: track count?
        //TODO: more keys, see https://docs.rs/mpris-server/0.8.1/mpris_server/builder/struct.MetadataBuilder.html
        meta.build()
    }
}

impl MprisState {
    fn playback_status(&self) -> PlaybackStatus {
        if self.paused {
            PlaybackStatus::Paused
        } else {
            PlaybackStatus::Playing
        }
    }
}

pub struct Player {
    msg_tx: Mutex<futures::channel::mpsc::Sender<Message>>,
    meta: Mutex<MprisMeta>,
    state: Mutex<MprisState>,
}

impl Player {
    async fn message(&self, message: Message) -> fdo::Result<()> {
        self.msg_tx
            .lock()
            .await
            .send(message)
            .await
            .map_err(|err| fdo::Error::Failed(err.to_string()))
    }
}

impl RootInterface for Player {
    async fn raise(&self) -> fdo::Result<()> {
        log::info!("Raise");
        Ok(())
    }

    async fn quit(&self) -> fdo::Result<()> {
        log::info!("Quit");
        Ok(())
    }

    async fn can_quit(&self) -> fdo::Result<bool> {
        log::info!("CanQuit");
        Ok(false)
    }

    async fn fullscreen(&self) -> fdo::Result<bool> {
        log::info!("Fullscreen");
        let state = self.state.lock().await;
        Ok(state.fullscreen)
    }

    async fn set_fullscreen(&self, fullscreen: bool) -> Result<()> {
        log::info!("SetFullscreen({})", fullscreen);
        Ok(())
    }

    async fn can_set_fullscreen(&self) -> fdo::Result<bool> {
        log::info!("CanSetFullscreen");
        Ok(false)
    }

    async fn can_raise(&self) -> fdo::Result<bool> {
        log::info!("CanRaise");
        Ok(false)
    }

    async fn has_track_list(&self) -> fdo::Result<bool> {
        log::info!("HasTrackList");
        Ok(false)
    }

    async fn identity(&self) -> fdo::Result<String> {
        log::info!("Identity");
        Ok("COSMIC Player".to_string())
    }

    async fn desktop_entry(&self) -> fdo::Result<String> {
        log::info!("DesktopEntry");
        Ok("com.system76.CosmicPlayer".to_string())
    }

    async fn supported_uri_schemes(&self) -> fdo::Result<Vec<String>> {
        log::info!("SupportedUriSchemes");
        Ok(vec![])
    }

    async fn supported_mime_types(&self) -> fdo::Result<Vec<String>> {
        log::info!("SupportedMimeTypes");
        Ok(vec![])
    }
}

impl PlayerInterface for Player {
    async fn next(&self) -> fdo::Result<()> {
        log::info!("Next");
        Ok(())
    }

    async fn previous(&self) -> fdo::Result<()> {
        log::info!("Previous");
        Ok(())
    }

    async fn pause(&self) -> fdo::Result<()> {
        log::info!("Pause");
        self.message(Message::Pause).await
    }

    async fn play_pause(&self) -> fdo::Result<()> {
        log::info!("PlayPause");
        self.message(Message::PlayPause).await
    }

    async fn stop(&self) -> fdo::Result<()> {
        log::info!("Stop");
        Ok(())
    }

    async fn play(&self) -> fdo::Result<()> {
        log::info!("Play");
        self.message(Message::Play).await
    }

    async fn seek(&self, offset: Time) -> fdo::Result<()> {
        log::info!("Seek({:?})", offset);
        Ok(())
    }

    async fn set_position(&self, track_id: TrackId, position: Time) -> fdo::Result<()> {
        log::info!("SetPosition({}, {:?})", track_id, position);
        Ok(())
    }

    async fn open_uri(&self, uri: String) -> fdo::Result<()> {
        log::info!("OpenUri({})", uri);
        Ok(())
    }

    async fn playback_status(&self) -> fdo::Result<PlaybackStatus> {
        log::info!("PlaybackStatus");
        let state = self.state.lock().await;
        Ok(state.playback_status())
    }

    async fn loop_status(&self) -> fdo::Result<LoopStatus> {
        log::info!("LoopStatus");
        Ok(LoopStatus::None)
    }

    async fn set_loop_status(&self, loop_status: LoopStatus) -> Result<()> {
        log::info!("SetLoopStatus({})", loop_status);
        Ok(())
    }

    async fn rate(&self) -> fdo::Result<PlaybackRate> {
        log::info!("Rate");
        Ok(1.0)
    }

    async fn set_rate(&self, rate: PlaybackRate) -> Result<()> {
        log::info!("SetRate({})", rate);
        Ok(())
    }

    async fn shuffle(&self) -> fdo::Result<bool> {
        log::info!("Shuffle");
        Ok(false)
    }

    async fn set_shuffle(&self, shuffle: bool) -> Result<()> {
        log::info!("SetShuffle({})", shuffle);
        Ok(())
    }

    async fn metadata(&self) -> fdo::Result<Metadata> {
        log::info!("Metadata");
        let meta = self.meta.lock().await;
        Ok(meta.metadata())
    }

    async fn volume(&self) -> fdo::Result<Volume> {
        log::info!("Volume");
        let state = self.state.lock().await;
        Ok(state.volume)
    }

    async fn set_volume(&self, volume: Volume) -> Result<()> {
        log::info!("SetVolume({})", volume);
        self.message(Message::AudioVolume(volume)).await?;
        Ok(())
    }

    async fn position(&self) -> fdo::Result<Time> {
        log::info!("Position");
        let state = self.state.lock().await;
        Ok(Time::from_micros(state.position_micros))
    }

    async fn minimum_rate(&self) -> fdo::Result<PlaybackRate> {
        log::info!("MinimumRate");
        Ok(1.0)
    }

    async fn maximum_rate(&self) -> fdo::Result<PlaybackRate> {
        log::info!("MaximumRate");
        Ok(1.0)
    }

    async fn can_go_next(&self) -> fdo::Result<bool> {
        log::info!("CanGoNext");
        Ok(false)
    }

    async fn can_go_previous(&self) -> fdo::Result<bool> {
        log::info!("CanGoPrevious");
        Ok(false)
    }

    async fn can_play(&self) -> fdo::Result<bool> {
        log::info!("CanPlay");
        Ok(true)
    }

    async fn can_pause(&self) -> fdo::Result<bool> {
        log::info!("CanPause");
        Ok(true)
    }

    async fn can_seek(&self) -> fdo::Result<bool> {
        log::info!("CanSeek");
        Ok(false)
    }

    async fn can_control(&self) -> fdo::Result<bool> {
        log::info!("CanControl");
        Ok(true)
    }
}

/*TODO: implement mpris tracklist
impl TrackListInterface for Player {
    async fn get_tracks_metadata(&self, track_ids: Vec<TrackId>) -> fdo::Result<Vec<Metadata>> {
        log::info!("GetTracksMetadata({:?})", track_ids);
        Ok(vec![])
    }

    async fn add_track(
        &self,
        uri: Uri,
        after_track: TrackId,
        set_as_current: bool,
    ) -> fdo::Result<()> {
        log::info!("AddTrack({}, {}, {})", uri, after_track, set_as_current);
        Ok(())
    }

    async fn remove_track(&self, track_id: TrackId) -> fdo::Result<()> {
        log::info!("RemoveTrack({})", track_id);
        Ok(())
    }

    async fn go_to(&self, track_id: TrackId) -> fdo::Result<()> {
        log::info!("GoTo({})", track_id);
        Ok(())
    }

    async fn tracks(&self) -> fdo::Result<Vec<TrackId>> {
        log::info!("Tracks");
        Ok(vec![])
    }

    async fn can_edit_tracks(&self) -> fdo::Result<bool> {
        log::info!("CanEditTracks");
        Ok(false)
    }
}
*/

/*TODO: implement mpris playlists
impl PlaylistsInterface for Player {
    async fn activate_playlist(&self, playlist_id: PlaylistId) -> fdo::Result<()> {
        log::info!("ActivatePlaylist({})", playlist_id);
        Ok(())
    }

    async fn get_playlists(
        &self,
        index: u32,
        max_count: u32,
        order: PlaylistOrdering,
        reverse_order: bool,
    ) -> fdo::Result<Vec<Playlist>> {
        log::info!(
            "GetPlaylists({}, {}, {}, {})",
            index, max_count, order, reverse_order
        );
        Ok(vec![])
    }

    async fn playlist_count(&self) -> fdo::Result<u32> {
        log::info!("PlaylistCount");
        Ok(0)
    }

    async fn orderings(&self) -> fdo::Result<Vec<PlaylistOrdering>> {
        log::info!("Orderings");
        Ok(vec![])
    }

    async fn active_playlist(&self) -> fdo::Result<Option<Playlist>> {
        log::info!("ActivePlaylist");
        Ok(None)
    }
}
*/

pub fn subscription() -> Subscription<Message> {
    struct MprisSubscription;
    subscription::channel(
        TypeId::of::<MprisSubscription>(),
        16,
        move |mut msg_tx| async move {
            let (event_tx, mut event_rx) = mpsc::unbounded_channel();
            let meta = MprisMeta::default();
            let state = MprisState::default();
            msg_tx
                .send(Message::MprisChannel(meta.clone(), state.clone(), event_tx))
                .await
                .unwrap();
            match Server::new(
                &format!("org.mpris.MediaPlayer2.cosmic-player.pid{}", process::id()),
                Player {
                    msg_tx: Mutex::new(msg_tx),
                    meta: Mutex::new(meta),
                    state: Mutex::new(state),
                },
            )
            .await
            {
                Ok(server) => {
                    log::info!("running mpris server");
                    while let Some(event) = event_rx.recv().await {
                        let mut props = Vec::new();
                        let mut sigs = Vec::new();
                        match event {
                            MprisEvent::Meta(new) => {
                                let mut old = server.imp().meta.lock().await;
                                let new_metadata = new.metadata();
                                if new_metadata != old.metadata() {
                                    props.push(Property::Metadata(new_metadata));
                                }
                                *old = new;
                            }
                            MprisEvent::State(new) => {
                                let mut old = server.imp().state.lock().await;
                                if new.fullscreen != old.fullscreen {
                                    props.push(Property::Fullscreen(new.fullscreen));
                                }
                                let new_playback_status = new.playback_status();
                                if new_playback_status != old.playback_status() {
                                    props.push(Property::PlaybackStatus(new_playback_status));
                                }
                                if new.volume != old.volume {
                                    props.push(Property::Volume(new.volume));
                                }
                                if new.position_micros != old.position_micros {
                                    sigs.push(Signal::Seeked {
                                        position: Time::from_micros(new.position_micros),
                                    });
                                }
                                *old = new;
                            }
                        }
                        if !props.is_empty() {
                            let _ = server.properties_changed(props).await;
                        }
                        for sig in sigs {
                            let _ = server.emit(sig).await;
                        }
                    }
                    future::pending().await
                }
                Err(err) => {
                    log::warn!("failed to start mpris server: {err}");
                    future::pending().await
                }
            }
        },
    )
}
