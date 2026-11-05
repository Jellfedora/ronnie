use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor};
use egui::Color32;

/// Palette used both for terminal colors and for the surrounding UI.
#[derive(Clone)]
pub struct Theme {
    pub fg: Color32,
    pub bg: Color32,
    pub cursor: Color32,
    pub selection: Color32,
    pub ansi: [Color32; 16],

    pub chrome_bg: Color32,
    pub tab_hover: Color32,
    pub tab_active: Color32,
    pub text_muted: Color32,
    pub text: Color32,
    pub accent: Color32,
    pub dark: bool,
    /// Ronnie Métal: the focused pane glows, panes flash, the cursors are drawn (see `METAL_THEME`).
    pub metal: bool,
    /// Jurassic Park: safety stripes, the park's logo, ferns (see `app::jurassic`).
    pub jurassic: bool,
    /// Hyperspace: laser beams, a blade around the focused pane, stars (see `app::hyperspace`).
    pub hyperspace: bool,
    /// Open space: post-its, a coffee machine, a plant, « T'as 5 minutes ? » at the bell (see
    /// `app::office`).
    pub office: bool,
    /// Gryffindor: a wand for the pointer, the house's scarf, the castle at night (see `app::wizard`).
    pub wizard: bool,
}

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Colors a tab can be tagged with.
pub const TAB_COLORS: [Color32; 8] = [
    hex(0xf7768e),
    hex(0xff9e64),
    hex(0xe0af68),
    hex(0x9ece6a),
    hex(0x73daca),
    hex(0x7aa2f7),
    hex(0xbb9af7),
    hex(0xc0caf5),
];

/// A built-in theme, as listed in the settings.
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    bg: u32,
    fg: u32,
    cursor: u32,
    /// Sidebar background.
    chrome: u32,
    /// Hovered rows and controls.
    surface: u32,
    /// The theme's signature color: active tab, focused pane, selection, links.
    accent: u32,
    ansi: [u32; 16],
    pub dark: bool,
}

pub const DEFAULT_THEME: &str = "dracula";

/// The theme unlocked by the typing game.
pub const METAL_THEME: &str = "ronnie-metal";
/// The theme with the park's own touches, unlocked by Ronnie.io.
pub const JURASSIC_THEME: &str = "jurassic-park";
/// Mass to reach in a round of Ronnie.io to unlock it.
pub const JURASSIC_UNLOCK: u32 = 4_000;
/// The space theme, unlocked at Larsen: beating the computer at its hardest, or someone online.
pub const HYPERSPACE_THEME: &str = "hyperspace";
/// The office theme, unlocked at Riff Hero: `OFFICE_UNLOCK` points in a song.
pub const OFFICE_THEME: &str = "open-space";
pub const OFFICE_UNLOCK: u32 = 30_000;

/// Whether a song of Riff Hero (any level) reached `OFFICE_UNLOCK`.
pub fn office_unlocked(riff_bests: &[u32]) -> bool {
    riff_bests.iter().any(|&b| b >= OFFICE_UNLOCK)
}
/// Gryffindor, won by using up Claude's weekly quota.
pub const WIZARD_THEME: &str = "gryffondor";
/// The themes with their own touches, most won in the games, shown apart in the settings.
pub const SPECIAL_THEMES: [&str; 5] = [METAL_THEME, JURASSIC_THEME, HYPERSPACE_THEME, OFFICE_THEME, WIZARD_THEME];
/// Letters to reach in a round of the typing game to unlock it.
pub const METAL_UNLOCK: u32 = 300;

