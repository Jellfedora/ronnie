//! The permissions dialog: read / write / execute for the owner, the group and the others, the special
//! bits, and a chmod field taking octal ("644") or textual changes ("u+x,go-w").
//! With several items, a box that differs between them stays mixed and each item keeps its own bit.

use super::*;

/// One permission bit across the selected items.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Tri {
    On,
    Off,
    Mixed,
}

/// Bits in display order: owner rwx, group rwx, others rwx, then setuid, setgid, sticky.
pub(super) const BITS: [u32; 12] = [0o400, 0o200, 0o100, 0o40, 0o20, 0o10, 0o4, 0o2, 0o1, 0o4000, 0o2000, 0o1000];

pub use crate::sftp::{ModeChange, Scope};

pub(super) struct PermEdit {
    pub names: Vec<String>,
    pub bits: [Tri; 12],
    pub text: String,
    /// The text couldn't be read.
    pub invalid: bool,
    pub recursive: bool,
    pub scope: Scope,
    pub any_dir: bool,
    /// The owner shown (servers tell it), when all the items have the same.
    pub owner: Option<String>,
}

impl PermEdit {
    /// From the items' current modes (None: unknown, taken as 644).
    pub fn new(names: Vec<String>, modes: &[Option<u32>], any_dir: bool, owner: Option<String>) -> Self {
        let modes: Vec<u32> = modes.iter().map(|m| m.unwrap_or(0o644)).collect();
        let bits = BITS.map(|b| {
            let on = modes.iter().filter(|m| **m & b != 0).count();
            if on == modes.len() { Tri::On } else if on == 0 { Tri::Off } else { Tri::Mixed }
        });
        let mut edit = Self { names, bits, text: String::new(), invalid: false, recursive: false, scope: Scope::All, any_dir, owner };
        edit.text = edit.octal();
        edit
    }

    /// "644", "4755", or "6x4" when a digit differs between the items.
    pub fn octal(&self) -> String {
        let digit = |bits: &[Tri]| -> char {
            if bits.contains(&Tri::Mixed) {
                return 'x';
            }
            let v = bits.iter().zip([4, 2, 1]).filter(|(b, _)| **b == Tri::On).map(|(_, v)| v).sum::<u32>();
            char::from_digit(v, 8).unwrap()
        };
        let special = digit(&self.bits[9..12]);
        let rest: String = [digit(&self.bits[0..3]), digit(&self.bits[3..6]), digit(&self.bits[6..9])].iter().collect();
        if special == '0' { rest } else { format!("{special}{rest}") }
    }

    pub fn change(&self) -> ModeChange {
        let mut c = ModeChange::default();
        for (tri, bit) in self.bits.iter().zip(BITS) {
            match tri {
                Tri::On => c.set |= bit,
                Tri::Off => c.clear |= bit,
                Tri::Mixed => {}
            }
        }
        c
    }

    /// The chmod field was edited: octal, or textual changes applied to the boxes.
    pub fn text_changed(&mut self) {
        let text = self.text.trim();
        if text.is_empty() {
            self.invalid = false;
            return;
        }
        if text.chars().all(|c| c.is_digit(8)) {
            match u32::from_str_radix(text, 8).ok().filter(|v| *v <= 0o7777 && text.len() <= 5) {
                Some(v) => {
                    self.bits = BITS.map(|b| if v & b != 0 { Tri::On } else { Tri::Off });
                    self.invalid = false;
                }
                None => self.invalid = true,
            }
            return;
        }
        match parse_symbolic(text, self.bits) {
            Some(bits) => {
                self.bits = bits;
                self.invalid = false;
            }
            None => self.invalid = true,
        }
    }
}

