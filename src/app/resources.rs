//! The machine's resources, small at the bottom of the strip of categories (or of the folded rail):
//! CPU and GPU with their last minute, the memory, the fullest disk. Under the pointer, the details
//! beside them: each disk, what is read and written. Read by `crate::sysmon`; hidden from the settings.

use super::*;
use super::files::format_size;
use crate::sysmon::{Snapshot, HISTORY};

/// The width of the details.
const DETAILS_W: f32 = 200.0;
const PAD: f32 = 12.0;
const CURVE_H: f32 = 34.0;
const BAR_H: f32 = 6.0;
/// The gauges: a name and a value, a curve or a bar under it.
const GAUGE_TEXT_H: f32 = 11.0;
const GAUGE_CURVE_H: f32 = 12.0;
const GAUGE_BAR_H: f32 = 4.0;
const GAUGE_GAP: f32 = 7.0;

impl App {
    /// The gauges between `x.min` and `x.max`, ending at `bottom`; none if they don't reach down
    /// from `above`. Their thread starts when shown and stops when turned off in the settings.
    pub(super) fn resources_gauges(&mut self, ui: &Ui, x: egui::Rangef, above: f32, bottom: f32) {
        if !self.config.settings.resources_panel {
            self.sysmon = None;
            return;
        }
        let s = self.sysmon.get_or_insert_with(|| crate::sysmon::Monitor::start(ui.ctx())).snapshot();
        let (t, theme) = (self.t(), self.theme.clone());
        let fullest = s.disks.iter().map(|d| ratio(d.used, d.total)).fold(None, |a: Option<f32>, r| Some(a.map_or(r, |a| a.max(r))));
        // (name, value in percent, the curve if any, its color)
        let mut gauges: Vec<(&str, f32, Option<&std::collections::VecDeque<f32>>, Color32)> = vec![("CPU", s.cpu.back().copied().unwrap_or(0.0), Some(&s.cpu), theme.accent)];
        if s.gpu_known {
            gauges.push(("GPU", s.gpu.back().copied().unwrap_or(0.0), Some(&s.gpu), theme.ansi[5]));
        }
        gauges.push(("RAM", ratio(s.ram_used, s.ram_total) * 100.0, None, theme.ansi[2]));
        if let Some(disk) = fullest {
            gauges.push((t.resources_disk_short, disk * 100.0, None, theme.ansi[3]));
        }
        let height: f32 = gauges.iter().map(|g| GAUGE_TEXT_H + 2.0 + if g.2.is_some() { GAUGE_CURVE_H } else { GAUGE_BAR_H }).sum::<f32>() + GAUGE_GAP * (gauges.len() - 1) as f32;
        let top = bottom - height;
        if top - 10.0 < above {
            return;
        }
        let painter = ui.painter();
        painter.hline(x.min + 4.0..=x.max - 4.0, top - 9.0, Stroke::new(1.0, theme.tab_hover.gamma_multiply(0.7)));
        let mut y = top;
        for (name, value, curve, color) in gauges {
            let value_color = if value > 90.0 { theme.ansi[1] } else { theme.text };
            let number = painter.layout_no_wrap(format!("{value:.0}"), FontId::monospace(8.5), value_color);
            let label = painter.layout_no_wrap(name.to_owned(), FontId::proportional(7.5), theme.text_muted);
            let mid = y + GAUGE_TEXT_H / 2.0;
            // Too narrow for both: the value alone.
            if label.size().x + number.size().x + 3.0 <= x.span() {
                painter.galley(Pos2::new(x.min, mid - label.size().y / 2.0), label, theme.text_muted);
                painter.galley(Pos2::new(x.max - number.size().x, mid - number.size().y / 2.0), number, value_color);
            } else {
                painter.galley(Pos2::new(x.center() - number.size().x / 2.0, mid - number.size().y / 2.0), number, value_color);
            }
            y += GAUGE_TEXT_H + 2.0;
            let color = if value > 90.0 && curve.is_none() { theme.ansi[1] } else { color };
            match curve {
                Some(values) => {
                    paint_curve(painter, Rect::from_x_y_ranges(x, y..=y + GAUGE_CURVE_H), &theme, values, color, 2.0);
                    y += GAUGE_CURVE_H;
                }
                None => {
                    paint_bar(painter, Rect::from_x_y_ranges(x, y..=y + GAUGE_BAR_H), &theme, value / 100.0, color);
                    y += GAUGE_BAR_H;
                }
            }
            y += GAUGE_GAP;
        }

        // Under the pointer: the details, beside, and the way to the processes (a click on the gauges
        // goes there too). The details stay while the pointer goes to them.
        let area = Rect::from_x_y_ranges(x.min - 4.0..=x.max + 4.0, top - 6.0..=bottom + 4.0);
        let id = egui::Id::new("resources-details");
        let shown: Option<Rect> = ui.ctx().data(|d| d.get_temp(id));
        let over_details = shown.zip(ui.ctx().pointer_hover_pos()).is_some_and(|(r, p)| r.union(area).contains(p));
        if !ui.rect_contains_pointer(area) && !over_details {
            ui.ctx().data_mut(|d| d.remove::<Rect>(id));
            return;
        }
        let mut open = ui.interact(area, id.with("gauges"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
        let details = egui::Area::new(id).order(egui::Order::Foreground).pivot(Align2::LEFT_BOTTOM).fixed_pos(Pos2::new(area.max.x + 10.0, bottom + 6.0)).show(ui.ctx(), |ui| {
            Frame::NONE
                .fill(theme.chrome_bg)
                .stroke(Stroke::new(1.0, theme.tab_hover))
                .corner_radius(10.0)
                .inner_margin(PAD)
                .shadow(egui::Shadow { offset: [4, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(110) })
                .show(ui, |ui| {
                    ui.set_width(DETAILS_W - 2.0 * PAD);
                    sections(ui, t, &theme, &s);
                    ui.add_space(4.0);
                    let label = egui::RichText::new(format!("{}  ›", t.resources_procs)).size(12.5).color(theme.text);
                    if ui.add(egui::Button::new(label).corner_radius(6.0).min_size(Vec2::new(ui.available_width(), 28.0))).clicked() {
                        open = true;
                    }
                });
        });
        ui.ctx().data_mut(|d| d.insert_temp(id, details.response.rect));
        if open {
            ui.ctx().data_mut(|d| d.remove::<Rect>(id));
            self.open_procs_page();
        }
    }
}

fn sections(ui: &mut Ui, t: &Strings, theme: &Theme, s: &Snapshot) {
    let (cpu_color, gpu_color, ram_color, disk_color) = (theme.accent, theme.ansi[5], theme.ansi[2], theme.ansi[3]);
    let cpu = s.cpu.back().copied().unwrap_or(0.0);
    heading(ui, theme, t.resources_cpu, &format!("{cpu:.0} %"));
    curve(ui, theme, &s.cpu, cpu_color);
    ui.add_space(14.0);
    if s.gpu_known {
        let gpu = s.gpu.back().copied().unwrap_or(0.0);
        heading(ui, theme, t.resources_gpu, &format!("{gpu:.0} %"));
        curve(ui, theme, &s.gpu, gpu_color);
    } else {
        heading(ui, theme, t.resources_gpu, "—");
    }
    ui.add_space(14.0);
    let ram = ratio(s.ram_used, s.ram_total);
    heading(ui, theme, t.resources_ram, &format!("{:.0} %", ram * 100.0));
    bar(ui, theme, ram, ram_color);
    detail(ui, theme, &format!("{} / {}", format_size(s.ram_used, t), format_size(s.ram_total, t)));
    ui.add_space(14.0);
    heading(ui, theme, t.resources_disks, "");
    for disk in &s.disks {
        ui.add_space(4.0);
        let used = ratio(disk.used, disk.total);
        let label = egui::RichText::new(&disk.mount).size(12.0).color(theme.text);
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(label).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(format!("{:.0} %", used * 100.0)).size(12.0).color(theme.text_muted));
            });
        });
        bar(ui, theme, used, if used > 0.9 { theme.ansi[1] } else { disk_color });
        detail(ui, theme, &format!("{} / {}", format_size(disk.used, t), format_size(disk.total, t)));
        detail(ui, theme, &format!("↓ {}/s   ↑ {}/s", format_size(disk.read, t), format_size(disk.written, t)));
        ui.add_space(6.0);
    }
}