pub const PRESETS: &[Preset] = &[
    // The house theme: deep black, blood red, old gold and bone-colored text.
    Preset {
        id: "ronnie",
        name: "Ronnie",
        bg: 0x1b1819,
        fg: 0xefe8de,
        cursor: 0xd9a93f,
        chrome: 0x141112,
        surface: 0x322a2c,
        accent: 0xd8323c,
        ansi: [
            0x2a2325, 0xd8323c, 0x8fb35a, 0xd9a93f, 0x6f8fc4, 0xb8528c, 0x5fa9a0, 0xd4cabd, //
            0x5c4b4d, 0xff5058, 0xa9d06c, 0xf2c55c, 0x8eaae2, 0xde72ae, 0x80cfc5, 0xfff7ec,
        ],
        dark: true,
    },
    // Unlocked by the typing game: black as a stage, a red that burns and the gold of the spotlights.
    Preset {
        id: METAL_THEME,
        name: "Ronnie Métal",
        bg: 0x0f0c0d,
        fg: 0xf2ebe0,
        cursor: 0xf2b632,
        chrome: 0x080607,
        surface: 0x2c1517,
        accent: 0xff2b3a,
        ansi: [
            0x1f1718, 0xe8202f, 0x9fd03c, 0xf2b632, 0x7f9bc0, 0x9b59d0, 0x4fb3a9, 0xd8cfc2, //
            0x5a4446, 0xff4a56, 0xbcf05a, 0xffd25c, 0xa3bde0, 0xbd84ee, 0x76d6cb, 0xfffaf0,
        ],
        dark: true,
    },
    Preset {
        id: "tokyo-night",
        name: "Tokyo Night",
        bg: 0x1a1b26,
        fg: 0xc0caf5,
        cursor: 0xc0caf5,
        chrome: 0x16161e,
        surface: 0x292e42,
        accent: 0x7aa2f7,
        ansi: [
            0x15161e, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6, //
            0x414868, 0xff899d, 0x9fe044, 0xfaba4a, 0x8db0ff, 0xc7a9ff, 0xa4daff, 0xc0caf5,
        ],
        dark: true,
    },
    Preset {
        id: "catppuccin-mocha",
        name: "Catppuccin Mocha",
        bg: 0x1e1e2e,
        fg: 0xcdd6f4,
        cursor: 0xf5e0dc,
        chrome: 0x11111b,
        surface: 0x313244,
        accent: 0xcba6f7,
        ansi: [
            0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xbac2de, //
            0x585b70, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8,
        ],
        dark: true,
    },
    Preset {
        id: "dracula",
        name: "Dracula",
        bg: 0x282a36,
        fg: 0xf8f8f2,
        cursor: 0xf8f8f2,
        chrome: 0x191a21,
        surface: 0x44475a,
        accent: 0xff79c6,
        ansi: [
            0x21222c, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd, 0xf8f8f2, //
            0x6272a4, 0xff6e6e, 0x69ff94, 0xffffa5, 0xd6acff, 0xff92df, 0xa4ffff, 0xffffff,
        ],
        dark: true,
    },
    Preset {
        id: "one-dark",
        name: "One Dark",
        bg: 0x282c34,
        fg: 0xabb2bf,
        cursor: 0x528bff,
        chrome: 0x1e2127,
        surface: 0x3a3f4b,
        accent: 0x61afef,
        ansi: [
            0x282c34, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xabb2bf, //
            0x5c6370, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xffffff,
        ],
        dark: true,
    },
    Preset {
        id: "nord",
        name: "Nord",
        bg: 0x2e3440,
        fg: 0xd8dee9,
        cursor: 0xd8dee9,
        chrome: 0x242933,
        surface: 0x434c5e,
        accent: 0x88c0d0,
        ansi: [
            0x3b4252, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x88c0d0, 0xe5e9f0, //
            0x4c566a, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x8fbcbb, 0xeceff4,
        ],
        dark: true,
    },
    Preset {
        id: "gruvbox-dark",
        name: "Gruvbox Dark",
        bg: 0x282828,
        fg: 0xebdbb2,
        cursor: 0xebdbb2,
        chrome: 0x1d2021,
        surface: 0x3c3836,
        accent: 0xfe8019,
        ansi: [
            0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984, //
            0x928374, 0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2,
        ],
        dark: true,
    },
    Preset {
        id: "solarized-dark",
        name: "Solarized Dark",
        bg: 0x002b36,
        fg: 0x93a1a1,
        cursor: 0x93a1a1,
        chrome: 0x00212b,
        surface: 0x073642,
        accent: 0x2aa198,
        ansi: [
            0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5, //
            0x586e75, 0xcb4b16, 0x93a1a1, 0x839496, 0x6c71c4, 0xd33682, 0x2aa198, 0xfdf6e3,
        ],
        dark: true,
    },
    Preset {
        id: "rose-pine",
        name: "Rosé Pine",
        bg: 0x191724,
        fg: 0xe0def4,
        cursor: 0xebbcba,
        chrome: 0x12101a,
        surface: 0x26233a,
        accent: 0xebbcba,
        ansi: [
            0x26233a, 0xeb6f92, 0x31748f, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xebbcba, 0xe0def4, //
            0x6e6a86, 0xeb6f92, 0x31748f, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xebbcba, 0xe0def4,
        ],
        dark: true,
    },
    Preset {
        id: "monokai",
        name: "Monokai",
        bg: 0x272822,
        fg: 0xf8f8f2,
        cursor: 0xf8f8f0,
        chrome: 0x1e1f1a,
        surface: 0x3e3d32,
        accent: 0xa6e22e,
        ansi: [
            0x272822, 0xf92672, 0xa6e22e, 0xf4bf75, 0x66d9ef, 0xae81ff, 0xa1efe4, 0xf8f8f2, //
            0x75715e, 0xf92672, 0xa6e22e, 0xf4bf75, 0x66d9ef, 0xae81ff, 0xa1efe4, 0xf9f8f5,
        ],
        dark: true,
    },
    // The park at night: jungle black and moss, khaki text, the amber of the logo's ring, the red of its
    // title for the cursor.
    Preset {
        id: JURASSIC_THEME,
        name: "Jurassic Park",
        bg: 0x10160f,
        fg: 0xe9e2c8,
        cursor: 0xe0301e,
        chrome: 0x0a0f09,
        surface: 0x26331f,
        accent: 0xf2a900,
        ansi: [
            0x1c2419, 0xd7261e, 0x6fae3a, 0xf2a900, 0x4f8fa8, 0xc0577a, 0x58a98e, 0xd8d0b4, //
            0x4a5a42, 0xff4a3a, 0x96d35a, 0xffc93c, 0x7fb6cc, 0xe07ea0, 0x7fd0b4, 0xfff6dc,
        ],
        dark: true,
    },
    // Star Wars: deep space, starlight text, a Jedi's blue blade for the accent, a Sith's red one for
    // the cursor and the alerts, the title's yellow.
    Preset {
        id: HYPERSPACE_THEME,
        name: "Star Wars",
        bg: 0x05070d,
        fg: 0xdfe7f2,
        cursor: 0xff2d2d,
        chrome: 0x020308,
        surface: 0x151b2c,
        accent: 0x3fa9ff,
        ansi: [
            0x101626, 0xff3b3b, 0x5ee36b, 0xffe81f, 0x3fa9ff, 0xb07cff, 0x5fe0ff, 0xc9d3e3, //
            0x3a4560, 0xff6b6b, 0x8cf595, 0xffe27a, 0x7cc4ff, 0xcfa6ff, 0x9aeeff, 0xffffff,
        ],
        dark: true,
    },
    // Gryffindor: the house's maroon (a burgundy leaning to purple) for the sidebar and the bars, a
    // darker one behind the text, cream text, mustard gold for the accent.
    Preset {
        id: WIZARD_THEME,
        name: "Gryffondor",
        bg: 0x34101d,
        fg: 0xf6e8cc,
        cursor: 0xf2a933,
        chrome: 0x5c1a2b,
        surface: 0x7c2a3c,
        accent: 0xf2a933,
        ansi: [
            0x4a1c24, 0xff3b47, 0x7fc25a, 0xf2c232, 0x6f93e0, 0xd46aa0, 0x5fc4bc, 0xefe2c6, //
            0x7a4048, 0xff6b72, 0xa2dc7c, 0xffd860, 0x95b3f0, 0xe692bf, 0x8adcd4, 0xfffaee,
        ],
        dark: true,
    },
    // Open space: a light office, beige walls and grey carpet, the corporate blue, post-it yellow.
    Preset {
        id: OFFICE_THEME,
        name: "Open space",
        bg: 0xf6f3ec,
        fg: 0x2e3440,
        cursor: 0x2f6fde,
        chrome: 0xe4ded2,
        surface: 0xd3ccbd,
        accent: 0x2f6fde,
        ansi: [
            0x3b4252, 0xc8414b, 0x3f8f4a, 0xd9a400, 0x2f6fde, 0x9b59b6, 0x168a8f, 0xcfc8ba, //
            0x6b7280, 0xe0565f, 0x52a85d, 0xf2c230, 0x5b8ff0, 0xb07cc9, 0x2aa7ad, 0xfaf8f2,
        ],
        dark: false,
    },
    Preset {
        id: "catppuccin-latte",
        name: "Catppuccin Latte",
        bg: 0xeff1f5,
        fg: 0x4c4f69,
        cursor: 0xdc8a78,
        chrome: 0xdce0e8,
        surface: 0xccd0da,
        accent: 0x8839ef,
        ansi: [
            0x5c5f77, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xacb0be, //
            0x6c6f85, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xbcc0cc,
        ],
        dark: false,
    },
    Preset {
        id: "github-light",
        name: "GitHub Light",
        bg: 0xffffff,
        fg: 0x1f2328,
        cursor: 0x0969da,
        chrome: 0xf6f8fa,
        surface: 0xe7ecf0,
        accent: 0x0969da,
        ansi: [
            0x24292f, 0xcf222e, 0x116329, 0x4d2d00, 0x0969da, 0x8250df, 0x1b7c83, 0x6e7781, //
            0x57606a, 0xa40e26, 0x1a7f37, 0x633c01, 0x218bff, 0xa475f9, 0x3192aa, 0x8c959f,
        ],
        dark: false,
    },
    Preset {
        id: "solarized-light",
        name: "Solarized Light",
        bg: 0xfdf6e3,
        fg: 0x586e75,
        cursor: 0x586e75,
        chrome: 0xeee8d5,
        surface: 0xe4ddc8,
        accent: 0xb58900,
        ansi: [
            0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5, //
            0x002b36, 0xcb4b16, 0x586e75, 0x657b83, 0x6c71c4, 0xd33682, 0x2aa198, 0xfdf6e3,
        ],
        dark: false,
    },
];

