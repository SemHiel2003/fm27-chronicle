//! De app: zijbalk, verversen op de achtergrond en alle pagina's.

use crate::data::{self, Dataset, Match, Record, Res, Season};
use crate::i18n::{self, Lang};
use crate::theme::{self, ACCENT, ACCENT_HI, BORDER, DANGER, INFO, SUCCESS, SURFACE, SURFACE2, TEXT, TEXT_DIM, TEXT_FAINT, WARNING, icon};
use crate::widgets::{self as w, GOLD, Series};
use crate::{fm, store, t};
use anyhow::{Result, anyhow};
use eframe::egui::{self, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

pub fn run() -> Result<()> {
    let size = std::env::var("FMC_WINDOW")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some([w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?])
        })
        .unwrap_or([1360.0, 900.0]);
    let options = eframe::NativeOptions {
        // een testopname (FMC_SNAPSHOT) pakt de focus niet af van wat er op dat moment gebeurt
        viewport: egui::ViewportBuilder::default().with_title("FM27 Chronicle").with_inner_size(size).with_min_inner_size([1080.0, 700.0]).with_active(std::env::var_os("FMC_SNAPSHOT").is_none())
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon/icon_256.png")).unwrap_or_default()),
        ..Default::default()
    };
    eframe::run_native(
        "FM27 Chronicle",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
    .map_err(|e| anyhow!("{e}"))
}

#[derive(Clone, Serialize, Deserialize)]
struct Settings {
    lang: Lang,
    /// Bedragen in euro's tonen (FM bewaart ponden).
    euro: bool,
    rate: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { lang: i18n::detect(), euro: true, rate: 1.17 }
    }
}

fn settings_file() -> std::path::PathBuf {
    crate::data_dir().join("settings.json")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Season,
    Matches,
    Table,
    Career,
    Squad,
    Money,
    Card,
    Settings,
}

enum Msg {
    Progress(String),
    Unchanged,
    Done(Result<Dataset, String>),
}

struct App {
    settings: Settings,
    page: Page,
    ds: Option<Arc<Dataset>>,
    season: Option<u16>,
    comp: Option<String>,
    status: String,
    error: bool,
    progress: Option<String>,
    rx: Option<Receiver<Msg>>,
    last_check: Instant,
    connected: bool,
    frames: u32,
    card_rect: Option<Rect>,
    want_card: bool,
    card_career: bool,
    shot_sent: bool,
    notice: Option<(String, Instant)>,
}

fn strip(comp: &str) -> String {
    comp.replace("VriendenLoterij ", "")
}

impl App {
    fn new() -> Self {
        let mut settings: Settings = std::fs::read(settings_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        if let Ok(l) = std::env::var("FMC_LANG") {
            settings.lang = if l == "nl" { Lang::Nl } else { Lang::En };
        }
        i18n::set(settings.lang);
        // testhulp: FMC_DATA=<dump.json> toont een opgeslagen uitlezing zonder FM
        let ds: Option<Dataset> = match std::env::var("FMC_DATA") {
            Ok(p) => std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok()),
            Err(_) => store::latest(),
        };
        let page = match std::env::var("FMC_PAGE").as_deref() {
            Ok("matches") => Page::Matches,
            Ok("table") => Page::Table,
            Ok("career") => Page::Career,
            Ok("squad") => Page::Squad,
            Ok("money") => Page::Money,
            Ok("card") => Page::Card,
            Ok("settings") => Page::Settings,
            _ => Page::Season,
        };
        App {
            settings,
            page,
            ds: ds.map(Arc::new),
            season: std::env::var("FMC_SEASON").ok().and_then(|s| s.parse().ok()),
            comp: None,
            status: String::new(),
            error: false,
            progress: None,
            rx: None,
            last_check: Instant::now() - Duration::from_secs(60),
            connected: false,
            frames: 0,
            card_rect: None,
            want_card: false,
            card_career: std::env::var("FMC_CARD").as_deref() == Ok("career"),
            shot_sent: false,
            notice: None,
        }
    }

    fn save_settings(&self) {
        let _ = std::fs::create_dir_all(crate::data_dir());
        if let Ok(b) = serde_json::to_vec_pretty(&self.settings) {
            let _ = std::fs::write(settings_file(), b);
        }
    }

    fn cash(&self, v: i64) -> String {
        if self.settings.euro { data::money((v as f64 * self.settings.rate as f64) as i64).replace('£', "€") } else { data::money(v) }
    }

    fn delta(&self, d: i64) -> String {
        if d == 0 { String::new() } else { format!("{}{}", if d > 0 { "+" } else { "" }, self.cash(d)) }
    }