fn ratio(used: u64, total: u64) -> f32 {
    if total == 0 { 0.0 } else { (used as f64 / total as f64) as f32 }
}

/// The name of a resource, its value at the right.
fn heading(ui: &mut Ui, theme: &Theme, name: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(name).size(11.5).strong().color(theme.text_muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).size(13.0).strong().color(theme.text).monospace());
        });
    });
}

fn detail(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).size(11.0).color(theme.text_muted)).truncate());
}

/// How full, as a thin rounded bar.
fn bar(ui: &mut Ui, theme: &Theme, value: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), BAR_H), Sense::hover());
    paint_bar(ui.painter(), rect, theme, value, color);
}

fn paint_bar(painter: &egui::Painter, rect: Rect, theme: &Theme, value: f32, color: Color32) {
    let radius = rect.height() / 2.0;
    painter.rect_filled(rect, radius, theme.tab_hover);
    let filled = Rect::from_min_size(rect.min, Vec2::new(rect.width() * value.clamp(0.0, 1.0), rect.height()));
    painter.rect_filled(filled, radius, color);
}

/// The last minute of a percent, the latest at the right, the area under it lightly filled.
fn curve(ui: &mut Ui, theme: &Theme, values: &std::collections::VecDeque<f32>, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), CURVE_H), Sense::hover());
    paint_curve(ui.painter(), rect, theme, values, color, 4.0);
}

fn paint_curve(painter: &egui::Painter, rect: Rect, theme: &Theme, values: &std::collections::VecDeque<f32>, color: Color32, radius: f32) {
    painter.rect_filled(rect, radius, theme.bg.gamma_multiply(0.6));
    if values.len() < 2 {
        return;
    }
    let step = rect.width() / (HISTORY - 1) as f32;
    let start = rect.max.x - step * (values.len() - 1) as f32;
    let points: Vec<Pos2> = values.iter().enumerate().map(|(k, v)| Pos2::new(start + step * k as f32, rect.max.y - 1.0 - (rect.height() - 2.0) * v / 100.0)).collect();
    let fill = color.gamma_multiply(0.18);
    let mut mesh = egui::Mesh::default();
    for (k, p) in points.iter().enumerate() {
        mesh.colored_vertex(*p, fill);
        mesh.colored_vertex(Pos2::new(p.x, rect.max.y), fill);
        if k > 0 {
            let i = (k as u32) * 2;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i + 1, i);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.add(egui::Shape::line(points, Stroke::new(if rect.height() < 20.0 { 1.0 } else { 1.5 }, color)));
}
