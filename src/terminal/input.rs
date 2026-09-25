use alacritty_terminal::term::TermMode;
use egui::{Key, Modifiers};

/// xterm modifier parameter (1 + shift + 2*alt + 4*ctrl), or `None` when no modifier is held.
fn modifier_param(m: Modifiers) -> Option<u8> {
    let v = 1 + m.shift as u8 + 2 * m.alt as u8 + 4 * m.ctrl as u8;
    (v > 1).then_some(v)
}

/// `CSI 1 ; mod X` when modified, otherwise `CSI X` (or `SS3 X` in application cursor mode).
fn cursor_key(letter: char, m: Modifiers, mode: TermMode) -> Vec<u8> {
    match modifier_param(m) {
        Some(p) => format!("\x1b[1;{p}{letter}").into_bytes(),
        None if mode.contains(TermMode::APP_CURSOR) => format!("\x1bO{letter}").into_bytes(),
        None => format!("\x1b[{letter}").into_bytes(),
    }
}

/// `CSI n ~` style keys (PageUp, Delete, F5...).
fn tilde_key(n: u8, m: Modifiers) -> Vec<u8> {
    match modifier_param(m) {
        Some(p) => format!("\x1b[{n};{p}~").into_bytes(),
        None => format!("\x1b[{n}~").into_bytes(),
    }
}

/// Translates a key press into the bytes a terminal program expects.
/// Printable text is not handled here: it arrives through `Event::Text`.
pub fn key_to_bytes(key: Key, m: Modifiers, mode: TermMode) -> Option<Vec<u8>> {
    // On macOS, Cmd is reserved for application shortcuts.
    if m.mac_cmd {
        return None;
    }

    let bytes = match key {
        Key::Enter => {
            if m.alt { b"\x1b\r".to_vec() } else { b"\r".to_vec() }
        }
        Key::Backspace => {
            if m.ctrl {
                b"\x08".to_vec()
            } else if m.alt {
                b"\x1b\x7f".to_vec()
            } else {
                b"\x7f".to_vec()
            }
        }
        Key::Tab => {
            if m.shift { b"\x1b[Z".to_vec() } else { b"\t".to_vec() }
        }
        Key::Escape => b"\x1b".to_vec(),
        Key::ArrowUp => cursor_key('A', m, mode),
        Key::ArrowDown => cursor_key('B', m, mode),
        Key::ArrowRight => cursor_key('C', m, mode),
        Key::ArrowLeft => cursor_key('D', m, mode),
        Key::Home => cursor_key('H', m, mode),
        Key::End => cursor_key('F', m, mode),
        Key::Insert => tilde_key(2, m),
        Key::Delete => tilde_key(3, m),
        Key::PageUp => tilde_key(5, m),
        Key::PageDown => tilde_key(6, m),
        Key::F1 => cursor_key_ss3('P', m),
        Key::F2 => cursor_key_ss3('Q', m),
        Key::F3 => cursor_key_ss3('R', m),
        Key::F4 => cursor_key_ss3('S', m),
        Key::F5 => tilde_key(15, m),
        Key::F6 => tilde_key(17, m),
        Key::F7 => tilde_key(18, m),
        Key::F8 => tilde_key(19, m),
        Key::F9 => tilde_key(20, m),
        Key::F10 => tilde_key(21, m),
        Key::F11 => tilde_key(23, m),
        Key::F12 => tilde_key(24, m),
        _ if m.ctrl => return ctrl_key(key, m),
        _ if m.alt && !cfg!(target_os = "macos") => {
            // Alt as Meta (on macOS, Option composes characters instead: needed for | { } [ ] on AZERTY).
            let c = key_char(key)?;
            let c = if m.shift { c.to_ascii_uppercase() } else { c };
            vec![0x1b, c as u8]
        }
        _ => return None,
    };
    Some(bytes)
}

fn cursor_key_ss3(letter: char, m: Modifiers) -> Vec<u8> {
    match modifier_param(m) {
        Some(p) => format!("\x1b[1;{p}{letter}").into_bytes(),
        None => format!("\x1bO{letter}").into_bytes(),
    }
}

fn key_char(key: Key) -> Option<char> {
    let name = key.name();
    let mut chars = name.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(c.to_ascii_lowercase())
}

/// Ctrl+letter and the few punctuation control codes.
fn ctrl_key(key: Key, m: Modifiers) -> Option<Vec<u8>> {
    let code = match key {
        Key::Space | Key::Num2 => 0x00,
        Key::OpenBracket | Key::Num3 => 0x1b,
        Key::Backslash | Key::Num4 => 0x1c,
        Key::CloseBracket | Key::Num5 => 0x1d,
        Key::Num6 => 0x1e,
        Key::Minus | Key::Slash | Key::Num7 => 0x1f,
        _ => {
            let c = key_char(key)?;
            if !c.is_ascii_lowercase() {
                return None;
            }
            c as u8 - b'a' + 1
        }
    };
    let mut out = Vec::with_capacity(2);
    if m.alt {
        out.push(0x1b);
    }
    out.push(code);
    Some(out)
}
