use std::process::Command;
use std::time::{Duration, Instant};

const AUTO_RESUME_MAX_AGE: Duration = Duration::from_secs(600);
const RECENT_PLAYING_WINDOW: Duration = Duration::from_secs(6);

pub struct MediaController {
    auto_paused_players: Vec<String>,
    paused_at: Option<Instant>,
    recent_playing_players: Vec<String>,
    recent_playing_at: Option<Instant>,
}

impl MediaController {
    pub fn new() -> Self {
        Self {
            auto_paused_players: Vec::new(),
            paused_at: None,
            recent_playing_players: Vec::new(),
            recent_playing_at: None,
        }
    }

    pub fn clear(&mut self) {
        self.auto_paused_players.clear();
        self.paused_at = None;
        self.recent_playing_players.clear();
        self.recent_playing_at = None;
    }

    fn list_mpris_players() -> Vec<String> {
        let Ok(output) = Command::new("busctl")
            .args(["--user", "list", "--no-legend"])
            .output()
        else {
            return Vec::new();
        };

        let text = String::from_utf8_lossy(&output.stdout);
        let mut players = Vec::new();
        for line in text.lines() {
            let dest = line.split_whitespace().next().unwrap_or("");
            if dest.starts_with("org.mpris.MediaPlayer2.") {
                players.push(dest.to_string());
            }
        }
        players
    }

    fn get_playback_status(dest: &str) -> Option<String> {
        let out = Command::new("busctl")
            .args([
                "--user",
                "get-property",
                dest,
                "/org/mpris/MediaPlayer2",
                "org.mpris.MediaPlayer2.Player",
                "PlaybackStatus",
            ])
            .output()
            .ok()?;

        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Snapshot currently playing MPRIS players while earbuds are in the ear,
    /// so that even if hardware AVRCP `XF86AudioPause` pauses the player a few
    /// milliseconds before `on_bud_removed()` runs, we still know which player
    /// was active and should be resumed when the earbud goes back in.
    pub fn snapshot_playing(&mut self) {
        let players = Self::list_mpris_players();
        let mut playing = Vec::new();
        for dest in players {
            if let Some(status) = Self::get_playback_status(&dest) {
                if status.contains("Playing") {
                    playing.push(dest);
                }
            }
        }
        if !playing.is_empty() {
            self.recent_playing_players = playing;
            self.recent_playing_at = Some(Instant::now());
        }
    }

    /// Called when an earbud transitions from in-ear to out-of-ear while In-Ear Detection (OHD) is enabled.
    pub fn on_bud_removed(&mut self) {
        let players = Self::list_mpris_players();
        let mut newly_paused = Vec::new();

        for dest in &players {
            if let Some(status) = Self::get_playback_status(dest) {
                if status.contains("Playing") {
                    let _ = Command::new("busctl")
                        .args([
                            "--user",
                            "call",
                            dest,
                            "/org/mpris/MediaPlayer2",
                            "org.mpris.MediaPlayer2.Player",
                            "Pause",
                        ])
                        .output();
                    newly_paused.push(dest.clone());
                }
            }
        }

        if !newly_paused.is_empty() {
            eprintln!("pixel-buds: auto-paused {:?}", newly_paused);
            self.auto_paused_players = newly_paused.clone();
            self.paused_at = Some(Instant::now());
            self.recent_playing_players = newly_paused;
            self.recent_playing_at = Some(Instant::now());
        } else if !self.auto_paused_players.is_empty() {
            // Second earbud was removed while playback was already auto-paused by the first bud
            self.paused_at = Some(Instant::now());
        } else if let Some(recent_at) = self.recent_playing_at {
            // Hardware AVRCP XF86AudioPause beat us by a few ms and already paused the player
            if recent_at.elapsed() <= RECENT_PLAYING_WINDOW {
                let mut hw_paused = Vec::new();
                for dest in &self.recent_playing_players {
                    if let Some(status) = Self::get_playback_status(dest) {
                        if status.contains("Paused") {
                            hw_paused.push(dest.clone());
                        }
                    }
                }
                if !hw_paused.is_empty() {
                    eprintln!(
                        "pixel-buds: recorded AVRCP auto-paused players {:?}",
                        hw_paused
                    );
                    self.auto_paused_players = hw_paused;
                    self.paused_at = Some(Instant::now());
                }
            }
        }
    }

    /// Called when an earbud transitions from out-of-ear to in-ear while In-Ear Detection (OHD) is enabled.
    pub fn on_bud_inserted(&mut self) {
        if self.auto_paused_players.is_empty() {
            return;
        }

        if let Some(at) = self.paused_at {
            if at.elapsed() > AUTO_RESUME_MAX_AGE {
                self.clear();
                return;
            }
        }

        // If any player is already playing (e.g. hardware AVRCP Play already resumed it,
        // or the user manually started playback), record it as playing and don't double-trigger.
        let mut currently_playing = Vec::new();
        for dest in Self::list_mpris_players() {
            if let Some(status) = Self::get_playback_status(&dest) {
                if status.contains("Playing") {
                    currently_playing.push(dest);
                }
            }
        }
        if !currently_playing.is_empty() {
            eprintln!(
                "pixel-buds: player {:?} already resumed on insertion",
                currently_playing
            );
            self.auto_paused_players.clear();
            self.paused_at = None;
            self.recent_playing_players = currently_playing;
            self.recent_playing_at = Some(Instant::now());
            return;
        }

        let to_resume = std::mem::take(&mut self.auto_paused_players);
        self.paused_at = None;
        let mut resumed = Vec::new();

        for dest in to_resume {
            if let Some(status) = Self::get_playback_status(&dest) {
                if status.contains("Paused") {
                    let _ = Command::new("busctl")
                        .args([
                            "--user",
                            "call",
                            &dest,
                            "/org/mpris/MediaPlayer2",
                            "org.mpris.MediaPlayer2.Player",
                            "Play",
                        ])
                        .output();
                    resumed.push(dest);
                }
            }
        }

        if !resumed.is_empty() {
            eprintln!("pixel-buds: auto-resumed {:?}", resumed);
            self.recent_playing_players = resumed;
            self.recent_playing_at = Some(Instant::now());
        }
    }
}