impl Preset {
    pub fn theme(&self) -> Theme {
        let (bg, fg, chrome, surface, accent) = (hex(self.bg), hex(self.fg), hex(self.chrome), hex(self.surface), hex(self.accent));
        Theme {
            fg,
            bg,
            cursor: hex(self.cursor),
            selection: Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), if self.dark { 80 } else { 70 }),
            ansi: self.ansi.map(hex),
            chrome_bg: chrome,
            tab_hover: mix(chrome, surface, 0.6),
            tab_active: surface,
            text_muted: mix(fg, chrome, 0.42),
            text: fg,
            accent,
            dark: self.dark,
            metal: self.id == METAL_THEME,
            jurassic: self.id == JURASSIC_THEME,
            hyperspace: self.id == HYPERSPACE_THEME,
            office: self.id == OFFICE_THEME,
            wizard: self.id == WIZARD_THEME,
        }
    }

    /// Still locked, with this best score at the typing game, and whether a round of Ronnie.io unlocked
    /// the park, whether Larsen unlocked hyperspace, and whether the fishing's binder is full (never in
    /// a dev build, to try them).
    pub fn locked(&self, best: u32, jurassic: bool, hyperspace: bool, office: bool, wizard: bool) -> bool {
        crate::config::OFFICIAL && ((self.id == METAL_THEME && best < METAL_UNLOCK) || (self.id == JURASSIC_THEME && !jurassic) || (self.id == HYPERSPACE_THEME && !hyperspace) || (self.id == OFFICE_THEME && !office) || (self.id == WIZARD_THEME && !wizard))
    }

    /// The preset with this id, or the default one.
    pub fn find(id: &str) -> &'static Preset {
        PRESETS.iter().find(|p| p.id == id).unwrap_or(&PRESETS[0])
    }
}

