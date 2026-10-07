//! System notifications (a long command finished while Ronnie wasn't looked at).
//!
//! macOS: the Notification Center, as Ronnie.app (the system asks the user once). A build run outside the
//! app bundle (cargo run) has no identity for it and goes through osascript instead.
//! Linux: `notify-send` (libnotify), when installed. Windows: a toast, under an app identity Ronnie registers
//! for itself (no installer makes a Start menu shortcut for it).

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
    windows::send(title, body);
}

#[cfg(windows)]
mod windows {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    /// The AppUserModelID the toasts come from: its name and icon are read from the registry.
    const APP_ID: &str = "Ronnie.Terminal";

    pub fn send(title: &str, body: &str) {
        let (title, body) = (title.to_owned(), body.to_owned());
        let _ = std::thread::Builder::new().name("toast".into()).spawn(move || {
            if let Err(e) = show(&title, &body) {
                crate::log::info(&format!("toast: {e}"));
            }
        });
    }

    fn show(title: &str, body: &str) -> windows::core::Result<()> {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        register();
        let escape = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
        let xml = format!(
            "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
            escape(title),
            escape(body)
        );
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?.Show(&toast)
    }

    /// Windows only shows toasts of an app it knows: HKCU\Software\Classes\AppUserModelId\<id> gives
    /// it the name and icon (a PNG next to the config). Done once per run.
    fn register() {
        static DONE: std::sync::Once = std::sync::Once::new();
        DONE.call_once(|| {
            use windows_sys::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
            let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
            let key = wide(&format!("Software\\Classes\\AppUserModelId\\{APP_ID}"));
            let set = |name: &str, value: &str| {
                let value = wide(value);
                let status = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), wide(name).as_ptr(), REG_SZ, value.as_ptr().cast(), (value.len() * 2) as u32) };
                if status != 0 {
                    crate::log::info(&format!("toast: registry {name}: error {status}"));
                }
            };
            set("DisplayName", "Ronnie");
            if let Some(dir) = crate::config::config_dir() {
                let icon = dir.join("notification-icon.png");
                if !icon.exists() {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = std::fs::write(&icon, include_bytes!("../assets/icon/icon.png"));
                }
                set("IconUri", &icon.to_string_lossy());
            }
        });
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, NSObject, ProtocolObject};
    use objc2::{define_class, msg_send, AllocAnyThread};
    use objc2_foundation::{NSBundle, NSError, NSObjectProtocol, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationSound,
        UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };

    define_class!(
        // SAFETY: NSObject has no subclassing requirements, and this class doesn't implement Drop.
        #[unsafe(super(NSObject))]
        #[name = "RonnieNotificationDelegate"]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            // Without it, the system keeps quiet about the notifications of the app in front (the
            // settings' "Test" button, a command done in another tab).
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(&self, _center: &UNUserNotificationCenter, _notification: &UNNotification, completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>) {
                completion.call((UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List | UNNotificationPresentationOptions::Sound,));
            }
        }
    );

    /// The center only holds its delegate weakly: this one lives as long as Ronnie.
    fn set_delegate(center: &UNUserNotificationCenter) {
        static DONE: std::sync::Once = std::sync::Once::new();
        DONE.call_once(|| {
            let this = Delegate::alloc().set_ivars(());
            // SAFETY: NSObject's init.
            let delegate: Retained<Delegate> = unsafe { msg_send![super(this), init] };
            center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            std::mem::forget(delegate);
        });
    }

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
        set_delegate(&center);
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
