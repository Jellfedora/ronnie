//! macOS: the remote commands (media keys, Control Center, headphones) and the "now playing" info.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSDictionary, NSNumber, NSString};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPMediaItemPropertyAlbumTitle, MPMediaItemPropertyArtist, MPMediaItemPropertyPlaybackDuration, MPMediaItemPropertyTitle, MPNowPlayingInfoCenter,
    MPNowPlayingInfoPropertyElapsedPlaybackTime, MPNowPlayingInfoPropertyPlaybackRate, MPNowPlayingPlaybackState, MPRemoteCommand, MPRemoteCommandCenter, MPRemoteCommandEvent,
    MPRemoteCommandHandlerStatus,
};

use super::{Command, NowPlaying, Sender};

pub struct Imp {
    /// Each command and the target added to it, removed when dropped.
    targets: Vec<(Retained<MPRemoteCommand>, Retained<AnyObject>)>,
}

impl Imp {
    pub fn start(sender: Sender, _window: Option<raw_window_handle::RawWindowHandle>) -> Result<Self, String> {
        // SAFETY: the shared command center, used on the main thread (where the app runs); the handlers
        // are called there too and only send to a channel.
        unsafe {
            let center = MPRemoteCommandCenter::sharedCommandCenter();
            let mut targets = Vec::new();
            let simple = [
                (center.togglePlayPauseCommand(), Command::Toggle),
                (center.playCommand(), Command::Play),
                (center.pauseCommand(), Command::Pause),
                (center.nextTrackCommand(), Command::Next),
                (center.previousTrackCommand(), Command::Previous),
                (center.stopCommand(), Command::Stop),
            ];
            for (command, what) in simple {
                let sender = sender.clone();
                let handler = RcBlock::new(move |_event: NonNull<MPRemoteCommandEvent>| {
                    sender.send(what);
                    MPRemoteCommandHandlerStatus::Success
                });
                command.setEnabled(true);
                let target = command.addTargetWithHandler(&handler);
                targets.push((command, target));
            }
            // The bar of the player in Control Center, dragged.
            let position = center.changePlaybackPositionCommand();
            let handler = RcBlock::new(move |event: NonNull<MPRemoteCommandEvent>| {
                let event = event.as_ref();
                match event.downcast_ref::<MPChangePlaybackPositionCommandEvent>() {
                    Some(event) => {
                        sender.send(Command::Seek(event.positionTime()));
                        MPRemoteCommandHandlerStatus::Success
                    }
                    None => MPRemoteCommandHandlerStatus::CommandFailed,
                }
            });
            position.setEnabled(true);
            let target = position.addTargetWithHandler(&handler);
            targets.push((Retained::into_super(position), target));
            Ok(Self { targets })
        }
    }

    pub fn show(&mut self, song: &NowPlaying) {
        // SAFETY: the shared info center, on the main thread; the keys and values are of the types it
        // documents (strings, numbers).
        unsafe {
            let keys: [&NSString; 6] = [MPMediaItemPropertyTitle, MPMediaItemPropertyArtist, MPMediaItemPropertyAlbumTitle, MPMediaItemPropertyPlaybackDuration, MPNowPlayingInfoPropertyElapsedPlaybackTime, MPNowPlayingInfoPropertyPlaybackRate];
            let values: [Retained<AnyObject>; 6] = [
                Retained::into_super(Retained::into_super(NSString::from_str(&song.title))),
                Retained::into_super(Retained::into_super(NSString::from_str(&song.artist))),
                Retained::into_super(Retained::into_super(NSString::from_str(&song.album))),
                Retained::into_super(Retained::into_super(Retained::into_super(NSNumber::new_f64(song.duration)))),
                Retained::into_super(Retained::into_super(Retained::into_super(NSNumber::new_f64(song.elapsed)))),
                Retained::into_super(Retained::into_super(Retained::into_super(NSNumber::new_f64(if song.paused { 0.0 } else { 1.0 })))),
            ];
            let values: Vec<&AnyObject> = values.iter().map(|v| &**v).collect();
            let info = NSDictionary::from_slices(&keys, &values);
            let center = MPNowPlayingInfoCenter::defaultCenter();
            center.setNowPlayingInfo(Some(&info));
            center.setPlaybackState(if song.paused { MPNowPlayingPlaybackState::Paused } else { MPNowPlayingPlaybackState::Playing });
        }
    }
}

impl Drop for Imp {
    fn drop(&mut self) {
        // SAFETY: as above.
        unsafe {
            for (command, target) in &self.targets {
                command.removeTarget(Some(target));
                command.setEnabled(false);
            }
            let center = MPNowPlayingInfoCenter::defaultCenter();
            center.setNowPlayingInfo(None);
            center.setPlaybackState(MPNowPlayingPlaybackState::Stopped);
        }
    }
}