/// "u+x,go-w", "a=rx", "+x", "o=", or a full "rwxr-xr-x" (9 letters), applied to `bits`.
pub(super) fn parse_symbolic(text: &str, mut bits: [Tri; 12]) -> Option<[Tri; 12]> {
    // ls style.
    if text.len() == 9 && text.chars().all(|c| "rwxstST-".contains(c)) {
        let chars: Vec<char> = text.chars().collect();
        let mut out = [Tri::Off; 12];
        for (i, c) in chars.iter().enumerate() {
            let expected = ['r', 'w', 'x'][i % 3];
            match *c {
                '-' => {}
                c if c == expected => out[i] = Tri::On,
                's' | 't' if i % 3 == 2 => {
                    out[i] = Tri::On;
                    out[9 + i / 3] = Tri::On;
                }
                'S' | 'T' if i % 3 == 2 => out[9 + i / 3] = Tri::On,
                _ => return None,
            }
        }
        // s only for owner / group, t only for others.
        if (chars[2] == 't' || chars[2] == 'T') || (chars[5] == 't' || chars[5] == 'T') || matches!(chars[8], 's' | 'S') {
            return None;
        }
        return Some(out);
    }
    for clause in text.split(',') {
        let op_at = clause.find(['+', '-', '='])?;
        let (who, rest) = clause.split_at(op_at);
        if !who.chars().all(|c| "ugoa".contains(c)) {
            return None;
        }
        let who: Vec<usize> = if who.is_empty() || who.contains('a') { vec![0, 1, 2] } else { who.chars().map(|c| "ugo".find(c).unwrap()).collect() };
        let op = rest.chars().next()?;
        let perms = &rest[1..];
        if !perms.chars().all(|c| "rwxst".contains(c)) {
            return None;
        }
        for &w in &who {
            for (k, p) in ['r', 'w', 'x'].into_iter().enumerate() {
                let i = w * 3 + k;
                match op {
                    '+' if perms.contains(p) => bits[i] = Tri::On,
                    '-' if perms.contains(p) => bits[i] = Tri::Off,
                    '=' => bits[i] = if perms.contains(p) { Tri::On } else { Tri::Off },
                    _ => {}
                }
            }
            // Special bits: s for the owner / group, t for "others" (or no one named: sticky).
            let special = match w {
                0 | 1 if perms.contains('s') => Some(9 + w),
                2 if perms.contains('t') => Some(11),
                _ => None,
            };
            if let Some(i) = special {
                bits[i] = if op == '-' { Tri::Off } else { Tri::On };
            } else if op == '=' && w < 2 {
                bits[9 + w] = Tri::Off;
            }
        }
        if perms.contains('t') && !who.contains(&2) {
            bits[11] = if op == '-' { Tri::Off } else { Tri::On };
        }
    }
    Some(bits)
}

/// The dialog's content (the buttons are the file manager's): the rwx grid, the special bits, and the
/// value field.
pub(super) fn perm_ui(ui: &mut Ui, edit: &mut PermEdit, theme: &Theme, t: &Strings) {
    let what = if edit.names.len() == 1 { edit.names[0].clone() } else { t.files_n_items.replace("{n}", &edit.names.len().to_string()) };
    ui.label(egui::RichText::new(what).monospace().size(12.5).color(theme.text_muted));
    if let Some(owner) = &edit.owner {
        ui.label(egui::RichText::new(t.perm_owned_by.replace("{owner}", owner)).size(12.0).color(theme.text_muted));
    }
    ui.add_space(8.0);
    let mut changed = false;
    let mut boxed = |ui: &mut Ui, tri: &mut Tri, label: &str| {
        let mut on = *tri == Tri::On;
        let resp = ui.add(egui::Checkbox::new(&mut on, egui::RichText::new(label).size(12.5)).indeterminate(*tri == Tri::Mixed));
        if resp.changed() {
            *tri = if on { Tri::On } else { Tri::Off };
            changed = true;
        }
        if *tri == Tri::Mixed {
            resp.on_hover_text(t.perm_mixed);
        }
    };
    egui::Grid::new("chmod-grid").num_columns(4).spacing([18.0, 8.0]).show(ui, |ui| {
        ui.label("");
        for h in [t.files_read, t.files_write, t.files_exec] {
            ui.label(egui::RichText::new(h).size(12.5).color(theme.text_muted));
        }
        ui.end_row();
        for (row, who) in [t.files_owner, t.files_group, t.files_others].into_iter().enumerate() {
            ui.label(egui::RichText::new(who).size(13.0));
            for k in 0..3 {
                boxed(ui, &mut edit.bits[row * 3 + k], "");
            }
            ui.end_row();
        }
        ui.label(egui::RichText::new(t.perm_special).size(13.0));
        for (k, label) in ["setuid", "setgid", "sticky"].into_iter().enumerate() {
            boxed(ui, &mut edit.bits[9 + k], label);
        }
        ui.end_row();
    });
    if changed {
        edit.text = edit.octal();
        edit.invalid = false;
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(t.files_octal).size(13.0));
        let field = egui::TextEdit::singleline(&mut edit.text).desired_width(110.0).font(FontId::monospace(13.0));
        let field = if edit.invalid { field.text_color(theme.ansi[1]) } else { field };
        if ui.add(field).changed() {
            edit.text_changed();
        }
        if !edit.bits.contains(&Tri::Mixed) {
            let mode = edit.change().set;
            ui.label(egui::RichText::new(super::files::mode_string(mode, false)).monospace().size(12.5).color(theme.text_muted));
        }
    });
    let (hint, color) = if edit.invalid { (t.perm_invalid, theme.ansi[1]) } else { (t.perm_hint, theme.text_muted) };
    ui.label(egui::RichText::new(hint).size(11.5).color(color));
    if edit.any_dir {
        ui.add_space(6.0);
        ui.checkbox(&mut edit.recursive, egui::RichText::new(t.files_recursive).size(13.0));
        if edit.recursive {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.radio_value(&mut edit.scope, Scope::All, egui::RichText::new(t.perm_scope_all).size(12.5));
                ui.radio_value(&mut edit.scope, Scope::Files, egui::RichText::new(t.perm_scope_files).size(12.5));
                ui.radio_value(&mut edit.scope, Scope::Dirs, egui::RichText::new(t.perm_scope_dirs).size(12.5));
            });
        }
    }
}