    /// Start een uitlezing op de achtergrond. Zonder `force` alleen als de speldatum veranderd is.
    fn refresh(&mut self, ctx: &egui::Context, force: bool) {
        if self.rx.is_some() || std::env::var_os("FMC_DATA").is_some() {
            return;
        }
        let known = self.ds.as_ref().and_then(|d| d.today);
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let send = |m: Msg| {
                let _ = tx.send(m);
                ctx.request_repaint();
            };
            if !force {
                let now = fm::request(serde_json::json!({"cmd": "game_date"})).ok().and_then(|d| chrono::NaiveDate::from_yo_opt(d.get("year")?.as_i64()? as i32, d.get("doy")?.as_u64()? as u32));
                if now.is_some() && now == known {
                    return send(Msg::Unchanged);
                }
            }
            let r = data::load::dataset(&|s| send(Msg::Progress(s.to_string())));
            send(Msg::Done(r.map_err(|e| e.to_string())));
        });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let mut done = false;
        if let Some(rx) = &self.rx {
            while let Ok(m) = rx.try_recv() {
                match m {
                    Msg::Progress(s) => self.progress = Some(s),
                    Msg::Unchanged => {
                        self.connected = true;
                        done = true;
                    }
                    Msg::Done(Ok(new)) => {
                        let key = store::key(&new);
                        let merged = store::merge(store::find(&key, &new), new, 7);
                        self.status.clear();
                        if let Err(e) = store::save(&key, &merged) {
                            self.status = format!("{}: {e}", t!("Archief niet opgeslagen", "Archive not saved"));
                        }
                        let first = self.ds.is_none();
                        self.ds = Some(Arc::new(merged));
                        (self.connected, self.error, done) = (true, false, true);
                        if first {
                            theme::restart_enter(ctx);
                        }
                    }
                    Msg::Done(Err(e)) => {
                        (self.connected, self.error, done) = (false, true, true);
                        self.status = e;
                    }
                }
            }
        }
        if done {
            self.rx = None;
            self.progress = None;
        }
        // elke 20 s kijken of de speldatum veranderd is
        if self.rx.is_none() && self.last_check.elapsed() > Duration::from_secs(20) && std::env::var_os("FMC_DATA").is_none() {
            self.last_check = Instant::now();
            if fm::module_ready() {
                self.refresh(ctx, false);
            } else {
                self.connected = false;
            }
        }
        ctx.request_repaint_after(Duration::from_secs(2));
    }

    /// Testhulp: FMC_SNAPSHOT=pad.png legt het eigen venster vast en sluit af. Ook de export van de deelkaart.
    fn screenshots(&mut self, ctx: &egui::Context) {
        let snap = std::env::var("FMC_SNAPSHOT").ok();
        self.frames += 1;
        // met een echte verbinding eerst de uitlezing afwachten
        let ready = std::env::var_os("FMC_DATA").is_some() || (self.rx.is_none() && (self.ds.is_some() || self.error));
        if snap.is_some() && !self.shot_sent && self.frames >= 100 && ready {
            self.shot_sent = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        if snap.is_some() {
            ctx.request_repaint();
        }
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(img) = shot else { return };
        let save = |img: &egui::ColorImage, path: &std::path::Path| -> bool {
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, bytes).is_some_and(|b| b.save(path).is_ok())
        };
        if self.want_card {
            self.want_card = false;
            if let Some(rect) = self.card_rect {
                let part = img.region(&rect, Some(ctx.pixels_per_point()));
                let dir = crate::home().join("Pictures").join("FM27 Chronicle");
                let _ = std::fs::create_dir_all(&dir);
                let career = self.card_career;
                let name = self.ds.as_ref().map(|d| if career { format!("{} career.png", d.club_short) } else { format!("{} {}.png", d.season(self.season.unwrap_or(0)).team, d.season_label(self.season.unwrap_or(0)).replace('/', "-")) }).unwrap_or_else(|| "season.png".into());
                let path = dir.join(name);
                let ok = save(&part, &path);
                self.notice = Some((if ok { format!("{} {}", t!("Opgeslagen:", "Saved:"), path.display()) } else { t!("Opslaan mislukt.", "Saving failed.").to_string() }, Instant::now()));
                if ok {
                    open_path(&dir);
                }
            }
        } else if let Some(path) = snap {
            save(&img, std::path::Path::new(&path));
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn go(&mut self, ctx: &egui::Context, p: Page) {
        if self.page != p {
            self.page = p;
            theme::restart_enter(ctx);
        }
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("nav")
            .exact_width(236.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::SIDEBAR).inner_margin(egui::Margin::symmetric(14, 18)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
                    w::gradient(ui.painter(), r, ACCENT, Color32::from_rgb(255, 120, 190), 11);
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, icon::BOOK_OPEN_TEXT, FontId::new(20.0, theme::icons()), Color32::WHITE);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(RichText::new("FM27 Chronicle").font(FontId::new(16.0, theme::bold())).color(TEXT));
                        ui.label(theme::faint(t!("Jouw carrière, vastgelegd", "Your career, on record")));
                    });
                });
                ui.add_space(22.0);
                ui.label(theme::caps(t!("Kroniek", "Chronicle")));
                ui.add_space(4.0);
                let items = [
                    (Page::Season, icon::CALENDAR_STAR, t!("Seizoen", "Season")),
                    (Page::Matches, icon::SOCCER_BALL, t!("Wedstrijden", "Matches")),
                    (Page::Table, icon::RANKING, t!("Stand", "Table")),
                    (Page::Career, icon::TROPHY, t!("Carrière", "Career")),
                    (Page::Squad, icon::USERS_THREE, t!("Selectie", "Squad")),
                    (Page::Money, icon::CHART_LINE_UP, t!("Financiën", "Finances")),
                    (Page::Card, icon::SHARE_NETWORK, t!("Deelkaart", "Share card")),
                ];
                for (p, ic, label) in items {
                    if theme::nav_item(ui, ic, label, self.page == p).clicked() {
                        self.go(ctx, p);
                    }
                }
                ui.add_space(14.0);
                if theme::nav_item(ui, icon::GEAR_SIX, t!("Instellingen", "Settings"), self.page == Page::Settings).clicked() {
                    self.go(ctx, Page::Settings);
                }
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.label(theme::faint(format!("v{}", env!("CARGO_PKG_VERSION"))));
                    ui.add_space(4.0);
                    if let Some(d) = self.ds.as_ref().and_then(|d| d.today) {
                        ui.label(theme::faint(format!("{} {}", t!("Speldatum", "Game date"), i18n::date(d))));
                    }
                    ui.horizontal(|ui| {
                        let busy = self.rx.is_some() && self.progress.is_some();
                        theme::status_dot(ui, if busy { INFO } else if self.connected { SUCCESS } else { TEXT_FAINT });
                        let text = if busy {
                            self.progress.clone().unwrap_or_default()
                        } else if self.connected {
                            t!("Verbonden met FM27", "Connected to FM27").to_string()
                        } else if self.ds.is_some() {
                            t!("Archief (FM niet verbonden)", "Archive (FM not connected)").to_string()
                        } else {
                            t!("Niet verbonden", "Not connected").to_string()
                        };
                        ui.add(egui::Label::new(RichText::new(text).size(12.5).color(TEXT_DIM)).truncate());
                    });
                    ui.separator();
                });
            });
    }

    // ------------------------------------------------------------------ gedeelde stukken

    fn seasons(&mut self, ds: &Dataset) -> u16 {
        let years = ds.season_years();
        let cur = self.season.filter(|y| years.contains(y)).or(years.last().copied()).unwrap_or(0);
        self.season = Some(cur);
        cur
    }

    fn season_picker(&mut self, ui: &mut Ui, ds: &Dataset) {
        let labels: Vec<(u16, String)> = ds.season_years().into_iter().rev().take(8).rev().map(|y| (y, ds.season_label(y))).collect();
        let opts: Vec<(u16, &str)> = labels.iter().map(|(y, l)| (*y, l.as_str())).collect();
        let mut cur = self.season.unwrap_or(0);
        if theme::segmented(ui, &mut cur, &opts) {
            self.season = Some(cur);
            theme::restart_enter(ui.ctx());
        }
    }

    fn header(&mut self, ui: &mut Ui, ds: &Dataset, title: &str, sub: &str, seasons: bool) {
        theme::page_header(ui, title, sub, |ui| {
            let busy = self.rx.is_some();
            if theme::button(ui, icon::ARROWS_CLOCKWISE, t!("Ververs", "Refresh"), !busy && fm::module_ready()).clicked() {
                self.refresh(ui.ctx(), true);
            }
            if seasons {
                self.season_picker(ui, ds);
            }
        });
    }

    // ------------------------------------------------------------------ seizoen

    fn page_season(&mut self, ui: &mut Ui, ds: &Dataset) {
        let year = self.seasons(ds);
        let s = ds.season(year);
        self.header(ui, ds, t!("Seizoensoverzicht", "Season review"), &format!("{} · {}", s.club, ds.season_label(year)), true);
        let played = s.played();
        if played.is_empty() {
            return w::empty(ui, icon::CALENDAR_BLANK, t!("Nog geen wedstrijden", "No matches yet"), t!("Zodra je eerste officiële wedstrijd gespeeld is, verschijnt hier je seizoen.", "Your season shows up here once the first competitive match is played."));
        }
        let all = Record::of(played.iter().copied());
        let league_rec = Record::of(s.league_matches().iter().copied());
        let rows = s.league.map(|l| data::table(&l.games, None)).unwrap_or_default();
        let pos = data::position(&rows, &s.team);
        let champion = pos == Some(1) && s.league.is_some_and(|l| l.open == 0);
        let trophies: Vec<data::Trophy> = ds.trophies().into_iter().filter(|t| t.season == year).collect();

        // kop met verloop
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 150.0), Sense::hover());
        let p = ui.painter().with_clip_rect(rect);
        w::gradient(&p, rect, Color32::from_rgb(38, 30, 92), Color32::from_rgb(16, 18, 30), 20);
        w::glow(&p, rect.right_top() + vec2(-140.0, 10.0), 240.0, if champion { GOLD } else { ACCENT }, 0.5);
        p.rect_stroke(rect, CornerRadius::same(20), Stroke::new(1.0, theme::mix(BORDER, ACCENT, 0.25)), egui::StrokeKind::Inside);
        let x = rect.left() + 28.0;
        p.text(pos2(x, rect.top() + 24.0), Align2::LEFT_TOP, ds.season_label(year), FontId::new(13.0, theme::semibold()), ACCENT_HI);
        p.text(pos2(x, rect.top() + 44.0), Align2::LEFT_TOP, &s.club, FontId::new(34.0, theme::bold()), Color32::WHITE);
        let line = match (pos, s.league) {
            (Some(_), Some(l)) if champion => format!("{} · {}", t!("Kampioen", "Champions"), strip(&l.comp)),
            (Some(n), Some(l)) => format!("{} {} · {}{}", w::ordinal(n), t!("plaats", "place"), strip(&l.comp), if l.open > 0 { format!(" · {}", t!("seizoen loopt", "season in progress")) } else { String::new() }),
            _ => String::new(),
        };
        if champion {
            p.text(pos2(x, rect.top() + 101.0), Align2::LEFT_CENTER, icon::CROWN_SIMPLE, FontId::new(18.0, theme::icons()), GOLD);
        }
        p.text(pos2(x + if champion { 26.0 } else { 0.0 }, rect.top() + 101.0), Align2::LEFT_CENTER, line, FontId::new(15.0, theme::medium()), if champion { GOLD } else { TEXT_DIM });
        // prijzen rechts
        let mut tx = rect.right() - 36.0;
        for tr in trophies.iter().rev() {
            let c = pos2(tx - 30.0, rect.center().y - 10.0);
            p.circle_filled(c, 28.0, Color32::from_rgba_unmultiplied(255, 200, 87, 34));
            p.text(c, Align2::CENTER_CENTER, icon::TROPHY, FontId::new(28.0, theme::icons()), GOLD);
            p.text(pos2(c.x, c.y + 36.0), Align2::CENTER_TOP, shorten(&strip(&tr.comp), 18), FontId::new(11.0, theme::medium()), TEXT_DIM);
            tx -= 124.0;
        }
        ui.add_space(14.0);

        // tegels
        ui.columns(5, |c| {
            let teams = rows.len().max(1) as f32;
            match pos {
                Some(n) => theme::stat_tile(&mut c[0], icon::RANKING, t!("Positie", "Position"), &w::ordinal(n), &format!("{} {}", t!("van", "of"), rows.len()), if n == 1 { GOLD } else { ACCENT }, Some(100.0 * (teams - n as f32 + 1.0) / teams)),
                None => theme::stat_tile(&mut c[0], icon::RANKING, t!("Positie", "Position"), "–", "", ACCENT, None),
            }
            theme::stat_tile(&mut c[1], icon::STAR, t!("Punten", "Points"), &league_rec.points().to_string(), &format!("{:.2} {}", league_rec.ppg(), t!("per duel", "per game")), SUCCESS, Some(100.0 * league_rec.ppg() / 3.0));
            theme::stat_tile(&mut c[2], icon::HANDSHAKE, t!("W – G – V", "W – D – L"), &format!("{}–{}–{}", all.w, all.d, all.l), &format!("{} {}", all.p, t!("wedstrijden", "matches")), INFO, None);
            theme::stat_tile(&mut c[3], icon::SOCCER_BALL, t!("Doelpunten", "Goals"), &format!("{}:{}", all.gf, all.ga), &format!("{:+} · {:.1} {}", all.gd(), all.gf as f32 / all.p.max(1) as f32, t!("per duel", "per game")), WARNING, None);
            theme::stat_tile(&mut c[4], icon::PERCENT, t!("Gewonnen", "Win rate"), &format!("{:.0}%", all.win_pct()), t!("alle competities", "all competitions"), SUCCESS, Some(all.win_pct()));
        });
        ui.add_space(14.0);

        ui.columns(2, |c| {
            theme::card(&mut c[0], |ui| {
                ui.label(theme::h2(t!("Seizoensverloop", "Season progression")));
                ui.label(theme::faint(t!("Positie op de ranglijst na elke speelronde", "League position after each matchday")));
                ui.add_space(6.0);
                let ps = s.positions();
                if ps.len() < 2 {
                    ui.label(theme::dim(t!("Nog te weinig competitieduels.", "Not enough league matches yet.")));
                } else {
                    let pts: Vec<f32> = ps.iter().map(|x| x.1 as f32).collect();
                    let labels: Vec<(usize, String)> = (0..ps.len()).filter(|i| (i + 1) % 5 == 0 || *i == 0).map(|i| (i, (i + 1).to_string())).collect();
                    let tips: Vec<String> = ps.iter().enumerate().map(|(i, (d, pos, pt))| format!("{} {} · {}\n{} · {} {}", t!("Speelronde", "Matchday"), i + 1, i18n::date(*d), w::ordinal(*pos), pt, t!("punten", "points"))).collect();
                    w::line_chart(ui, 190.0, &[Series { points: &pts, color: ACCENT_HI, fill: true }], (1.0, rows.len().max(2) as f32), true, &labels, &|v| format!("{v:.0}"), &tips);
                }
                ui.add_space(8.0);
                ui.label(theme::caps(t!("Vorm (laatste 12)", "Form (last 12)")));
                ui.add_space(2.0);
                let recent: Vec<&Match> = played.iter().rev().take(12).rev().copied().collect();
                w::form_strip(ui, &recent, 26.0, true);
                ui.add_space(14.0);
                let (home, away): (Vec<&Match>, Vec<&Match>) = played.iter().copied().partition(|m| m.at_home);
                comp_row(ui, t!("Thuis", "Home"), &Record::of(home));
                comp_row(ui, t!("Uit", "Away"), &Record::of(away));
            });
            theme::card(&mut c[1], |ui| {
                ui.label(theme::h2(t!("Hoogtepunten", "Highlights")));
                ui.add_space(6.0);
                records(ui, &played);
            });
        });
        ui.add_space(14.0);

        ui.columns(2, |c| {
            theme::card(&mut c[0], |ui| {
                ui.label(theme::h2(t!("Per competitie", "By competition")));
                ui.add_space(8.0);
                for (comp, r) in s.by_comp() {
                    comp_row(ui, &comp, &r);
                }
            });
            theme::card(&mut c[1], |ui| {
                use chrono::Datelike;
                ui.horizontal(|ui| {
                    ui.label(theme::h2(t!("Doelpunten per maand", "Goals by month")));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        legend(ui, t!("tegen", "against"), theme::mix(DANGER, SURFACE, 0.25));
                        legend(ui, t!("voor", "for"), SUCCESS);
                    });
                });
                ui.add_space(8.0);
                let mut months: Vec<(chrono::NaiveDate, f32, f32)> = Vec::new();
                for m in &played {
                    let key = m.date.with_day(1).unwrap();
                    let (f, a) = m.goals().unwrap();
                    match months.last_mut() {
                        Some(l) if l.0 == key => {
                            l.1 += f as f32;
                            l.2 += a as f32;
                        }
                        _ => months.push((key, f as f32, a as f32)),
                    }
                }
                let groups: Vec<(String, f32, f32)> = months.iter().map(|(d, f, a)| (i18n::month_short(d.month()).to_string(), *f, *a)).collect();
                w::bars(ui, 190.0, &groups, (SUCCESS, theme::mix(DANGER, SURFACE, 0.25)));
                let runs = s.cup_runs();
                if !runs.is_empty() {
                    ui.add_space(8.0);
                    ui.label(theme::caps(t!("Toernooien", "Tournaments")));
                    ui.add_space(2.0);
                    for r in runs {
                        ui.horizontal(|ui| {
                            let (ic, col) = match r.won_final {
                                Some(true) => (icon::TROPHY, GOLD),
                                Some(false) => (icon::MEDAL, TEXT_DIM),
                                None => (icon::FLAG_PENNANT, INFO),
                            };
                            ui.label(theme::ic(ic).size(16.0).color(col));
                            ui.label(RichText::new(&r.comp).color(TEXT));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let text = match r.won_final {
                                    Some(true) => t!("Gewonnen", "Winners").to_string(),
                                    Some(false) => t!("Finalist", "Runners-up").to_string(),
                                    None => r.stage.clone(),
                                };
                                theme::pill(ui, &text, col);
                            });
                        });
                    }
                }
            });
        });
        let next = s.upcoming();
        if !next.is_empty() {
            ui.add_space(14.0);
            theme::card(ui, |ui| {
                ui.label(theme::h2(t!("Nog te spelen", "Still to play")));
                ui.add_space(4.0);
                for m in next.iter().take(6) {
                    match_row(ui, m, &s.team);
                }
            });
        }
    }

    // ------------------------------------------------------------------ wedstrijden

    fn page_matches(&mut self, ui: &mut Ui, ds: &Dataset) {
        let year = self.seasons(ds);
        let s = ds.season(year);
        self.header(ui, ds, t!("Wedstrijden", "Matches"), &format!("{} · {}", s.club, ds.season_label(year)), true);
        let mut comps: Vec<String> = Vec::new();
        for m in &s.matches {
            if !comps.contains(&m.comp) {
                comps.push(m.comp.clone());
            }
        }
        ui.horizontal_wrapped(|ui| {
            if chip(ui, t!("Alles", "All"), self.comp.is_none()).clicked() {
                self.comp = None;
            }
            for c in &comps {
                if chip(ui, &strip(c), self.comp.as_deref() == Some(c)).clicked() {
                    self.comp = Some(c.clone());
                }
            }
        });
        ui.add_space(10.0);
        let list: Vec<&Match> = s.matches.iter().copied().filter(|m| self.comp.as_ref().is_none_or(|c| *c == m.comp || !comps.contains(c))).collect();
        let rec = Record::of(list.iter().copied().filter(|m| m.played()));
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::h2(format!("{} {}", list.len(), t!("wedstrijden", "matches"))));
                ui.label(theme::dim(format!("{}–{}–{} · {}:{}", rec.w, rec.d, rec.l, rec.gf, rec.ga)));
            });
            ui.add_space(4.0);
            w::wdl_bar(ui, &rec, 6.0);
            ui.add_space(8.0);
            for m in list.iter().rev() {
                match_row(ui, m, &s.team);
            }
        });
    }

    // ------------------------------------------------------------------ stand

    fn page_table(&mut self, ui: &mut Ui, ds: &Dataset) {
        let year = self.seasons(ds);
        let s = ds.season(year);
        let sub = s.league.map(|l| format!("{} · {}", l.comp, ds.season_label(year))).unwrap_or_default();
        self.header(ui, ds, t!("Stand", "League table"), &sub, true);
        let Some(l) = s.league else {
            return w::empty(ui, icon::RANKING, t!("Geen competitie gevonden", "No league found"), t!("Voor dit seizoen zijn geen competitieduels gelezen.", "No league matches were read for this season."));
        };
        let rows = data::table(&l.games, None);
        theme::card(ui, |ui| {
            let width = ui.available_width();
            let cols = [0.0, 44.0, width - 470.0, width - 420.0, width - 370.0, width - 320.0, width - 255.0, width - 180.0, width - 110.0];
            let head = ["#", t!("Club", "Club"), t!("GS", "P"), t!("W", "W"), t!("G", "D"), t!("V", "L"), t!("Doelp.", "Goals"), "+/-", t!("Ptn", "Pts")];
            let (hr, _) = ui.allocate_exact_size(vec2(width, 24.0), Sense::hover());
            for (i, text) in head.iter().enumerate() {
                ui.painter().text(pos2(hr.left() + cols[i] + 12.0, hr.center().y), Align2::LEFT_CENTER, text.to_uppercase(), FontId::new(11.0, theme::semibold()), TEXT_FAINT);
            }
            let e = theme::enter(ui.ctx(), 0.6);
            for (i, r) in rows.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(width, 36.0), Sense::hover());
                let me = r.team == s.team;
                let p = ui.painter();
                if me {
                    p.rect_filled(rect, CornerRadius::same(10), theme::ACCENT_SOFT);
                    p.rect_filled(Rect::from_min_size(rect.min + vec2(0.0, 8.0), vec2(3.0, 20.0)), CornerRadius::same(2), ACCENT);
                } else if resp.hovered() {
                    p.rect_filled(rect, CornerRadius::same(10), SURFACE2);
                }
                let y = rect.center().y;
                let col = if me { Color32::WHITE } else { TEXT };
                let cell = |i: usize, text: String, c: Color32, bold: bool| {
                    p.text(pos2(rect.left() + cols[i] + 12.0, y), Align2::LEFT_CENTER, text, FontId::new(14.0, if bold { theme::semibold() } else { egui::FontFamily::Proportional }), c);
                };
                cell(0, (i + 1).to_string(), if i == 0 { GOLD } else { TEXT_DIM }, true);
                cell(1, r.team.clone(), col, me);
                cell(2, r.rec.p.to_string(), TEXT_DIM, false);
                cell(3, r.rec.w.to_string(), TEXT_DIM, false);
                cell(4, r.rec.d.to_string(), TEXT_DIM, false);
                cell(5, r.rec.l.to_string(), TEXT_DIM, false);
                cell(6, format!("{}:{}", r.rec.gf, r.rec.ga), TEXT_DIM, false);
                cell(7, format!("{:+}", r.rec.gd()), if r.rec.gd() >= 0 { SUCCESS } else { DANGER }, false);
                cell(8, ((r.rec.points() as f32 * e).round() as u32).to_string(), col, true);
            }
            ui.add_space(6.0);
            ui.label(theme::faint(if l.open > 0 {
                t!(format!("Berekend uit {} gespeelde wedstrijden; nog {} te spelen. Volgorde: punten, doelsaldo, doelpunten voor.", l.games.len(), l.open), format!("Computed from {} played matches; {} still to play. Order: points, goal difference, goals for.", l.games.len(), l.open))
            } else {
                t!(format!("Eindstand, berekend uit alle {} wedstrijden. Volgorde: punten, doelsaldo, doelpunten voor.", l.games.len()), format!("Final table, computed from all {} matches. Order: points, goal difference, goals for.", l.games.len()))
            }));
        });
    }

    // ------------------------------------------------------------------ carrière

    fn page_career(&mut self, ui: &mut Ui, ds: &Dataset) {
        let years = ds.season_years();
        let sub = match (years.first(), years.last()) {
            (Some(a), Some(b)) => {
                // alle clubs van de carrière, in volgorde
                let mut clubs: Vec<String> = Vec::new();
                for y in &years {
                    let c = ds.season(*y).club;
                    if clubs.last() != Some(&c) {
                        clubs.push(c);
                    }
                }
                format!("{} · {} – {}", clubs.join(" → "), ds.season_label(*a), ds.season_label(*b))
            }
            _ => ds.club.clone(),
        };
        self.header(ui, ds, t!("Carrière", "Career"), &sub, false);
        let all = ds.competitive();
        if all.is_empty() {
            return w::empty(ui, icon::TROPHY, t!("Nog geen carrière", "No career yet"), t!("Speel je eerste officiële wedstrijd en je kroniek begint.", "Play your first competitive match and your chronicle begins."));
        }
        let rec = Record::of(all.iter().copied());
        let trophies = ds.trophies();
        ui.columns(5, |c| {
            theme::stat_tile(&mut c[0], icon::TROPHY, t!("Prijzen", "Trophies"), &trophies.len().to_string(), t!("titels, bekers, supercups", "leagues, cups, super cups"), GOLD, None);
            theme::stat_tile(&mut c[1], icon::SOCCER_BALL, t!("Wedstrijden", "Matches"), &rec.p.to_string(), &format!("{} {}", years.len(), t!("seizoenen", "seasons")), INFO, None);
            theme::stat_tile(&mut c[2], icon::PERCENT, t!("Gewonnen", "Win rate"), &format!("{:.0}%", rec.win_pct()), &format!("{}–{}–{}", rec.w, rec.d, rec.l), SUCCESS, Some(rec.win_pct()));
            theme::stat_tile(&mut c[3], icon::TARGET, t!("Doelpunten", "Goals"), &rec.gf.to_string(), &format!("{:.2} {}", rec.gf as f32 / rec.p.max(1) as f32, t!("per duel", "per game")), WARNING, None);
            theme::stat_tile(&mut c[4], icon::STAR, t!("Punten per duel", "Points per game"), &format!("{:.2}", rec.ppg()), t!("alle competities", "all competitions"), ACCENT, Some(100.0 * rec.ppg() / 3.0));
        });
        ui.add_space(14.0);

        theme::card(ui, |ui| {
            ui.label(theme::h2(t!("Prijzenkast", "Trophy cabinet")));
            ui.add_space(8.0);
            if trophies.is_empty() {
                ui.label(theme::dim(t!("Nog leeg. De eerste prijs komt hier te staan.", "Still empty. Your first trophy will stand here.")));
            }
            for row in trophies.chunks(4) {
                ui.columns(4, |c| {
                    for (i, tr) in row.iter().enumerate() {
                        egui::Frame::new().fill(theme::mix(SURFACE2, GOLD, 0.06)).stroke(Stroke::new(1.0, theme::mix(BORDER, GOLD, 0.3))).corner_radius(CornerRadius::same(14)).inner_margin(egui::Margin::symmetric(14, 12)).show(&mut c[i], |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(theme::ic(icon::TROPHY).size(28.0).color(GOLD));
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 1.0;
                                    ui.add(egui::Label::new(RichText::new(strip(&tr.comp)).font(FontId::new(14.0, theme::semibold())).color(TEXT)).truncate());
                                    ui.label(theme::faint(ds.season_label(tr.season)));
                                    ui.add(egui::Label::new(RichText::new(if tr.detail.is_empty() { t!("Landskampioen", "League champions") } else { tr.detail.as_str() }).size(11.5).color(TEXT_DIM)).truncate());
                                });
                            });
                        });
                    }
                });
                ui.add_space(2.0);
            }
        });
        ui.add_space(14.0);

        ui.columns(2, |c| {
            theme::card(&mut c[0], |ui| {
                ui.label(theme::h2(t!("Seizoen na seizoen", "Season by season")));
                ui.add_space(8.0);
                for y in years.iter().rev() {
                    let s = ds.season(*y);
                    let r = Record::of(s.played().iter().copied());
                    let pos = s.league.and_then(|l| data::position(&data::table(&l.games, None), &s.team));
                    let won = trophies.iter().filter(|t| t.season == *y).count();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(ds.season_label(*y)).font(FontId::new(15.0, theme::semibold())).color(TEXT));
                        ui.label(theme::dim(&s.team));
                        if let Some(n) = pos {
                            theme::pill(ui, &format!("{} {}", w::ordinal(n), t!("plaats", "place")), if n == 1 { GOLD } else { ACCENT_HI });
                        }
                        for _ in 0..won {
                            ui.label(theme::ic(icon::TROPHY).size(15.0).color(GOLD));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(theme::dim(format!("{}–{}–{} · {}:{} · {:.0}%", r.w, r.d, r.l, r.gf, r.ga, r.win_pct())));
                        });
                    });
                    w::wdl_bar(ui, &r, 7.0);
                    ui.add_space(8.0);
                }
                if years.len() > 1 {
                    let pts: Vec<f32> = years.iter().map(|y| Record::of(ds.season(*y).played().iter().copied()).win_pct()).collect();
                    let labels: Vec<(usize, String)> = years.iter().enumerate().map(|(i, y)| (i, ds.season_label(*y))).collect();
                    ui.label(theme::caps(t!("Winstpercentage per seizoen", "Win rate by season")));
                    w::line_chart(ui, 130.0, &[Series { points: &pts, color: SUCCESS, fill: true }], (0.0, 100.0), false, &labels, &|v| format!("{v:.0}%"), &[]);
                }
            });
            theme::card(&mut c[1], |ui| {
                ui.label(theme::h2(t!("Records aller tijden", "All-time records")));
                ui.add_space(6.0);
                records(ui, &all);
            });
        });
        ui.add_space(14.0);

        let h2h = data::head_to_head(&all);
        theme::card(ui, |ui| {
            ui.label(theme::h2(t!("Tegenstanders", "Opponents")));
            ui.add_space(6.0);
            let regular: Vec<&(String, Record)> = h2h.iter().filter(|x| x.1.p >= 3).collect();
            let best = regular.iter().max_by(|a, b| a.1.ppg().total_cmp(&b.1.ppg()).then(a.1.gd().cmp(&b.1.gd())));
            let worst = regular.iter().min_by(|a, b| a.1.ppg().total_cmp(&b.1.ppg()).then(a.1.gd().cmp(&b.1.gd())));
            ui.columns(2, |c| {
                if let Some((n, r)) = best {
                    w::record_card(&mut c[0], icon::SMILEY, SUCCESS, t!("Favoriete tegenstander", "Favourite opponent"), n, &format!("{}–{}–{} · {}:{}", r.w, r.d, r.l, r.gf, r.ga));
                }
                if let Some((n, r)) = worst {
                    w::record_card(&mut c[1], icon::SKULL, DANGER, t!("Lastigste tegenstander", "Bogey team"), n, &format!("{}–{}–{} · {}:{}", r.w, r.d, r.l, r.gf, r.ga));
                }
            });
            ui.add_space(10.0);
            ui.label(theme::caps(t!("Vaakst tegen gespeeld", "Most played")));
            ui.add_space(4.0);
            ui.columns(2, |c| {
                let half = h2h.len().min(16).div_ceil(2);
                for (i, (name, r)) in h2h.iter().take(16).enumerate() {
                    comp_row(&mut c[if i < half { 0 } else { 1 }], name, r);
                }
            });
        });
    }

    // ------------------------------------------------------------------ selectie

    fn page_squad(&mut self, ui: &mut Ui, ds: &Dataset) {
        self.header(ui, ds, t!("Selectie", "Squad"), t!("Waarde en ontwikkeling sinds de app meekijkt", "Value and development since the app started watching"), false);
        let snaps = ds.club_snapshots();
        let (Some(first), Some(last)) = (snaps.first(), snaps.last()) else {
            return w::empty(ui, icon::USERS_THREE, t!("Nog geen momentopname", "No snapshot yet"), t!("Verbind met FM27; daarna legt de app elke speelweek je selectie vast.", "Connect to FM27; the app then records your squad every game week."));
        };
        let total = |s: &data::Snapshot| s.players.iter().map(|p| p.value.max(0)).sum::<i64>();
        let avg = |s: &data::Snapshot| {
            let v: Vec<f32> = s.players.iter().filter(|p| p.overall > 0.0).map(|p| p.overall).collect();
            if v.is_empty() { 0.0 } else { v.iter().sum::<f32>() / v.len() as f32 }
        };
        ui.columns(4, |c| {
            theme::stat_tile(&mut c[0], icon::COINS, t!("Selectiewaarde", "Squad value"), &self.cash(total(last)), &if snaps.len() > 1 { self.delta(total(last) - total(first)) } else { t!("som van alle spelers", "sum of all players").to_string() }, SUCCESS, None);
            theme::stat_tile(&mut c[1], icon::USERS_THREE, t!("Spelers", "Players"), &last.players.len().to_string(), &format!("{} {}", snaps.len(), t!("momentopnames", "snapshots")), INFO, None);
            theme::stat_tile(&mut c[2], icon::WALLET, t!("Loonsom per week", "Weekly wages"), &self.cash(last.players.iter().map(|p| p.wage).sum()), t!("alle spelers samen", "all players together"), WARNING, None);
            theme::stat_tile(&mut c[3], icon::GAUGE, t!("Gemiddeld niveau", "Average level"), &format!("{:.1}", avg(last)), t!("beste 20 attributen, van 20", "best 20 attributes, out of 20"), ACCENT, Some(avg(last) * 5.0));
        });
        ui.add_space(14.0);
        if snaps.len() > 1 {
            theme::card(ui, |ui| {
                ui.label(theme::h2(t!("Selectiewaarde door de tijd", "Squad value over time")));
                ui.add_space(6.0);
                let pts: Vec<f32> = snaps.iter().map(|s| total(s) as f32 / 1e6 * if self.settings.euro { self.settings.rate } else { 1.0 }).collect();
                let (lo, hi) = pts.iter().fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
                let step = (snaps.len() / 6).max(1);
                let labels: Vec<(usize, String)> = snaps.iter().enumerate().filter(|(i, _)| i % step == 0).map(|(i, s)| (i, i18n::date_short(s.date))).collect();
                let tips: Vec<String> = snaps.iter().map(|s| format!("{}\n{}", i18n::date(s.date), self.cash(total(s)))).collect();
                w::line_chart(ui, 170.0, &[Series { points: &pts, color: SUCCESS, fill: true }], (lo * 0.97, hi * 1.03), false, &labels, &|v| format!("{v:.0}M"), &tips);
            });
        } else {
            theme::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::ic(icon::HOURGLASS_MEDIUM).size(18.0).color(INFO));
                    ui.add(egui::Label::new(theme::dim(t!(
                        "Dit is de eerste momentopname. Speel door met de app open: elke speelweek komt er een bij, en dan zie je hier wie zich ontwikkelt, wie in waarde stijgt en wie er kwam of ging.",
                        "This is the first snapshot. Keep playing with the app open: one is added every game week, and then you see here who develops, whose value rises and who arrived or left."
                    )))
                    .wrap());
                });
            });
        }
        ui.add_space(14.0);
        let arrived: Vec<&data::Player> = last.players.iter().filter(|p| !first.players.iter().any(|o| o.uid == p.uid)).collect();
        let left: Vec<&data::Player> = first.players.iter().filter(|p| !last.players.iter().any(|o| o.uid == p.uid)).collect();
        if !arrived.is_empty() || !left.is_empty() {
            ui.columns(2, |c| {
                for (i, (title, list, col, ic)) in [(t!("Gekomen", "Arrived"), &arrived, SUCCESS, icon::ARROW_CIRCLE_DOWN_RIGHT), (t!("Vertrokken", "Left"), &left, DANGER, icon::ARROW_CIRCLE_UP_RIGHT)].into_iter().enumerate() {
                    theme::card(&mut c[i], |ui| {
                        ui.label(theme::h2(format!("{title} ({})", list.len())));
                        ui.label(theme::faint(format!("{} {}", t!("sinds", "since"), i18n::date(first.date))));
                        for p in list.iter() {
                            ui.horizontal(|ui| {
                                ui.label(theme::ic(ic).size(15.0).color(col));
                                ui.label(RichText::new(&p.name).color(TEXT));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(theme::dim(self.cash(p.value))));
                            });
                        }
                    });
                }
            });
            ui.add_space(14.0);
        }
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::h2(t!("Spelers", "Players")));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(theme::faint(t!("waarde · niveau · loon", "value · level · wage")));
                });
            });
            ui.add_space(6.0);
            let mut players: Vec<&data::Player> = last.players.iter().collect();
            players.sort_by_key(|p| std::cmp::Reverse(p.value));
            let width = ui.available_width();
            let max = players.first().map(|p| p.value.max(1)).unwrap_or(1) as f32;
            let e = theme::enter(ui.ctx(), 0.7);
            for p in players {
                // de eerste momentopname waarin deze speler voorkomt
                let then = snaps.iter().find_map(|s| s.players.iter().find(|o| o.uid == p.uid));
                let (rect, resp) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
                let pa = ui.painter();
                if resp.hovered() {
                    pa.rect_filled(rect, CornerRadius::same(8), SURFACE2);
                }
                let y = rect.center().y;
                pa.text(pos2(rect.left() + 10.0, y), Align2::LEFT_CENTER, &p.name, FontId::new(14.0, theme::medium()), TEXT);
                let bar = Rect::from_min_size(pos2(rect.left() + 230.0, y - 3.0), vec2((width - 640.0).max(60.0), 6.0));
                pa.rect_filled(bar, CornerRadius::same(3), theme::TRACK);
                let mut f = bar;
                f.set_width(bar.width() * (p.value.max(0) as f32 / max) * e);
                pa.rect_filled(f, CornerRadius::same(3), SUCCESS);
                pa.text(pos2(rect.right() - 300.0, y), Align2::RIGHT_CENTER, self.cash(p.value), FontId::new(14.0, theme::semibold()), TEXT);
                if let Some(o) = then.filter(|o| o.value != p.value) {
                    let d = p.value - o.value;
                    pa.text(pos2(rect.right() - 292.0, y), Align2::LEFT_CENTER, self.delta(d), FontId::new(12.0, egui::FontFamily::Proportional), if d >= 0 { SUCCESS } else { DANGER });
                }
                if p.overall > 0.0 {
                    pa.text(pos2(rect.right() - 150.0, y), Align2::RIGHT_CENTER, format!("{:.1}", p.overall), FontId::new(14.0, theme::semibold()), ACCENT_HI);
                    if let Some(o) = then.filter(|o| o.overall > 0.0 && (o.overall - p.overall).abs() >= 0.05) {
                        let d = p.overall - o.overall;
                        pa.text(pos2(rect.right() - 142.0, y), Align2::LEFT_CENTER, format!("{d:+.1}"), FontId::new(12.0, egui::FontFamily::Proportional), if d >= 0.0 { SUCCESS } else { DANGER });
                    }
                }
                pa.text(pos2(rect.right() - 10.0, y), Align2::RIGHT_CENTER, format!("{}/wk", self.cash(p.wage)), FontId::new(12.5, egui::FontFamily::Proportional), TEXT_DIM);
            }
        });
    }

    // ------------------------------------------------------------------ financiën

    fn page_money(&mut self, ui: &mut Ui, ds: &Dataset) {
        self.header(ui, ds, t!("Financiën", "Finances"), t!("Saldo en budgetten door de tijd", "Balance and budgets over time"), false);
        let snaps = ds.club_snapshots();
        let (Some(first), Some(last)) = (snaps.first(), snaps.last()) else {
            return w::empty(ui, icon::CHART_LINE_UP, t!("Nog geen momentopname", "No snapshot yet"), t!("Verbind met FM27; daarna legt de app elke speelweek de financiën vast.", "Connect to FM27; the app then records the finances every game week."));
        };
        ui.columns(3, |c| {
            theme::stat_tile(&mut c[0], icon::BANK, t!("Saldo", "Balance"), &self.cash(last.balance), &self.delta(last.balance - first.balance), if last.balance >= 0 { SUCCESS } else { DANGER }, None);
            theme::stat_tile(&mut c[1], icon::ARROWS_LEFT_RIGHT, t!("Transferbudget", "Transfer budget"), &self.cash(last.transfer_budget), &self.delta(last.transfer_budget - first.transfer_budget), INFO, None);
            theme::stat_tile(&mut c[2], icon::WALLET, t!("Loonbudget per week", "Weekly wage budget"), &self.cash(last.wage_budget), &self.delta(last.wage_budget - first.wage_budget), WARNING, None);
        });
        ui.add_space(14.0);
        if snaps.len() < 2 {
            theme::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::ic(icon::HOURGLASS_MEDIUM).size(18.0).color(INFO));
                    ui.add(egui::Label::new(theme::dim(t!(
                        "Dit is de eerste momentopname. Speel door met de app open: elke speelweek komt er een punt bij en dan groeit hier de grafiek van je saldo en budgetten.",
                        "This is the first snapshot. Keep playing with the app open: a point is added every game week and the chart of your balance and budgets grows here."
                    )))
                    .wrap());
                });
            });
            return;
        }
        let k = if self.settings.euro { self.settings.rate } else { 1.0 } / 1e6;
        let step = (snaps.len() / 6).max(1);
        let labels: Vec<(usize, String)> = snaps.iter().enumerate().filter(|(i, _)| i % step == 0).map(|(i, s)| (i, i18n::date_short(s.date))).collect();
        let charts: [(&str, fn(&data::Snapshot) -> i64, Color32); 2] = [(t!("Saldo", "Balance"), |s| s.balance, SUCCESS), (t!("Transferbudget", "Transfer budget"), |s| s.transfer_budget, INFO)];
        for (title, get, col) in charts {
            theme::card(ui, |ui| {
                ui.label(theme::h2(title));
                ui.add_space(6.0);
                let pts: Vec<f32> = snaps.iter().map(|s| get(s) as f32 * k).collect();
                let (lo, hi) = pts.iter().fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
                let pad = ((hi - lo) * 0.08).max(0.5);
                let tips: Vec<String> = snaps.iter().map(|s| format!("{}\n{}", i18n::date(s.date), self.cash(get(s)))).collect();
                w::line_chart(ui, 170.0, &[Series { points: &pts, color: col, fill: true }], (lo - pad, hi + pad), false, &labels, &|v| format!("{v:.0}M"), &tips);
            });
            ui.add_space(14.0);
        }
    }

    // ------------------------------------------------------------------ deelkaart

    fn page_card(&mut self, ui: &mut Ui, ds: &Dataset) {
        let year = self.seasons(ds);
        theme::page_header(ui, t!("Deelkaart", "Share card"), t!("Je seizoen of je hele carrière in één beeld, klaar om te delen", "Your season or your whole career in one image, ready to share"), |ui| {
            if theme::primary_button(ui, icon::DOWNLOAD_SIMPLE, t!("Bewaar als PNG", "Save as PNG"), true).clicked() {
                self.want_card = true;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            if !self.card_career {
                self.season_picker(ui, ds);
            }
            if theme::segmented(ui, &mut self.card_career, &[(false, t!("Seizoen", "Season")), (true, t!("Carrière", "Career"))]) {
                theme::restart_enter(ui.ctx());
            }
        });
        if let Some((msg, at)) = &self.notice {
            if at.elapsed() < Duration::from_secs(8) {
                ui.label(RichText::new(msg).color(SUCCESS));
            }
        }
        let s = ds.season(year);
        let played = if self.card_career { ds.competitive() } else { s.played() };
        if played.is_empty() {
            return w::empty(ui, icon::SHARE_NETWORK, t!("Nog niets te delen", "Nothing to share yet"), "");
        }
        let card = if self.card_career { career_card(ds, &played) } else { season_card(ds, &s, &played, year) };
        let h = (ui.available_height() - 12.0).clamp(420.0, 760.0);
        let k = h / 700.0;
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(560.0 * k, h), Sense::hover());
            self.card_rect = Some(rect);
            draw_card(&ui.painter().with_clip_rect(rect.expand(1.0)), rect, k, &card);
        });
    }

    // ------------------------------------------------------------------ instellingen

    fn page_settings(&mut self, ui: &mut Ui) {
        theme::page_header(ui, t!("Instellingen", "Settings"), "", |_| {});
        let mut dirty = false;
        theme::card(ui, |ui| {
            ui.label(theme::h2(t!("Algemeen", "General")));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(t!("Taal", "Language"));
                if theme::segmented(ui, &mut self.settings.lang, &[(Lang::Nl, "Nederlands"), (Lang::En, "English")]) {
                    i18n::set(self.settings.lang);
                    dirty = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label(t!("Bedragen", "Amounts"));
                dirty |= theme::segmented(ui, &mut self.settings.euro, &[(true, "€"), (false, "£")]);
                if self.settings.euro {
                    ui.label(theme::dim(t!("koers € per £", "rate € per £")));
                    dirty |= ui.add(egui::DragValue::new(&mut self.settings.rate).speed(0.01).range(0.5..=2.0)).changed();
                }
            });
        });
        ui.add_space(14.0);
        theme::card(ui, |ui| {
            ui.label(theme::h2(t!("Verbinding met FM27", "Connection to FM27")));
            ui.add_space(6.0);
            ui.add(egui::Label::new(theme::dim(if cfg!(target_os = "macos") {
                t!(
                    "Op de Mac leest de app via een module die met FM27 meestart. Sluit FM27, klik op de knop en laad je save. Heb je FM gestart met FM27 Trainer, dan verbindt deze app vanzelf.",
                    "On the Mac the app reads through a module that starts with FM27. Quit FM27, click the button and load your save. If you started FM with FM27 Trainer, this app connects by itself."
                )
            } else {
                t!("Start FM27 en laad je save; de app leest het spel van buitenaf en verbindt vanzelf.", "Start FM27 and load your save; the app reads the game from outside and connects by itself.")
            }))
            .wrap());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if theme::primary_button(ui, icon::PLAY, t!("Start FM27", "Start FM27"), !fm::fm_running()).clicked() {
                    match fm::start_fm() {
                        Ok(m) => (self.status, self.error) = (m, false),
                        Err(e) => (self.status, self.error) = (e.to_string(), true),
                    }
                }
                if theme::button(ui, icon::FOLDER_OPEN, t!("Open archiefmap", "Open archive folder"), true).clicked() {
                    let _ = std::fs::create_dir_all(store::dir());
                    open_path(&store::dir());
                }
            });
            if !self.status.is_empty() {
                ui.add_space(4.0);
                ui.add(egui::Label::new(RichText::new(&self.status).color(if self.error { DANGER } else { TEXT_DIM })).wrap());
            }
        });
        ui.add_space(14.0);
        theme::card(ui, |ui| {
            ui.label(theme::h2(t!("Over", "About")));
            ui.add(egui::Label::new(theme::dim(t!(
                "FM27 Chronicle leest alleen en verandert niets aan je save. Wedstrijden, competities en finales komen rechtstreeks uit FM27; de stand wordt uit de uitslagen berekend. Selectie en financiën worden vanaf nu elke speelweek vastgelegd. Onafhankelijk fanproject, niet verbonden aan Sports Interactive of SEGA. Lettertype: Inter (SIL OFL). Iconen: Phosphor (MIT).",
                "FM27 Chronicle only reads and never changes your save. Matches, competitions and finals come straight from FM27; the table is computed from the results. Squad and finances are recorded every game week from now on. Independent fan project, not affiliated with Sports Interactive or SEGA. Font: Inter (SIL OFL). Icons: Phosphor (MIT)."
            )))
            .wrap());
            ui.add_space(6.0);
            if theme::button(ui, icon::HEART, t!("Steun de ontwikkeling (Ko-fi)", "Support development (Ko-fi)"), true).clicked() {
                open_path(std::path::Path::new("https://ko-fi.com/semhiel"));
            }
        });
        if dirty {
            self.save_settings();
        }
    }

    fn welcome(&mut self, ui: &mut Ui) {
        ui.add_space(90.0);
        ui.vertical_centered(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(84.0, 84.0), Sense::hover());
            w::glow(ui.painter(), r.center(), 120.0, ACCENT, 0.5);
            w::gradient(ui.painter(), r, ACCENT, Color32::from_rgb(255, 120, 190), 24);
            ui.painter().text(r.center(), Align2::CENTER_CENTER, icon::BOOK_OPEN_TEXT, FontId::new(44.0, theme::icons()), Color32::WHITE);
            ui.add_space(18.0);
            ui.label(theme::title(t!("Je carrière, vastgelegd", "Your career, on record")));
            ui.add_space(4.0);
            ui.label(theme::dim(t!(
                "FM27 Chronicle leest je save live uit en bouwt er je seizoensoverzicht, prijzenkast en records van.",
                "FM27 Chronicle reads your save live and builds your season review, trophy cabinet and records from it."
            )));
            ui.add_space(18.0);
            if let Some(p) = &self.progress {
                ui.spinner();
                ui.label(theme::dim(p));
            } else if fm::module_ready() {
                if theme::primary_button(ui, icon::ARROWS_CLOCKWISE, t!("Lees mijn save", "Read my save"), self.rx.is_none()).clicked() {
                    self.refresh(ui.ctx(), true);
                }
            } else {
                if theme::primary_button(ui, icon::PLAY, t!("Start FM27", "Start FM27"), !fm::fm_running()).clicked() {
                    match fm::start_fm() {
                        Ok(m) => (self.status, self.error) = (m, false),
                        Err(e) => (self.status, self.error) = (e.to_string(), true),
                    }
                }
                ui.add_space(6.0);
                ui.label(theme::faint(if cfg!(target_os = "macos") && fm::fm_running() {
                    t!("FM27 draait zonder de module. Sla op, sluit FM27 en start het hier opnieuw.", "FM27 is running without the module. Save, quit FM27 and start it again from here.")
                } else {
                    t!("Laad daarna je save; de app verbindt vanzelf.", "Then load your save; the app connects by itself.")
                }));
            }
            if !self.status.is_empty() {
                ui.add_space(10.0);
                ui.label(RichText::new(&self.status).color(if self.error { DANGER } else { TEXT_DIM }));
            }
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.sidebar(ctx);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(28, 24))).show(ctx, |ui| {
            let fade = theme::enter(ctx, 0.25);
            ui.set_opacity(0.35 + 0.65 * fade);
            if self.page == Page::Settings {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.page_settings(ui));
                return;
            }
            let Some(ds) = self.ds.clone() else {
                return self.welcome(ui);
            };
            if self.page == Page::Card {
                return self.page_card(ui, &ds);
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                match self.page {
                    Page::Season => self.page_season(ui, &ds),
                    Page::Matches => self.page_matches(ui, &ds),
                    Page::Table => self.page_table(ui, &ds),
                    Page::Career => self.page_career(ui, &ds),
                    Page::Squad => self.page_squad(ui, &ds),
                    Page::Money => self.page_money(ui, &ds),
                    Page::Card | Page::Settings => {}
                }
                ui.add_space(20.0);
            });
        });
        self.screenshots(ctx);
    }
}

