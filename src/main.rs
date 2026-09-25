#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod i18n;
mod pane;
mod ssh;
mod terminal;
mod theme;
mod update;

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
    // The sidebar logo.
    fonts.font_data.insert("metal".to_owned(), Arc::new(FontData::from_static(include_bytes!("../assets/fonts/MetalMania-Regular.ttf"))));
    fonts.families.insert(FontFamily::Name("metal".into()), vec!["metal".to_owned()]);
    // Last-resort fallback for the UI: egui's proportional font lacks the ⇧ ⌘ ⌥ key symbols.
    if let Some(ui_family) = fonts.families.get_mut(&FontFamily::Proportional) {
        ui_family.push("mono".to_owned());
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
    let session = config::load::<config::Session>(config::session_path()).unwrap_or_default();
    let config = config::load_config();
    let theme = theme::Preset::find(config.as_ref().map_or(theme::DEFAULT_THEME, |c| &c.settings.theme)).theme();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Ronnie")
        .with_inner_size([1100.0, 700.0])
        .with_min_inner_size([400.0, 240.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon/icon.png")) {
        viewport = viewport.with_icon(icon);
    }
    // Reopen the window where it was left.
    if let Some(w) = session.window {
        viewport = viewport
            .with_inner_size([w.width.max(400.0), w.height.max(240.0)])
            .with_position([w.x, w.y])
            .with_maximized(w.maximized)
            .with_fullscreen(w.fullscreen);
    }
    if cfg!(target_os = "macos") {
        // Merge the title bar into the tab bar, keeping the traffic lights.
        viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    }

    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "Ronnie",
        options,
        Box::new(move |cc| {
            #[cfg(target_os = "macos")]
            tune_macos_window(cc, theme.chrome_bg);
            install_fonts(&cc.egui_ctx);
            cc.egui_ctx.set_visuals(theme.visuals());
            Ok(Box::new(app::App::new(cc, session, config, theme)))
        }),
    )
}
