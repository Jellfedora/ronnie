//! Windows: the system media transport controls (media keys, the volume flyout's player), attached to
//! the main window.

use windows::core::{HSTRING, Ref};
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs, SystemMediaTransportControls, SystemMediaTransportControlsButton, SystemMediaTransportControlsButtonPressedEventArgs,
    SystemMediaTransportControlsTimelineProperties,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;

use super::{Command, NowPlaying, Sender};

pub struct Imp {
    controls: SystemMediaTransportControls,
    tokens: [i64; 2],
}

/// Seconds, in the 100 ns ticks of a TimeSpan.
fn span(secs: f64) -> TimeSpan {
    TimeSpan { Duration: (secs * 10_000_000.0) as i64 }
}

impl Imp {
    pub fn start(sender: Sender, window: Option<raw_window_handle::RawWindowHandle>) -> Result<Self, String> {
        let Some(raw_window_handle::RawWindowHandle::Win32(handle)) = window else { return Err("no window".into()) };
        let hwnd = HWND(handle.hwnd.get() as *mut core::ffi::c_void);
        let start = || -> windows::core::Result<Self> {
            let interop = windows::core::factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
            // SAFETY: the app's own, live window.
            let controls: SystemMediaTransportControls = unsafe { interop.GetForWindow(hwnd)? };
            controls.SetIsEnabled(true)?;
            controls.SetIsPlayEnabled(true)?;
            controls.SetIsPauseEnabled(true)?;
            controls.SetIsNextEnabled(true)?;
            controls.SetIsPreviousEnabled(true)?;
            controls.SetIsStopEnabled(true)?;
            let buttons = sender.clone();
            let pressed = controls.ButtonPressed(&TypedEventHandler::new(move |_, args: Ref<SystemMediaTransportControlsButtonPressedEventArgs>| {
                if let Some(args) = args.as_ref() {
                    let command = match args.Button()? {
                        SystemMediaTransportControlsButton::Play => Some(Command::Play),
                        SystemMediaTransportControlsButton::Pause => Some(Command::Pause),
                        SystemMediaTransportControlsButton::Next => Some(Command::Next),
                        SystemMediaTransportControlsButton::Previous => Some(Command::Previous),
                        SystemMediaTransportControlsButton::Stop => Some(Command::Stop),
                        _ => None,
                    };
                    if let Some(command) = command {
                        buttons.send(command);
                    }
                }
                Ok(())
            }))?;
            let moved = controls.PlaybackPositionChangeRequested(&TypedEventHandler::new(move |_, args: Ref<PlaybackPositionChangeRequestedEventArgs>| {
                if let Some(args) = args.as_ref() {
                    sender.send(Command::Seek(args.RequestedPlaybackPosition()?.Duration as f64 / 10_000_000.0));
                }
                Ok(())
            }))?;
            Ok(Self { controls, tokens: [pressed, moved] })
        };
        start().map_err(|e| e.to_string())
    }

    pub fn show(&mut self, song: &NowPlaying) {
        let show = || -> windows::core::Result<()> {
            let updater = self.controls.DisplayUpdater()?;
            updater.SetType(MediaPlaybackType::Music)?;
            let music = updater.MusicProperties()?;
            music.SetTitle(&HSTRING::from(&song.title))?;
            music.SetArtist(&HSTRING::from(&song.artist))?;
            music.SetAlbumTitle(&HSTRING::from(&song.album))?;
            updater.Update()?;
            self.controls.SetPlaybackStatus(if song.paused { MediaPlaybackStatus::Paused } else { MediaPlaybackStatus::Playing })?;
            let timeline = SystemMediaTransportControlsTimelineProperties::new()?;
            timeline.SetStartTime(span(0.0))?;
            timeline.SetMinSeekTime(span(0.0))?;
            timeline.SetEndTime(span(song.duration))?;
            timeline.SetMaxSeekTime(span(song.duration))?;
            timeline.SetPosition(span(song.elapsed))?;
            self.controls.UpdateTimelineProperties(&timeline)
        };
        if let Err(e) = show() {
            crate::log::error(&format!("media keys: {e}"));
        }
    }
}

impl Drop for Imp {
    fn drop(&mut self) {
        let _ = self.controls.RemoveButtonPressed(self.tokens[0]);
        let _ = self.controls.RemovePlaybackPositionChangeRequested(self.tokens[1]);
        if let Ok(updater) = self.controls.DisplayUpdater() {
            let _ = updater.ClearAll();
            let _ = updater.Update();
        }
        let _ = self.controls.SetPlaybackStatus(MediaPlaybackStatus::Closed);
        let _ = self.controls.SetIsEnabled(false);
    }
}