// ---- losse stukken ---------------------------------------------------------------------------

fn open_path(p: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(p).status();
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(p).status();
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = p;
}

fn shorten(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n - 1).collect::<String>()) }
}

fn thousands(v: u32) -> String {
    let s = v.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(if i18n::is_nl() { '.' } else { ',' });
        }
        out.push(ch);
    }
    out
}

fn legend(ui: &mut Ui, text: &str, c: Color32) {
    ui.label(theme::faint(text));
    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(3), c);
}

fn chip(ui: &mut Ui, text: &str, selected: bool) -> egui::Response {
    let b = egui::Button::new(RichText::new(text).font(FontId::new(13.0, theme::medium())).color(if selected { Color32::WHITE } else { TEXT_DIM }))
        .fill(if selected { ACCENT } else { SURFACE2 })
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(16))
        .min_size(vec2(0.0, 30.0));
    ui.add(b).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Naam, record en een winst/gelijk/verlies-balk.
fn comp_row(ui: &mut Ui, name: &str, r: &Record) {
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(RichText::new(strip(name)).font(FontId::new(14.0, theme::medium())).color(TEXT)).truncate());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(theme::dim(format!("{}–{}–{} · {}:{}", r.w, r.d, r.l, r.gf, r.ga)));
        });
    });
    w::wdl_bar(ui, r, 6.0);
    ui.add_space(6.0);
}

