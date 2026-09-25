use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor};
use egui::Color32;

/// Palette used both for terminal colors and for the surrounding UI.
pub struct Theme {
    pub fg: Color32,
    pub bg: Color32,
    pub cursor: Color32,
    pub selection: Color32,
    pub ansi: [Color32; 16],

    pub chrome_bg: Color32,
    pub tab_bg: Color32,
    pub tab_hover: Color32,
    pub tab_active: Color32,
    pub text_muted: Color32,
    pub text: Color32,
    pub accent: Color32,
    pub dark: bool,
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

pub const PRESETS: &[Preset] = &[
    // The house theme: deep black, blood red, old gold and bone-colored text.
    Preset {
        id: "ronnie",
        name: "Ronnie",
        bg: 0x110e0f,
        fg: 0xe6ddd0,
        cursor: 0xd9a93f,
        chrome: 0x0a0809,
        surface: 0x2b1e20,
        accent: 0xd8323c,
        ansi: [
            0x1c1617, 0xd8323c, 0x8fb35a, 0xd9a93f, 0x6f8fc4, 0xb8528c, 0x5fa9a0, 0xd4cabd, //
            0x5c4b4d, 0xff5058, 0xa9d06c, 0xf2c55c, 0x8eaae2, 0xde72ae, 0x80cfc5, 0xfff7ec,
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
            tab_bg: chrome,
            tab_hover: mix(chrome, surface, 0.6),
            tab_active: surface,
            text_muted: mix(fg, chrome, 0.42),
            text: fg,
            accent,
            dark: self.dark,
        }
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