impl Default for Theme {
    fn default() -> Self {
        Preset::find(DEFAULT_THEME).theme()
    }
}

impl Theme {
    /// Gives egui the theme's styling. The system's light or dark mode must never show: egui is pinned to
    /// the theme's own mode instead of following it, and both its styles get the theme (`set_visuals`
    /// alone left the other one egui's default: light buttons on a dark theme with macOS in light mode).
    pub fn apply(&self, ctx: &egui::Context) {
        ctx.set_theme(if self.dark { egui::Theme::Dark } else { egui::Theme::Light });
        let v = self.visuals();
        ctx.set_visuals_of(egui::Theme::Dark, v.clone());
        ctx.set_visuals_of(egui::Theme::Light, v);
    }

    /// egui styling matching the theme, for menus, popups and dialogs.
    pub fn visuals(&self) -> egui::Visuals {
        let mut v = if self.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.panel_fill = self.chrome_bg;
        v.window_fill = self.chrome_bg;
        v.extreme_bg_color = self.bg;
        v.faint_bg_color = self.tab_hover;
        v.window_stroke.color = self.tab_hover;
        v.override_text_color = Some(self.text);
        v.selection.bg_fill = self.selection;
        v.selection.stroke.color = self.accent;
        v.hyperlink_color = self.accent;
        v.widgets.noninteractive.bg_stroke.color = self.tab_hover;
        v.widgets.inactive.weak_bg_fill = self.tab_hover;
        v.widgets.inactive.bg_fill = self.tab_hover;
        v.widgets.hovered.weak_bg_fill = mix(self.tab_hover, self.fg, 0.08);
        v.widgets.hovered.bg_fill = mix(self.tab_hover, self.fg, 0.08);
        v.widgets.active.weak_bg_fill = mix(self.tab_hover, self.fg, 0.14);
        v
    }
}

