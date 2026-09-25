//! macOS menu bar: Ronnie's app menu (about, settings, reload, hide, quit) and a Window menu.
//! There is deliberately no Edit menu: its items would take Cmd+C / Cmd+V away from the terminal.

use std::sync::mpsc::{self, Receiver};

use muda::accelerator::Accelerator;
use muda::{AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

use crate::config::Shortcut;
use crate::i18n::Strings;

pub enum MenuAction {
    Settings,
    Reload,
    Quit,
}

pub struct MenuBar {
    /// Kept alive: dropping it would empty the menu bar.
    _menu: Menu,
    settings: MenuItem,
    reload: MenuItem,
    quit: MenuItem,
    events: Receiver<MenuEvent>,
    /// What the items currently show, to update them only on change.
    shown: (&'static str, String),
}

impl MenuBar {
    /// Replaces the default menu bar. Clicks wake the UI up through `ctx`.
    pub fn install(ctx: &egui::Context, t: &'static Strings, settings_shortcut: &Shortcut) -> Option<Self> {
        let settings = MenuItem::new(format!("{}…", t.settings), true, accelerator(settings_shortcut));
        let reload = MenuItem::new(t.reload_app, true, None);
        let quit = MenuItem::new(t.quit_app, true, "Cmd+Q".parse().ok());
        let about = AboutMetadata { name: Some("Ronnie".into()), version: Some(crate::update::VERSION.into()), ..Default::default() };
        let app_menu = Submenu::with_items(
            "Ronnie",
            true,
            &[
                &PredefinedMenuItem::about(Some(t.about_app), Some(about)),
                &PredefinedMenuItem::separator(),
                &settings,
                &reload,
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &quit,
            ],
        )
        .ok()?;
        let window_menu = Submenu::with_items(
            t.window_menu,
            true,
            &[
                &PredefinedMenuItem::minimize(None),
                &PredefinedMenuItem::maximize(None),
                &PredefinedMenuItem::fullscreen(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::bring_all_to_front(None),
            ],
        )
        .ok()?;
        let menu = Menu::with_items(&[&app_menu, &window_menu]).ok()?;
        menu.init_for_nsapp();

        let (tx, events) = mpsc::channel();
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event| {
            let _ = tx.send(event);
            ctx.request_repaint();
        }));
        Some(Self { _menu: menu, settings, reload, quit, events, shown: (t.settings, settings_shortcut.0.clone()) })
    }

    /// The menu items clicked since the last call.
    pub fn actions(&self) -> Vec<MenuAction> {
        self.events
            .try_iter()
            .filter_map(|e| match e.id {
                id if id == *self.settings.id() => Some(MenuAction::Settings),
                id if id == *self.reload.id() => Some(MenuAction::Reload),
                id if id == *self.quit.id() => Some(MenuAction::Quit),
                _ => None,
            })
            .collect()
    }

    /// Follows the language and the settings shortcut.
    pub fn sync(&mut self, t: &'static Strings, settings_shortcut: &Shortcut) {
        if self.shown.0 == t.settings && self.shown.1 == settings_shortcut.0 {
            return;
        }
        self.settings.set_text(format!("{}…", t.settings));
        let _ = self.settings.set_accelerator(accelerator(settings_shortcut));
        self.reload.set_text(t.reload_app);
        self.quit.set_text(t.quit_app);
        self.shown = (t.settings, settings_shortcut.0.clone());
    }
}

fn accelerator(shortcut: &Shortcut) -> Option<Accelerator> {
    shortcut.0.parse().ok()
}
