//! Reads what Apple Music is playing from Windows' media session API.

pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    /// Seconds into the track, when Windows knows it.
    pub position: Option<f64>,
    /// Track length in seconds, when Windows knows it.
    pub duration: Option<f64>,
}

#[cfg(windows)]
pub use imp::Media;

#[cfg(windows)]
mod imp {
    use super::NowPlaying;
    use std::time::{SystemTime, UNIX_EPOCH};
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager as SessionManager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
    };

    #[derive(Default)]
    pub struct Media {
        manager: Option<SessionManager>,
    }

    const TICKS_PER_SEC: f64 = 10_000_000.0;
    /// 100ns ticks between 1601-01-01 (Windows epoch) and 1970-01-01.
    const EPOCH_DIFF_TICKS: i64 = 116_444_736_000_000_000;

    impl Media {
        /// `Ok(None)` means Apple Music isn't open (or has nothing loaded).
        pub fn now_playing(&mut self) -> Result<Option<NowPlaying>, String> {
            let result = self.read();
            if result.is_err() {
                self.manager = None; // re-request next time
            }
            result.map_err(|e| e.to_string())
        }

        fn read(&mut self) -> windows::core::Result<Option<NowPlaying>> {
            if self.manager.is_none() {
                self.manager = Some(SessionManager::RequestAsync()?.get()?);
            }
            let manager = self.manager.as_ref().unwrap();

            let mut session = None;
            for s in manager.GetSessions()? {
                let id = s.SourceAppUserModelId()?.to_string();
                if id.contains("AppleMusic") || id.contains("iTunes") {
                    session = Some(s);
                    break;
                }
            }
            let Some(session) = session else { return Ok(None) };

            let props = session.TryGetMediaPropertiesAsync()?.get()?;
            let title = props.Title()?.to_string().trim().to_string();
            if title.is_empty() {
                return Ok(None);
            }
            let mut artist = props.Artist()?.to_string().trim().to_string();
            let mut album = props.AlbumTitle()?.to_string().trim().to_string();
            // The Apple Music app reports the artist field as "Artist — Album".
            if let Some(i) = artist.find(" \u{2014} ") {
                if album.is_empty() {
                    album = artist[i + " \u{2014} ".len()..].trim().to_string();
                }
                artist = artist[..i].trim().to_string();
            }

            let playing = session.GetPlaybackInfo()?.PlaybackStatus()? == PlaybackStatus::Playing;

            let mut position = None;
            let mut duration = None;
            if let Ok(tl) = session.GetTimelineProperties() {
                if let (Ok(start), Ok(end), Ok(pos)) = (tl.StartTime(), tl.EndTime(), tl.Position()) {
                    if end.Duration as f64 / TICKS_PER_SEC > 1.0 {
                        let dur = (end.Duration - start.Duration) as f64 / TICKS_PER_SEC;
                        let mut p = pos.Duration as f64 / TICKS_PER_SEC;
                        if playing {
                            if let Ok(updated) = tl.LastUpdatedTime() {
                                let age = (now_ticks() - updated.UniversalTime) as f64 / TICKS_PER_SEC;
                                if age > 0.0 && age < 36_000.0 {
                                    p += age;
                                }
                            }
                        }
                        position = Some(p.min(dur));
                        duration = Some(dur);
                    }
                }
            }

            Ok(Some(NowPlaying { title, artist, album, playing, position, duration }))
        }
    }

    fn now_ticks() -> i64 {
        let unix = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        (unix.as_nanos() / 100) as i64 + EPOCH_DIFF_TICKS
    }
}

/// Stand-in so the rest of the app type-checks on other systems.
#[cfg(not(windows))]
#[derive(Default)]
pub struct Media;

#[cfg(not(windows))]
impl Media {
    pub fn now_playing(&mut self) -> Result<Option<NowPlaying>, String> {
        Ok(None)
    }
}