/// De recordkaartjes van een reeks wedstrijden (een seizoen of de hele carrière).
fn records(ui: &mut Ui, played: &[&Match]) {
    let line = |m: &Match| format!("{} {} {} · {}", m.home, m.score(), m.away, i18n::date(m.date));
    let span = |a: &Match, b: &Match| format!("{} – {}", i18n::date(a.date), i18n::date(b.date));
    let margin = |m: &&&Match| {
        let (f, a) = m.goals().unwrap();
        (f as i32 - a as i32, f as i32)
    };
    let mut cards: Vec<(&str, Color32, String, String, String)> = Vec::new();
    if let Some(m) = played.iter().filter(|m| m.result() == Some(Res::W)).max_by_key(margin) {
        cards.push((icon::FIRE, SUCCESS, t!("Grootste zege", "Biggest win").into(), m.score(), line(m)));
    }
    if let Some(m) = played.iter().filter(|m| m.result() == Some(Res::L)).min_by_key(margin) {
        cards.push((icon::CLOUD_RAIN, DANGER, t!("Zwaarste nederlaag", "Heaviest defeat").into(), m.score(), line(m)));
    }
    if let Some((n, a, b)) = data::longest(played, |m| m.result() == Some(Res::W)).filter(|x| x.0 > 1) {
        cards.push((icon::LIGHTNING, WARNING, t!("Langste zegereeks", "Longest winning run").into(), format!("{n} {}", t!("op rij", "in a row")), span(a, b)));
    }
    if let Some((n, a, b)) = data::longest(played, |m| m.result() != Some(Res::L)).filter(|x| x.0 > 1) {
        cards.push((icon::SHIELD_CHECK, INFO, t!("Ongeslagen reeks", "Unbeaten run").into(), format!("{n} {}", t!("duels", "games")), span(a, b)));
    }
    if let Some(m) = played.iter().max_by_key(|m| m.goals().map(|(f, a)| f as u32 + a as u32).unwrap_or(0)) {
        cards.push((icon::CONFETTI, ACCENT_HI, t!("Meeste doelpunten in één duel", "Most goals in a match").into(), m.score(), line(m)));
    }
    if let Some(m) = played.iter().filter(|m| m.at_home && m.attendance > 0).max_by_key(|m| m.attendance) {
        cards.push((icon::USERS_FOUR, TEXT_DIM, t!("Meeste toeschouwers thuis", "Biggest home crowd").into(), thousands(m.attendance), format!("vs {} · {}", m.away, i18n::date(m.date))));
    }
    let clean = played.iter().filter(|m| m.goals().is_some_and(|g| g.1 == 0)).count();
    cards.push((icon::LOCK_KEY, SUCCESS, t!("De nul gehouden", "Clean sheets").into(), clean.to_string(), format!("{:.0}% {}", 100.0 * clean as f32 / played.len().max(1) as f32, t!("van de duels", "of matches"))));
    let cur = data::current(played, |m| m.result() != Some(Res::L));
    cards.push((icon::TREND_UP, ACCENT, t!("Huidige reeks", "Current run").into(), format!("{cur} {}", t!("ongeslagen", "unbeaten")), String::new()));
    for pair in cards.chunks(2) {
        ui.columns(2, |c| {
            for (i, (ic, col, title, value, detail)) in pair.iter().enumerate() {
                w::record_card(&mut c[i], ic, *col, title, value, detail);
            }
        });
        ui.add_space(2.0);
    }
}