fn dim(c: Color32) -> Color32 {
    Color32::from_rgb(
        (c.r() as f32 * 0.66) as u8,
        (c.g() as f32 * 0.66) as u8,
        (c.b() as f32 * 0.66) as u8,
    )
}

impl Theme {
    fn indexed(&self, i: u8) -> Color32 {
        match i {
            0..=15 => self.ansi[i as usize],
            16..=231 => {
                let i = i - 16;
                let step = |v: u8| if v == 0 { 0 } else { v * 40 + 55 };
                Color32::from_rgb(step(i / 36), step((i / 6) % 6), step(i % 6))
            }
            _ => {
                let v = (i - 232) * 10 + 8;
                Color32::from_rgb(v, v, v)
            }
        }
    }

    fn named(&self, n: NamedColor) -> Color32 {
        use NamedColor::*;
        match n {
            Foreground | BrightForeground => self.fg,
            Background => self.bg,
            Cursor => self.cursor,
            DimForeground => dim(self.fg),
            DimBlack => dim(self.ansi[0]),
            DimRed => dim(self.ansi[1]),
            DimGreen => dim(self.ansi[2]),
            DimYellow => dim(self.ansi[3]),
            DimBlue => dim(self.ansi[4]),
            DimMagenta => dim(self.ansi[5]),
            DimCyan => dim(self.ansi[6]),
            DimWhite => dim(self.ansi[7]),
            other => self.ansi[(other as usize).min(15)],
        }
    }

    /// Resolves a terminal color, honoring palette overrides set by the running program (OSC 4/10/11).
    pub fn resolve(&self, color: Color, overrides: &Colors) -> Color32 {
        match color {
            Color::Spec(rgb) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
            Color::Named(n) => overrides[n]
                .map(|rgb| Color32::from_rgb(rgb.r, rgb.g, rgb.b))
                .unwrap_or_else(|| self.named(n)),
            Color::Indexed(i) => overrides[i as usize]
                .map(|rgb| Color32::from_rgb(rgb.r, rgb.g, rgb.b))
                .unwrap_or_else(|| self.indexed(i)),
        }
    }
}
