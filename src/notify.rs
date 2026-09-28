//! System notifications (a long command finished while Ronnie wasn't looked at).
//!
//! macOS: the Notification Center, as Ronnie.app (the system asks the user once). A build run outside the
//! app bundle (cargo run) has no identity for it and goes through osascript instead.
//! Linux: `notify-send` (libnotify), when installed. Windows: nothing here, the taskbar button flashes.

/// Shows a notification; failures are only logged (notifications are a convenience).
pub fn send(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    macos::send(title, body);
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let spawned = std::process::Command::new("notify-send")
            .args(["--app-name=Ronnie", "--icon=ronnie", "--", title, body])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match spawned {
            // Reaped in the background: a notification daemon may take a moment to answer.
            Ok(mut child) => drop(std::thread::Builder::new().name("notify-send".into()).spawn(move || child.wait())),
            Err(e) => crate::log::info(&format!("notify-send: {e}")),
        }
    }
    #[cfg(windows)]
    let _ = (title, body);
}

#[cfg(target_os = "macos")]
mod macos {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSBundle, NSError, NSString};
    use objc2_user_notifications::{UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest, UNNotificationSound, UNUserNotificationCenter};

    pub fn send(title: &str, body: &str) {
        // The Notification Center only serves apps with a bundle identifier (it throws otherwise).
        if NSBundle::mainBundle().bundleIdentifier().is_none() {
            return osascript(title, body);
        }
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        content.setSound(Some(&UNNotificationSound::defaultSound()));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&NSString::from_str(&uuid::Uuid::new_v4().to_string()), &content, None);
        let center = UNUserNotificationCenter::currentNotificationCenter();
        // Asks the user the first time; afterwards answers at once with their choice.
        let handler = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
            if granted.as_bool() {
                UNUserNotificationCenter::currentNotificationCenter().addNotificationRequest_withCompletionHandler(&request, None);
            }
        });
        center.requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound, &handler);
    }

    /// Development builds (not in Ronnie.app): the notification comes from the script runner.
    fn osascript(title: &str, body: &str) {
        let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
        let script = format!("display notification {} with title {}", quote(body), quote(title));
        if let Ok(mut child) = std::process::Command::new("osascript").args(["-e", &script]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
            drop(std::thread::Builder::new().name("osascript".into()).spawn(move || child.wait()));
        }
    }
}