/// Eén wedstrijd op een regel: datum, competitie, thuis – uitslag – uit, toeschouwers.
fn match_row(ui: &mut Ui, m: &Match, club: &str) {
    let club = if m.team.is_empty() { club } else { m.team.as_str() };
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 40.0), Sense::hover());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(10), SURFACE2);
    }
    let y = rect.center().y;
    let col = m.result().map(w::res_color).unwrap_or(TEXT_FAINT);
    p.rect_filled(Rect::from_center_size(pos2(rect.left() + 6.0, y), vec2(4.0, 22.0)), CornerRadius::same(2), col);
    p.text(pos2(rect.left() + 20.0, y), Align2::LEFT_CENTER, i18n::date(m.date), FontId::new(12.5, egui::FontFamily::Proportional), TEXT_DIM);
    let mut comp = m.comp_short.clone();
    if !m.stage.is_empty() {
        comp = format!("{comp} · {}", m.stage);
    }
    if !m.leg.is_empty() {
        comp = format!("{comp} · {}", m.leg);
    }
    if m.is_final {
        p.text(pos2(rect.left() + 118.0, y), Align2::LEFT_CENTER, icon::TROPHY, FontId::new(14.0, theme::icons()), GOLD);
    }
    p.text(pos2(rect.left() + if m.is_final { 138.0 } else { 118.0 }, y), Align2::LEFT_CENTER, shorten(&comp, 52), FontId::new(12.5, egui::FontFamily::Proportional), if m.is_final { GOLD } else { TEXT_FAINT });
    let mid = rect.left() + width * 0.62;
    let font = |team: &str| FontId::new(14.0, if team == club { theme::bold() } else { egui::FontFamily::Proportional });
    p.text(pos2(mid - 40.0, y), Align2::RIGHT_CENTER, &m.home, font(&m.home), if m.home == club { Color32::WHITE } else { TEXT });
    p.text(pos2(mid + 40.0, y), Align2::LEFT_CENTER, &m.away, font(&m.away), if m.away == club { Color32::WHITE } else { TEXT });
    let box_r = Rect::from_center_size(pos2(mid, y), vec2(58.0, 26.0));
    p.rect_filled(box_r, CornerRadius::same(8), if m.played() { theme::tint(col, 0.22) } else { SURFACE2 });
    p.text(box_r.center(), Align2::CENTER_CENTER, m.score(), FontId::new(14.0, theme::bold()), if m.played() { col } else { TEXT_FAINT });
    let mut extra = String::new();
    if let Some((a, b)) = m.pens {
        extra = format!("{} {a}–{b}", t!("n.s.", "pens"));
    } else if let Some((a, b)) = m.agg {
        extra = format!("{} {a}–{b}", t!("totaal", "agg"));
    } else if m.attendance > 0 && m.played() {
        extra = thousands(m.attendance);
    }
    p.text(pos2(rect.right() - 12.0, y), Align2::RIGHT_CENTER, extra, FontId::new(12.0, egui::FontFamily::Proportional), TEXT_FAINT);
}

