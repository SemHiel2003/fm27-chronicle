//! Getekende bouwstenen van de kroniek: verloopvlakken, grafieken, vormstroken en balken.

use crate::data::{Match, Record, Res};
use crate::theme::{self, BORDER, DANGER, SUCCESS, SURFACE, SURFACE2, TEXT, TEXT_DIM, TEXT_FAINT, TRACK, WARNING};
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2};

pub const GOLD: Color32 = Color32::from_rgb(255, 200, 87);

pub fn res_color(r: Res) -> Color32 {
    match r {
        Res::W => SUCCESS,
        Res::D => WARNING,
        Res::L => DANGER,
    }
}

pub fn res_letter(r: Res) -> &'static str {
    match r {
        Res::W => crate::t!("W", "W"),
        Res::D => crate::t!("G", "D"),
        Res::L => crate::t!("V", "L"),
    }
}

/// Afgeronde rechthoek met een horizontaal kleurverloop (in dunne stroken, zodat de hoeken rond blijven).
pub fn gradient(p: &egui::Painter, rect: Rect, from: Color32, to: Color32, radius: u8) {
    let n = 64;
    let w = rect.width() / n as f32;
    for i in 0..n {
        let c = theme::mix(from, to, i as f32 / (n - 1) as f32);
        let r = Rect::from_min_max(pos2(rect.left() + w * i as f32 - 0.5, rect.top()), pos2(rect.left() + w * (i + 1) as f32 + 0.5, rect.bottom())).intersect(rect);
        let cr = if i == 0 {
            CornerRadius { nw: radius, sw: radius, ne: 0, se: 0 }
        } else if i == n - 1 {
            CornerRadius { nw: 0, sw: 0, ne: radius, se: radius }
        } else {
            CornerRadius::ZERO
        };
        p.rect_filled(r, cr, c);
    }
}

/// Zachte lichtvlek (voor de kop en de deelkaart).
pub fn glow(p: &egui::Painter, center: Pos2, radius: f32, color: Color32, strength: f32) {
    // waaier van driehoeken: kleur in het midden, doorzichtig aan de rand (vloeiend, zonder banden)
    let mut mesh = egui::Mesh::default();
    let mid = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (strength * 110.0).clamp(0.0, 255.0) as u8);
    let n = 72u32;
    mesh.colored_vertex(center, mid);
    for ring in [0.45_f32, 1.0] {
        let c = if ring < 1.0 { Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (strength * 38.0).clamp(0.0, 255.0) as u8) } else { Color32::TRANSPARENT };
        for i in 0..n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            mesh.colored_vertex(center + vec2(a.cos(), a.sin()) * radius * ring, c);
        }
    }
    for i in 0..n {
        let j = (i + 1) % n;
        mesh.add_triangle(0, 1 + i, 1 + j);
        mesh.add_triangle(1 + i, 1 + n + i, 1 + n + j);
        mesh.add_triangle(1 + i, 1 + n + j, 1 + j);
    }
    p.add(Shape::mesh(mesh));
}

/// Vormstrook: een blokje per wedstrijd, nieuwste rechts.
pub fn form_strip(ui: &mut Ui, ms: &[&Match], size: f32, letters: bool) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = (size * 0.18).max(2.0);
        let e = theme::enter(ui.ctx(), 0.7);
        for (i, m) in ms.iter().enumerate() {
            let Some(r) = m.result() else { continue };
            let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
            let shown = ((e * ms.len() as f32) - i as f32).clamp(0.0, 1.0);
            let c = theme::mix(SURFACE2, res_color(r), shown);
            ui.painter().rect_filled(rect, CornerRadius::same((size * 0.28) as u8), c);
            if letters {
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, res_letter(r), FontId::new(size * 0.5, theme::bold()), Color32::from_rgb(8, 10, 14));
            }
            resp.on_hover_text(format!("{}  {} {} {}\n{}", crate::i18n::date(m.date), m.home, m.score(), m.away, m.comp));
        }
    });
}