/// Local items: each gets its own new mode; recursively, links are skipped (they may point anywhere).
#[cfg(unix)]
pub(super) fn chmod_local(paths: &[std::path::PathBuf], change: ModeChange, recursive: bool, scope: Scope) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut stack: Vec<(std::path::PathBuf, bool)> = paths.iter().map(|p| (p.clone(), true)).collect();
    while let Some((path, root)) = stack.pop() {
        let meta = if root { std::fs::metadata(&path) } else { std::fs::symlink_metadata(&path) }.map_err(|e| format!("{} : {e}", path.display()))?;
        if meta.file_type().is_symlink() {
            continue;
        }
        let wanted = match scope {
            Scope::All => true,
            Scope::Files => !meta.is_dir(),
            Scope::Dirs => meta.is_dir(),
        };
        // Outside a recursive change, what was selected is changed whatever the scope.
        if wanted || !recursive {
            let mode = change.apply(meta.permissions().mode());
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).map_err(|e| format!("{} : {e}", path.display()))?;
        }
        if recursive && meta.is_dir() {
            for entry in std::fs::read_dir(&path).map_err(|e| format!("{} : {e}", path.display()))? {
                stack.push((entry.map_err(|e| e.to_string())?.path(), false));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_boxes_keep_each_items_bits() {
        let edit = PermEdit::new(vec!["a".into(), "b".into()], &[Some(0o644), Some(0o755)], false, None);
        assert_eq!(edit.octal(), "xxx");
        let mut edit = edit;
        edit.bits[2] = Tri::On; // owner x
        let c = edit.change();
        assert_eq!(c.apply(0o644), 0o744);
        assert_eq!(c.apply(0o755), 0o755);
    }

    #[test]
    fn reads_octal_and_textual_changes() {
        let mut edit = PermEdit::new(vec!["a".into()], &[Some(0o644)], false, None);
        edit.text = "u+x,go-r".into();
        edit.text_changed();
        assert_eq!(edit.octal(), "700");
        edit.text = "a=rx".into();
        edit.text_changed();
        assert_eq!(edit.octal(), "555");
        edit.text = "+t".into();
        edit.text_changed();
        assert_eq!(edit.octal(), "1555");
        edit.text = "rwsr-x---".into();
        edit.text_changed();
        assert_eq!(edit.octal(), "4750");
        edit.text = "00644".into();
        edit.text_changed();
        assert_eq!(edit.octal(), "644");
        edit.text = "u+q".into();
        edit.text_changed();
        assert!(edit.invalid);
        edit.text = "8".into();
        edit.text_changed();
        assert!(edit.invalid);
    }

    #[cfg(unix)]
    #[test]
    fn changes_local_items() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("ronnie-perms-{}", std::process::id()));
        std::fs::create_dir_all(base.join("d/sub")).unwrap();
        std::fs::write(base.join("d/a.sh"), b"").unwrap();
        std::fs::set_permissions(base.join("d/a.sh"), std::fs::Permissions::from_mode(0o644)).unwrap();
        let mode = |p: &str| std::fs::metadata(base.join(p)).unwrap().permissions().mode() & 0o7777;
        // Files only, recursively: u+x on a.sh, folders untouched.
        let before = mode("d/sub");
        chmod_local(&[base.join("d")], ModeChange { set: 0o100, clear: 0 }, true, Scope::Files).unwrap();
        assert_eq!(mode("d/a.sh"), 0o744);
        assert_eq!(mode("d/sub"), before);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