/// Wat er op een deelkaart staat (een seizoen of de hele carrière).
struct CardData<'a> {
    kicker: String,
    title: String,
    subtitle: String,
    gold: bool,
    stats: [(String, String); 4],
    trophies: Vec<(String, String)>,
    lines: Vec<(String, String)>,
    strip_title: String,
    played: &'a [&'a Match],
    footer: String,
}

/// De regels onder "Hoogtepunten", uit een reeks gespeelde wedstrijden.
fn highlight_lines(played: &[&Match]) -> Vec<(String, String)> {
    let mut lines: Vec<(String, String)> = Vec::new();
    if let Some(m) = played.iter().filter(|m| m.result() == Some(Res::W)).max_by_key(|m| m.goals().map(|(a, b)| (a as i32 - b as i32, a as i32)).unwrap()) {
        lines.push((t!("Grootste zege", "Biggest win").into(), format!("{} {} {}", m.home, m.score(), m.away)));
    }
    if let Some((n, _, _)) = data::longest(played, |m| m.result() == Some(Res::W)).filter(|x| x.0 > 1) {
        lines.push((t!("Langste zegereeks", "Longest winning run").into(), format!("{n} {}", t!("op rij", "in a row"))));
    }
    if let Some((n, _, _)) = data::longest(played, |m| m.result() != Some(Res::L)).filter(|x| x.0 > 1) {
        lines.push((t!("Ongeslagen reeks", "Unbeaten run").into(), format!("{n} {}", t!("duels", "games"))));
    }
    let clean = played.iter().filter(|m| m.goals().is_some_and(|g| g.1 == 0)).count();
    lines.push((t!("De nul gehouden", "Clean sheets").into(), clean.to_string()));
    lines
}