/// Gestapelde balk winst / gelijk / verlies.
pub fn wdl_bar(ui: &mut Ui, r: &Record, height: f32) {
    let w = ui.available_width().max(40.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, height), Sense::hover());
    let p = ui.painter();
    let rad = CornerRadius::same((height / 2.0) as u8);
    p.rect_filled(rect, rad, TRACK);
    if r.p == 0 {
        return;
    }
    let e = theme::enter(ui.ctx(), 0.7);
    let mut x = rect.left();
    let parts = [(r.w, SUCCESS), (r.d, WARNING), (r.l, DANGER)];
    let last = parts.iter().rposition(|(n, _)| *n > 0).unwrap_or(0);
    let first = parts.iter().position(|(n, _)| *n > 0).unwrap_or(0);
    for (i, (n, c)) in parts.iter().enumerate() {
        if *n == 0 {
            continue;
        }
        let pw = rect.width() * (*n as f32 / r.p as f32) * e;
        let h = (height / 2.0) as u8;
        let cr = CornerRadius { nw: if i == first { h } else { 0 }, sw: if i == first { h } else { 0 }, ne: if i == last { h } else { 0 }, se: if i == last { h } else { 0 } };
        p.rect_filled(Rect::from_min_size(pos2(x, rect.top()), vec2(pw, height)), cr, *c);
        x += pw;
    }
}

pub struct Series<'a> {
    pub points: &'a [f32],
    pub color: Color32,
    pub fill: bool,
}

/// Lijngrafiek met een verlopend vlak eronder. `invert`: lage waarden boven (ranglijstpositie).
/// `labels`: tekst onder de x-as (index, tekst); `fmt`: waarde naar tekst voor de y-as en de aanwijzer.
pub fn line_chart(ui: &mut Ui, height: f32, series: &[Series], range: (f32, f32), invert: bool, labels: &[(usize, String)], fmt: &dyn Fn(f32) -> String, tips: &[String]) {
    let w = ui.available_width().max(120.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, height), Sense::hover());
    let p = ui.painter().with_clip_rect(rect.expand(2.0));
    let plot = Rect::from_min_max(pos2(rect.left() + 34.0, rect.top() + 8.0), pos2(rect.right() - 8.0, rect.bottom() - 22.0));
    let n = series.iter().map(|s| s.points.len()).max().unwrap_or(0);
    let (lo, hi) = if (range.1 - range.0).abs() < f32::EPSILON { (range.0 - 1.0, range.1 + 1.0) } else { range };
    let y_of = |v: f32| {
        let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
        if invert { plot.top() + t * plot.height() } else { plot.bottom() - t * plot.height() }
    };
    let x_of = |i: usize| if n <= 1 { plot.center().x } else { plot.left() + plot.width() * i as f32 / (n - 1) as f32 };
    // raster
    for k in 0..=3 {
        let v = lo + (hi - lo) * k as f32 / 3.0;
        let y = y_of(v);
        p.line_segment([pos2(plot.left(), y), pos2(plot.right(), y)], Stroke::new(1.0, theme::mix(SURFACE, BORDER, 0.8)));
        p.text(pos2(plot.left() - 8.0, y), Align2::RIGHT_CENTER, fmt(v), FontId::new(11.0, egui::FontFamily::Proportional), TEXT_FAINT);
    }
    for (i, l) in labels {
        p.text(pos2(x_of(*i), plot.bottom() + 6.0), Align2::CENTER_TOP, l, FontId::new(11.0, egui::FontFamily::Proportional), TEXT_FAINT);
    }
    let e = theme::enter(ui.ctx(), 0.9);
    let reveal = plot.left() + plot.width() * e;
    let pc = p.with_clip_rect(Rect::from_min_max(rect.min, pos2(reveal + 4.0, rect.bottom())));
    for s in series {
        if s.points.is_empty() {
            continue;
        }
        let pts: Vec<Pos2> = s.points.iter().enumerate().map(|(i, v)| pos2(x_of(i), y_of(*v))).collect();
        if s.fill && pts.len() > 1 {
            let mut mesh = egui::Mesh::default();
            let top = Color32::from_rgba_unmultiplied(s.color.r(), s.color.g(), s.color.b(), 70);
            let base = Color32::from_rgba_unmultiplied(s.color.r(), s.color.g(), s.color.b(), 0);
            for pt in &pts {
                mesh.colored_vertex(*pt, top);
                mesh.colored_vertex(pos2(pt.x, plot.bottom()), base);
            }
            for i in 0..pts.len() as u32 - 1 {
                mesh.add_triangle(2 * i, 2 * i + 1, 2 * i + 2);
                mesh.add_triangle(2 * i + 1, 2 * i + 3, 2 * i + 2);
            }
            pc.add(Shape::mesh(mesh));
        }
        if pts.len() > 1 {
            pc.add(Shape::line(pts.clone(), Stroke::new(2.4, s.color)));
        }
        if let Some(last) = pts.last() {
            if e >= 1.0 {
                p.circle_filled(*last, 5.0, SURFACE);
                p.circle_filled(*last, 3.5, s.color);
            }
        }
    }
    // aanwijzer
    if let (Some(pos), true) = (resp.hover_pos(), n > 0) {
        let i = if n <= 1 { 0 } else { (((pos.x - plot.left()) / plot.width()) * (n - 1) as f32).round().clamp(0.0, (n - 1) as f32) as usize };
        let x = x_of(i);
        p.line_segment([pos2(x, plot.top()), pos2(x, plot.bottom())], Stroke::new(1.0, theme::mix(SURFACE, TEXT_DIM, 0.5)));
        for s in series {
            if let Some(v) = s.points.get(i) {
                p.circle_filled(pos2(x, y_of(*v)), 4.5, s.color);
            }
        }
        let mut text = tips.get(i).cloned().unwrap_or_default();
        if text.is_empty() {
            text = series.iter().filter_map(|s| s.points.get(i)).map(|v| fmt(*v)).collect::<Vec<_>>().join(" · ");
        }
        resp.on_hover_text_at_pointer(text);
    }
}

