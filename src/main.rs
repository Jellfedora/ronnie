#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod askpass;
mod blob;
mod awake;
mod claude;
mod config;
mod connect4;
mod db;
mod dragout;
mod floor;
mod i18n;
mod live;
mod log;
mod media_keys;
mod notify;
#[cfg(target_os = "macos")]
mod menu;
mod mssql;
mod subsonic;
mod pane;
mod screens;
mod sftp;
mod shell;
mod ssh;
mod terminal;
mod theme;
mod update;
mod voice;
#[cfg(windows)]
mod winproc;

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

/// First one found is used as a fallback for CJK characters (not bundled: they weigh several MB).
const SYSTEM_CJK_FONTS: &[&str] = &[
    "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "C:\\Windows\\Fonts\\msgothic.ttc",
    "C:\\Windows\\Fonts\\msyh.ttc",
];

/// The notes' faces (regular, bold, italic, bold italic) on each system: font file and the face's
/// index in it. The first set found is used.
const NOTE_FONTS: &[[(&str, u32); 4]] = &[
    [("/System/Library/Fonts/HelveticaNeue.ttc", 0), ("/System/Library/Fonts/HelveticaNeue.ttc", 1), ("/System/Library/Fonts/HelveticaNeue.ttc", 2), ("/System/Library/Fonts/HelveticaNeue.ttc", 3)],
    [("C:\\Windows\\Fonts\\segoeui.ttf", 0), ("C:\\Windows\\Fonts\\segoeuib.ttf", 0), ("C:\\Windows\\Fonts\\segoeuii.ttf", 0), ("C:\\Windows\\Fonts\\segoeuiz.ttf", 0)],
    [("/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf", 0), ("/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf", 0), ("/usr/share/fonts/truetype/noto/NotoSans-Italic.ttf", 0), ("/usr/share/fonts/truetype/noto/NotoSans-BoldItalic.ttf", 0)],
    [("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 0), ("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", 0), ("/usr/share/fonts/truetype/dejavu/DejaVuSans-Oblique.ttf", 0), ("/usr/share/fonts/truetype/dejavu/DejaVuSans-BoldOblique.ttf", 0)],
    [("/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf", 0), ("/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf", 0), ("/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Oblique.ttf", 0), ("/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-BoldOblique.ttf", 0)],
];

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let faces: [(&str, &'static [u8]); 4] = [
        ("mono", include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf")),
        ("mono-bold", include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf")),
        ("mono-italic", include_bytes!("../assets/fonts/JetBrainsMono-Italic.ttf")),
        ("mono-bold-italic", include_bytes!("../assets/fonts/JetBrainsMono-BoldItalic.ttf")),
    ];
    // Fallbacks: egui's bundled fonts for symbols and emoji, then a system CJK font if present.
    let mut fallbacks: Vec<String> = fonts.families[&FontFamily::Monospace].clone();
    if let Some(bytes) = SYSTEM_CJK_FONTS.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts.font_data.insert("system-cjk".to_owned(), Arc::new(FontData::from_owned(bytes)));
        fallbacks.push("system-cjk".to_owned());
    }
    for (name, bytes) in faces {
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
        let mut chain = vec![name.to_owned()];
        chain.extend(fallbacks.iter().cloned());
        fonts.families.insert(FontFamily::Name(name.into()), chain);
    }
    // The notes' text: the system's sans serif, with real bold and italics (JetBrains Mono's faces
    // when it has none of these).
    let note_faces = ["note", "note-bold", "note-italic", "note-bold-italic"];
    if let Some(set) = NOTE_FONTS.iter().find(|set| set.iter().all(|(path, _)| std::path::Path::new(path).is_file())) {
        let mut read: Vec<(&str, &'static [u8])> = Vec::new();
        for (name, (path, index)) in note_faces.iter().zip(set) {
            let bytes = match read.iter().find(|(p, _)| p == path) {
                Some((_, b)) => *b,
                None => {
                    let Ok(bytes) = std::fs::read(path) else { continue };
                    // Read once for the faces it holds, and kept for the whole run.
                    let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
                    read.push((path, bytes));
                    bytes
                }
            };
            let mut data = FontData::from_static(bytes);
            data.index = *index;
            fonts.font_data.insert((*name).to_owned(), Arc::new(data));
            let mut chain = vec![(*name).to_owned()];
            chain.extend(fallbacks.iter().cloned());
            fonts.families.insert(FontFamily::Name((*name).into()), chain);
        }
    }
    for (name, mono) in note_faces.iter().zip(["mono", "mono-bold", "mono-italic", "mono-bold-italic"]) {
        let chain = fonts.families[&FontFamily::Name(mono.into())].clone();
        fonts.families.entry(FontFamily::Name((*name).into())).or_insert(chain);
    }
    // The sidebar logo.
    fonts.font_data.insert("metal".to_owned(), Arc::new(FontData::from_static(include_bytes!("../assets/fonts/MetalMania-Regular.ttf"))));
    fonts.families.insert(FontFamily::Name("metal".into()), vec!["metal".to_owned()]);
    // Key symbols (⌘ ⇧ ⌥ ⌃ ⌫ ↩ arrows): egui's fonts lack some and draw others tiny, JetBrains Mono has
    // them all. It comes right after the UI font, before the emoji fonts, and is the UI's monospace font.
    if let Some(ui_family) = fonts.families.get_mut(&FontFamily::Proportional) {
        ui_family.insert(1.min(ui_family.len()), "mono".to_owned());
    }
    if let Some(mono_family) = fonts.families.get_mut(&FontFamily::Monospace) {
        mono_family.insert(0, "mono".to_owned());
    }
    ctx.set_fonts(fonts);
}

#[cfg(target_os = "macos")]
fn tune_macos_window(cc: &eframe::CreationContext<'_>, bg: egui::Color32) {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{msg_send, ClassType};
    use objc2_foundation::{ns_string, NSArray, NSDictionary, NSNull, NSString};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = cc.window_handle() else { return };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else { return };
    // SAFETY: the handle points to the live NSView of our window, on the main thread.
    let view: &objc2_app_kit::NSView = unsafe { appkit.ns_view.cast().as_ref() };

    if let Some(window) = view.window() {
        // With the title bar merged into the content, macOS moves the window when dragging anywhere in
        // the top strip, which would swallow tab drags. The app starts window drags itself instead.
        window.setMovable(false);
        // Shown for an instant in areas not yet redrawn while resizing.
        let [r, g, b, _] = bg.to_normalized_gamma_f32();
        let (r, g, b): (f64, f64, f64) = (r.into(), g.into(), b.into());
        // SAFETY: standard NSColor constructor, returns an autoreleased color we retain.
        let color: Retained<objc2_app_kit::NSColor> = unsafe {
            msg_send![objc2_app_kit::NSColor::class(), colorWithSRGBRed: r, green: g, blue: b, alpha: 1.0f64]
        };
        window.setBackgroundColor(Some(&color));
    }

    // Zoom (maximize, double-click on the bar, tiling) animates the window frame without letting the
    // app redraw, so the last frame is shown stretched until the animation ends. Make it instant.
    // SAFETY: registers an app-level default; NSUserDefaults is thread-safe.
    unsafe {
        let defaults: Retained<AnyObject> = msg_send![objc2::class!(NSUserDefaults), standardUserDefaults];
        let duration: Retained<AnyObject> = msg_send![objc2::class!(NSNumber), numberWithDouble: 0.001f64];
        let values = NSDictionary::from_slices(&[ns_string!("NSWindowResizeTime")], &[&*duration]);
        let _: () = msg_send![&*defaults, registerDefaults: &*values];
    }

    // wgpu renders into a CAMetalLayer added as a sublayer of the view's layer. Unlike a view's own
    // layer, a sublayer animates every bounds change (~0.25s), so text visibly scales for a moment
    // while the window is resized. Disable those implicit animations.
    // SAFETY: plain CALayer messages on layers owned by our view, on the main thread.
    unsafe {
        let root: Option<Retained<AnyObject>> = msg_send![view, layer];
        let Some(root) = root else { return };
        let sublayers: Option<Retained<NSArray<AnyObject>>> = msg_send![&*root, sublayers];
        let null = NSNull::null();
        let keys: [&NSString; 5] =
            [ns_string!("bounds"), ns_string!("position"), ns_string!("frame"), ns_string!("contents"), ns_string!("contentsScale")];
        let values: [&AnyObject; 5] = [&null; 5];
        let actions = NSDictionary::from_slices(&keys, &values);
        for layer in sublayers.iter().flat_map(|l| l.iter()) {
            let _: () = msg_send![&*layer, setActions: &*actions];
        }
    }
}

fn main() -> eframe::Result {
    // Started by ssh to type a saved password: answer and exit before any window opens.
    ssh::run_askpass();
    if std::env::args().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("ronnie {}{}", update::VERSION, if config::OFFICIAL { "" } else { " (dev)" });
        return Ok(());
    }
    if std::env::args().skip(1).any(|a| a == "--help" || a == "-h") {
        println!("Ronnie {} — Terminal Power Métal\n\nUsage: ronnie [--version | --help]\n\nEnvironment:\n  RONNIE_CONFIG_DIR   use another config directory\n  RONNIE_UPDATE_CHECK look for updates even in a dev build", update::VERSION);
        return Ok(());
    }
    log::install_panic_hook();
    // Claude Code's status line command: keeps the plan's usage for the panes where Claude runs.
    if std::env::args().nth(1).as_deref() == Some("claude-statusline") {
        claude::run_statusline();
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    update::integrate_appimage();
    log::info(&format!("start {}{}", update::VERSION, if config::OFFICIAL { "" } else { " dev" }));
    let (session, session_error) = config::load_session();
    let config = config::load_config();
    let theme = theme::Preset::find(config.as_ref().map_or(theme::DEFAULT_THEME, |c| &c.settings.theme)).theme();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(if config::OFFICIAL { "Ronnie" } else { "Ronnie (dev)" })
        // Linux: what ties the window to ronnie.desktop (its icon in the dock), Wayland and X11.
        .with_app_id(update::APP_ID)
        .with_inner_size([1100.0, 700.0])
        .with_min_inner_size([400.0, 240.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon/icon.png")) {
        viewport = viewport.with_icon(icon);
    }
    // Reopen the window where it was left.
    if let Some(w) = session.window {
        viewport = viewport.with_inner_size([w.width.max(400.0), w.height.max(240.0)]).with_maximized(w.maximized).with_fullscreen(w.fullscreen);
        // Left on a screen unplugged since: the system places it instead.
        if screens::on_screen(&w) {
            viewport = viewport.with_position([w.x, w.y]);
        }
    }
    if cfg!(target_os = "macos") {
        // Merge the title bar into the tab bar, keeping the traffic lights.
        viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    }

    let options = eframe::NativeOptions { viewport, ..Default::default() };
    let result = eframe::run_native(
        "Ronnie",
        options,
        Box::new(move |cc| {
            #[cfg(target_os = "macos")]
            tune_macos_window(cc, theme.chrome_bg);
            install_fonts(&cc.egui_ctx);
            cc.egui_ctx.set_visuals(theme.visuals());
            Ok(Box::new(app::App::new(cc, session, session_error, config, theme)))
        }),
    );
    // The windows are closed and their terminals dropped: what ran in them ends before Ronnie does.
    terminal::finish_ending();
    result
}