fn season_card<'a>(ds: &Dataset, s: &Season, played: &'a [&'a Match], year: u16) -> CardData<'a> {
    let rows = s.league.map(|l| data::table(&l.games, None)).unwrap_or_default();
    let pos = data::position(&rows, &s.team);
    let champion = pos == Some(1) && s.league.is_some_and(|l| l.open == 0);
    let all = Record::of(played.iter().copied());
    let lg = Record::of(s.league_matches().iter().copied());
    let subtitle = match (pos, s.league) {
        (Some(_), Some(l)) if champion => format!("{} · {}", t!("Kampioen", "Champions"), strip(&l.comp)),
        (Some(n), Some(l)) => format!("{} {} · {}", w::ordinal(n), t!("plaats", "place"), strip(&l.comp)),
        _ => String::new(),
    };
    CardData {
        kicker: format!("{} {}", t!("SEIZOEN", "SEASON"), ds.season_label(year)),
        title: s.club.clone(),
        subtitle,
        gold: champion,
        stats: [
            (lg.points().to_string(), t!("punten", "points").into()),
            (format!("{}–{}–{}", all.w, all.d, all.l), t!("w–g–v", "w–d–l").into()),
            (format!("{}:{}", all.gf, all.ga), t!("doelpunten", "goals").into()),
            (format!("{:.0}%", all.win_pct()), t!("gewonnen", "win rate").into()),
        ],
        trophies: ds.trophies().into_iter().filter(|t| t.season == year).map(|t| (strip(&t.comp), t.detail)).collect(),
        lines: highlight_lines(played),
        strip_title: t!("HET SEIZOEN, DUEL VOOR DUEL", "THE SEASON, MATCH BY MATCH").into(),
        played,
        footer: format!("{} {}", all.p, t!("wedstrijden", "matches")),
    }
}

fn career_card<'a>(ds: &Dataset, played: &'a [&'a Match]) -> CardData<'a> {
    let years = ds.season_years();
    let mut clubs: Vec<String> = Vec::new();
    for y in &years {
        let c = ds.season(*y).club;
        if clubs.last() != Some(&c) {
            clubs.push(c);
        }
    }
    let all = Record::of(played.iter().copied());
    // prijzen per competitie, meest gewonnen eerst
    let mut won: Vec<(String, Vec<String>)> = Vec::new();
    for tr in ds.trophies() {
        let (name, season) = (strip(&tr.comp), ds.season_label(tr.season));
        match won.iter_mut().find(|(n, _)| *n == name) {
            Some((_, v)) => v.push(season),
            None => won.push((name, vec![season])),
        }
    }
    won.sort_by_key(|(_, v)| std::cmp::Reverse(v.len()));
    let total: usize = won.iter().map(|(_, v)| v.len()).sum();
    let span = match (years.first(), years.last()) {
        (Some(a), Some(b)) if a != b => format!("{} – {}", ds.season_label(*a), ds.season_label(*b)),
        (Some(a), _) => ds.season_label(*a),
        _ => String::new(),
    };
    CardData {
        kicker: format!("{} · {span}", t!("CARRIÈRE", "CAREER")),
        title: if clubs.len() == 1 { clubs[0].clone() } else { clubs.join(" → ") },
        subtitle: format!("{} {} · {} {}", years.len(), t!("seizoenen", "seasons"), total, if total == 1 { t!("prijs", "trophy") } else { t!("prijzen", "trophies") }),
        gold: total > 0,
        stats: [
            (all.p.to_string(), t!("wedstrijden", "matches").into()),
            (format!("{}–{}–{}", all.w, all.d, all.l), t!("w–g–v", "w–d–l").into()),
            (format!("{}:{}", all.gf, all.ga), t!("doelpunten", "goals").into()),
            (format!("{:.0}%", all.win_pct()), t!("gewonnen", "win rate").into()),
        ],
        trophies: won.into_iter().map(|(n, v)| (if v.len() > 1 { format!("{}× {n}", v.len()) } else { n }, v.join(", "))).collect(),
        lines: highlight_lines(played),
        strip_title: t!("DE CARRIÈRE, DUEL VOOR DUEL", "THE CAREER, MATCH BY MATCH").into(),
        played,
        footer: format!("{:.2} {}", all.ppg(), t!("punten per duel", "points per game")),
    }
}

/// De deelkaart: alles geschaald met `k` (1 = 560×700).
fn draw_card(p: &egui::Painter, rect: Rect, k: f32, c: &CardData) {
    let f = |size: f32, fam: egui::FontFamily| FontId::new(size * k, fam);
    let soft = Color32::from_rgb(160, 166, 186);
    let light = Color32::from_rgb(205, 210, 226);
    let accent = if c.gold { GOLD } else { ACCENT_HI };
    let radius = CornerRadius::same((28.0 * k) as u8);

    p.rect_filled(rect, radius, Color32::from_rgb(13, 14, 24));
    w::glow(p, rect.left_top() + vec2(60.0, 40.0) * k, 380.0 * k, ACCENT, 0.85);
    w::glow(p, rect.right_bottom() - vec2(40.0, 120.0) * k, 320.0 * k, if c.gold { GOLD } else { Color32::from_rgb(255, 110, 180) }, 0.5);
    // de gloed loopt door tot in de hoeken: die weer dichtzetten met de paginakleur
    p.rect_stroke(rect, radius, Stroke::new(40.0 * k, theme::BG), egui::StrokeKind::Outside);
    p.rect_stroke(rect, radius, Stroke::new(1.5, Color32::from_rgba_unmultiplied(255, 255, 255, 28)), egui::StrokeKind::Inside);

    let x = rect.left() + 40.0 * k;
    let right = rect.right() - 40.0 * k;
    let mut y = rect.top() + 40.0 * k;
    p.text(pos2(x, y), Align2::LEFT_TOP, &c.kicker, f(13.0, theme::semibold()), accent);
    y += 24.0 * k;
    // een lange titel (twee clubs) kleiner, zodat hij op de kaart past
    let size = if c.title.chars().count() > 20 { 28.0 } else { 40.0 };
    p.text(pos2(x, y + (40.0 - size) * 0.5 * k), Align2::LEFT_TOP, &c.title, f(size, theme::bold()), Color32::WHITE);
    y += 56.0 * k;
    p.text(pos2(x, y), Align2::LEFT_TOP, &c.subtitle, f(17.0, theme::medium()), if c.gold { GOLD } else { light });
    y += 46.0 * k;

    let cw = (right - x + 10.0 * k) / 4.0;
    for (i, (v, l)) in c.stats.iter().enumerate() {
        let r = Rect::from_min_size(pos2(x + cw * i as f32, y), vec2(cw - 10.0 * k, 86.0 * k));
        p.rect_filled(r, CornerRadius::same((16.0 * k) as u8), Color32::from_rgba_unmultiplied(255, 255, 255, 14));
        p.text(pos2(r.center().x, r.top() + 18.0 * k), Align2::CENTER_TOP, v, f(if v.chars().count() > 7 { 17.0 } else if v.chars().count() > 6 { 20.0 } else { 26.0 }, theme::bold()), Color32::WHITE);
        p.text(pos2(r.center().x, r.bottom() - 14.0 * k), Align2::CENTER_BOTTOM, l.to_uppercase(), f(10.0, theme::semibold()), soft);
    }
    y += 110.0 * k;

    if !c.trophies.is_empty() {
        p.text(pos2(x, y), Align2::LEFT_TOP, t!("PRIJZEN", "TROPHIES"), f(11.0, theme::semibold()), soft);
        y += 22.0 * k;
        for (name, detail) in c.trophies.iter().take(5) {
            p.text(pos2(x, y + 11.0 * k), Align2::LEFT_CENTER, icon::TROPHY, f(20.0, theme::icons()), GOLD);
            p.text(pos2(x + 30.0 * k, y + 11.0 * k), Align2::LEFT_CENTER, name, f(16.0, theme::semibold()), Color32::WHITE);
            if !detail.is_empty() {
                p.text(pos2(right, y + 11.0 * k), Align2::RIGHT_CENTER, shorten(detail, 30), f(12.5, egui::FontFamily::Proportional), light);
            }
            y += 28.0 * k;
        }
        y += 14.0 * k;
    }

    p.text(pos2(x, y), Align2::LEFT_TOP, t!("HOOGTEPUNTEN", "HIGHLIGHTS"), f(11.0, theme::semibold()), soft);
    y += 22.0 * k;
    // met vijf prijzen is er plaats voor drie regels
    let room = ((rect.bottom() - 124.0 * k - y) / (25.0 * k)).floor().max(1.0) as usize;
    for (a, b) in c.lines.iter().take(room.min(4)) {
        p.text(pos2(x, y), Align2::LEFT_TOP, a, f(14.0, egui::FontFamily::Proportional), light);
        p.text(pos2(right, y), Align2::RIGHT_TOP, b, f(14.0, theme::semibold()), Color32::WHITE);
        y += 25.0 * k;
    }

    let strip_y = rect.bottom() - 92.0 * k;
    p.text(pos2(x, strip_y - 22.0 * k), Align2::LEFT_TOP, &c.strip_title, f(11.0, theme::semibold()), soft);
    let n = c.played.len().max(1) as f32;
    let step = ((right - x) / n).min(14.0 * k);
    let gap = if step > 5.0 * k { 2.0 * k } else { step * 0.25 };
    for (i, m) in c.played.iter().enumerate() {
        let r = Rect::from_min_size(pos2(x + step * i as f32, strip_y), vec2((step - gap).max(1.0), 22.0 * k));
        p.rect_filled(r, CornerRadius::same((3.0 * k).min(r.width() / 2.0) as u8), m.result().map(w::res_color).unwrap_or(TEXT_FAINT));
    }
    p.text(pos2(x, rect.bottom() - 34.0 * k), Align2::LEFT_CENTER, "FM27 Chronicle", f(13.0, theme::bold()), light);
    p.text(pos2(right, rect.bottom() - 34.0 * k), Align2::RIGHT_CENTER, &c.footer, f(12.0, egui::FontFamily::Proportional), soft);
}