/// Staafjes (bv. doelpunten per maand): twee waarden per groep, naast elkaar.
pub fn bars(ui: &mut Ui, height: f32, groups: &[(String, f32, f32)], colors: (Color32, Color32)) {
    let w = ui.available_width().max(120.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, height), Sense::hover());
    let p = ui.painter();
    let max = groups.iter().map(|g| g.1.max(g.2)).fold(1.0_f32, f32::max);
    let base = rect.bottom() - 20.0;
    let gw = rect.width() / groups.len().max(1) as f32;
    let bw = (gw * 0.28).min(16.0);
    let e = theme::enter(ui.ctx(), 0.8);
    for (i, (label, a, b)) in groups.iter().enumerate() {
        let cx = rect.left() + gw * (i as f32 + 0.5);
        for (k, (v, c)) in [(*a, colors.0), (*b, colors.1)].into_iter().enumerate() {
            let h = (base - rect.top() - 14.0) * (v / max) * e;
            let x = cx + if k == 0 { -bw - 1.5 } else { 1.5 };
            let r = Rect::from_min_max(pos2(x, base - h), pos2(x + bw, base));
            p.rect_filled(r, CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, c);
            if e >= 1.0 && v > 0.0 {
                p.text(pos2(x + bw / 2.0, base - h - 2.0), Align2::CENTER_BOTTOM, format!("{v:.0}"), FontId::new(10.5, theme::medium()), TEXT_DIM);
            }
        }
        p.text(pos2(cx, base + 5.0), Align2::CENTER_TOP, label, FontId::new(11.0, egui::FontFamily::Proportional), TEXT_FAINT);
    }
    p.line_segment([pos2(rect.left(), base), pos2(rect.right(), base)], Stroke::new(1.0, BORDER));
}

/// Kaartje met een record: icoon, titel, grote waarde, toelichting.
pub fn record_card(ui: &mut Ui, icon_s: &str, color: Color32, title: &str, value: &str, detail: &str) {
    egui::Frame::new()
        .fill(SURFACE2)
        .corner_radius(CornerRadius::same(14))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.set_min_height(74.0);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::same(11), theme::tint(color, 0.2));
                ui.painter().text(r.center(), Align2::CENTER_CENTER, icon_s, FontId::new(19.0, theme::icons()), color);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(theme::caps(title));
                    ui.label(RichText::new(value).font(FontId::new(18.0, theme::bold())).color(TEXT));
                    if !detail.is_empty() {
                        ui.add(egui::Label::new(RichText::new(detail).size(12.0).color(TEXT_DIM)).truncate());
                    }
                });
            });
        });
}

/// Rangtelwoord: 1e / 1st.
pub fn ordinal(n: usize) -> String {
    if crate::i18n::is_nl() {
        return format!("{n}e");
    }
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// Lege toestand met een icoon en uitleg.
pub fn empty(ui: &mut Ui, icon_s: &str, title: &str, text: &str) {
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        ui.label(theme::ic(icon_s).size(46.0).color(theme::ACCENT));
        ui.add_space(8.0);
        ui.label(theme::h2(title));
        ui.add_space(2.0);
        ui.add(egui::Label::new(theme::dim(text)).wrap());
    });
}
