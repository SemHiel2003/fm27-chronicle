//! Het model: wedstrijden uit FM's geheugen en alles wat de app daaruit afleidt (seizoenen, standen,
//! reeksen, records, prijzen). Het lezen staat onderaan (`load`); de rest is pure rekenlogica met tests.
//!
//! Gemeten op macOS arm64, Steam-build 26620374 (17 nov 2026, Ajax-save in april 2029), alleen lezen:
//! `sicomps::FIXTURE_RESULT` is een record van 0x80 bytes (vtable op +0, in reeksen achter elkaar):
//!   +0x08 thuisteam, +0x10 uitteam (db::TEAM), +0x20 db::FIXTURE_NAME, +0x30 u16 seizoen (0 = oefenduel),
//!   +0x38 u8 fase, +0x39 u8 ronde-index (0xFF = competitievorm), +0x3B u8 speelronde,
//!   +0x4C u16 dag van het jaar (lage 9 bits) + u16 jaar, +0x5C u32 toeschouwers,
//!   +0x64 thuisdoelpunten (0xFF = nog niet gespeeld), +0x67 totaal over twee duels,
//!   +0x69 uitdoelpunten, +0x6C totaal over twee duels.
//!   +0x65/+0x66 en +0x6A/+0x6B: vermoedelijk na verlenging en strafschoppen (niet bevestigd).
//! `db::FIXTURE_NAME` (0x48 bytes): +0x18 db::COMP, +0x20 pad/fase, +0x28 ronde, +0x30 heen/terug
//! (db::STAGE_NAME: +0x18 naam, +0x20 kort). db::COMP: +0x40 naam, +0x48 kort.
//! FM bewaart alle wedstrijden sinds het begin van de save, met tijdelijke kopieën ertussen.

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Match {
    pub date: NaiveDate,
    /// Startjaar van het seizoen volgens FM; oefenduels krijgen het seizoen van de wedstrijden eromheen.
    pub season: u16,
    pub comp: String,
    pub comp_short: String,
    /// Ronde ("Qtr Final") en duel ("Leg 1"); leeg in een competitie.
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub leg: String,
    pub home: String,
    pub away: String,
    pub hg: Option<u8>,
    pub ag: Option<u8>,
    #[serde(default)]
    pub pens: Option<(u8, u8)>,
    #[serde(default)]
    pub agg: Option<(u8, u8)>,
    /// Competitievorm (iedereen tegen iedereen) of knock-out.
    pub league: bool,
    /// Ronde-index binnen een knock-outtoernooi (hoger = verder).
    #[serde(default)]
    pub round: u8,
    #[serde(default)]
    pub matchday: u8,
    #[serde(default)]
    pub friendly: bool,
    /// Laatste ronde van het toernooi (finale of supercup).
    #[serde(default)]
    pub is_final: bool,
    #[serde(default)]
    pub attendance: u32,
    /// Speelt het eigen team thuis?
    pub at_home: bool,
    /// De club waarvan de manager toen trainer was (korte naam); een carrière kan meer clubs tellen.
    #[serde(default)]
    pub team: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Res {
    W,
    D,
    L,
}

impl Match {
    pub fn played(&self) -> bool {
        self.hg.is_some() && self.ag.is_some()
    }
    pub fn opponent(&self) -> &str {
        if self.at_home { &self.away } else { &self.home }
    }
    /// (voor, tegen) vanuit het eigen team.
    pub fn goals(&self) -> Option<(u8, u8)> {
        let (h, a) = (self.hg?, self.ag?);
        Some(if self.at_home { (h, a) } else { (a, h) })
    }
    pub fn result(&self) -> Option<Res> {
        let (f, a) = self.goals()?;
        Some(if f > a { Res::W } else if f < a { Res::L } else { Res::D })
    }
    pub fn score(&self) -> String {
        match (self.hg, self.ag) {
            (Some(h), Some(a)) => format!("{h}–{a}"),
            _ => "–".into(),
        }
    }
    /// Wie een knock-outduel won: Some(true) = wij. Bij gelijkspel tellen strafschoppen of het totaal.
    pub fn tie_won(&self) -> Option<bool> {
        let (f, a) = self.goals()?;
        if let Some((h, w)) = self.agg {
            let (tf, ta) = if self.at_home { (h, w) } else { (w, h) };
            if tf != ta {
                return Some(tf > ta);
            }
        } else if f != a {
            return Some(f > a);
        }
        let (ph, pa) = self.pens?;
        let (pf, pt) = if self.at_home { (ph, pa) } else { (pa, ph) };
        (pf != pt).then_some(pf > pt)
    }
}

/// Een wedstrijd uit een competitie (ook van andere clubs), voor de stand.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Game {
    pub date: NaiveDate,
    pub home: String,
    pub away: String,
    pub hg: u8,
    pub ag: u8,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct League {
    pub season: u16,
    pub comp: String,
    pub games: Vec<Game>,
    /// Aantal wedstrijden dat nog gespeeld moet worden.
    #[serde(default)]
    pub open: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Player {
    pub uid: u64,
    pub name: String,
    pub value: i64,
    pub wage: i64,
    pub morale: u8,
    /// Gemiddelde van de zichtbare attributen (1–20), 0 = onbekend.
    #[serde(default)]
    pub overall: f32,
}

/// Momentopname van club en selectie op een speldatum.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    /// UID van de club waar de manager toen werkte.
    #[serde(default)]
    pub club: u32,
    pub date: NaiveDate,
    pub balance: i64,
    pub transfer_budget: i64,
    pub wage_budget: i64,
    pub players: Vec<Player>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Dataset {
    pub club: String,
    pub club_short: String,
    pub club_uid: u32,
    pub today: Option<NaiveDate>,
    /// UID van de manager (herkent de carrière, ook na een overstap naar een andere club).
    #[serde(default)]
    pub manager_uid: u32,
    /// Sinds wanneer de manager bij de huidige club werkt: oudere wedstrijden van die club zijn niet van hem.
    #[serde(default)]
    pub joined: Option<NaiveDate>,
    pub matches: Vec<Match>,
    pub leagues: Vec<League>,
    #[serde(default)]
    pub snapshots: Vec<Snapshot>,
}

// ---- rekenen ---------------------------------------------------------------------------------

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Record {
    pub p: u32,
    pub w: u32,
    pub d: u32,
    pub l: u32,
    pub gf: u32,
    pub ga: u32,
}

impl Record {
    pub fn add(&mut self, f: u8, a: u8) {
        self.p += 1;
        self.gf += f as u32;
        self.ga += a as u32;
        if f > a {
            self.w += 1
        } else if f < a {
            self.l += 1
        } else {
            self.d += 1
        }
    }
    pub fn points(&self) -> u32 {
        self.w * 3 + self.d
    }
    pub fn gd(&self) -> i32 {
        self.gf as i32 - self.ga as i32
    }
    pub fn win_pct(&self) -> f32 {
        if self.p == 0 { 0.0 } else { 100.0 * self.w as f32 / self.p as f32 }
    }
    pub fn ppg(&self) -> f32 {
        if self.p == 0 { 0.0 } else { self.points() as f32 / self.p as f32 }
    }
    pub fn of<'a>(ms: impl IntoIterator<Item = &'a Match>) -> Record {
        let mut r = Record::default();
        for m in ms {
            if let Some((f, a)) = m.goals() {
                r.add(f, a);
            }
        }
        r
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub team: String,
    pub rec: Record,
}

/// Stand uit losse uitslagen: punten, doelsaldo, doelpunten voor, naam.
pub fn table(games: &[Game], until: Option<NaiveDate>) -> Vec<Row> {
    let mut map: BTreeMap<&str, Record> = BTreeMap::new();
    for g in games {
        map.entry(&g.home).or_default();
        map.entry(&g.away).or_default();
        if until.is_some_and(|u| g.date > u) {
            continue;
        }
        map.get_mut(g.home.as_str()).unwrap().add(g.hg, g.ag);
        map.get_mut(g.away.as_str()).unwrap().add(g.ag, g.hg);
    }
    let mut rows: Vec<Row> = map.into_iter().map(|(t, rec)| Row { team: t.to_string(), rec }).collect();
    rows.sort_by(|a, b| {
        (b.rec.points(), b.rec.gd(), b.rec.gf).cmp(&(a.rec.points(), a.rec.gd(), a.rec.gf)).then_with(|| a.team.cmp(&b.team))
    });
    rows
}

pub fn position(rows: &[Row], team: &str) -> Option<usize> {
    rows.iter().position(|r| r.team == team).map(|i| i + 1)
}

/// Langste reeks opeenvolgende wedstrijden die aan `ok` voldoen: (lengte, eerste, laatste).
pub fn longest<'a>(ms: &[&'a Match], ok: impl Fn(&Match) -> bool) -> Option<(usize, &'a Match, &'a Match)> {
    let mut best: Option<(usize, usize)> = None; // (lengte, einde)
    let mut run = 0;
    for (i, m) in ms.iter().enumerate() {
        if ok(m) {
            run += 1;
            if best.is_none_or(|(n, _)| run > n) {
                best = Some((run, i));
            }
        } else {
            run = 0;
        }
    }
    best.map(|(n, end)| (n, ms[end + 1 - n], ms[end]))
}

/// Huidige reeks aan het eind van de lijst.
pub fn current(ms: &[&Match], ok: impl Fn(&Match) -> bool) -> usize {
    ms.iter().rev().take_while(|m| ok(m)).count()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Trophy {
    pub season: u16,
    pub comp: String,
    pub date: NaiveDate,
    /// Tegenstander en uitslag van de finale; leeg bij een landstitel.
    pub detail: String,
}

/// Hoe ver het team kwam in een knock-outtoernooi.
#[derive(Clone, Debug, PartialEq)]
pub struct CupRun {
    pub comp: String,
    pub stage: String,
    /// Some(true) gewonnen, Some(false) finale verloren, None: eerder uitgeschakeld of nog bezig.
    pub won_final: Option<bool>,
    pub rec: Record,
}

pub struct Season<'a> {
    pub year: u16,
    /// De club van de manager in dit seizoen (korte naam; bij een overstap midden in het seizoen de laatste).
    pub team: String,
    /// Dezelfde club, voluit als dat de huidige is.
    pub club: String,
    pub matches: Vec<&'a Match>,
    pub league: Option<&'a League>,
}

impl Dataset {
    /// De momentopnames van de huidige club: na een overstap begint de vergelijking van selectie en geld opnieuw.
    pub fn club_snapshots(&self) -> &[Snapshot] {
        let start = self.snapshots.iter().rposition(|s| s.club != self.club_uid).map(|i| i + 1).unwrap_or(0);
        &self.snapshots[start..]
    }

    pub fn season_years(&self) -> Vec<u16> {
        let mut v: Vec<u16> = self.matches.iter().filter(|m| !m.friendly).map(|m| m.season).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// "2028/29", of "2028" voor een competitie binnen één kalenderjaar.
    pub fn season_label(&self, year: u16) -> String {
        let spans = self.matches.iter().filter(|m| m.season == year && !m.friendly).any(|m| m.date.year() as u16 != year);
        if spans { format!("{}/{:02}", year, (year + 1) % 100) } else { year.to_string() }
    }

    pub fn season(&self, year: u16) -> Season<'_> {
        let matches: Vec<&Match> = self.matches.iter().filter(|m| m.season == year).collect();
        // de eigen competitie: die met de meeste wedstrijden van het team
        let mut count: HashMap<&str, usize> = HashMap::new();
        for m in matches.iter().filter(|m| m.league && !m.friendly && m.stage.is_empty()) {
            *count.entry(m.comp.as_str()).or_default() += 1;
        }
        let main = count.into_iter().max_by_key(|(_, n)| *n).map(|(c, _)| c.to_string());
        let league = main.and_then(|c| self.leagues.iter().find(|l| l.season == year && l.comp == c));
        let team = matches.iter().rev().find(|m| !m.friendly && !m.team.is_empty()).map(|m| m.team.clone()).unwrap_or_else(|| self.club_short.clone());
        let club = if team == self.club_short { self.club.clone() } else { team.clone() };
        Season { year, team, club, matches, league }
    }

    /// Alle gespeelde wedstrijden (zonder oefenduels), op datum.
    pub fn competitive(&self) -> Vec<&Match> {
        self.matches.iter().filter(|m| m.played() && !m.friendly).collect()
    }

    pub fn trophies(&self) -> Vec<Trophy> {
        let mut out = Vec::new();
        for y in self.season_years() {
            let s = self.season(y);
            if let Some(l) = s.league {
                if l.open == 0 && !l.games.is_empty() && position(&table(&l.games, None), &s.team) == Some(1) {
                    let date = l.games.iter().map(|g| g.date).max().unwrap();
                    out.push(Trophy { season: y, comp: l.comp.clone(), date, detail: String::new() });
                }
            }
            for m in s.matches.iter().filter(|m| m.is_final && m.played()) {
                if m.tie_won() == Some(true) {
                    out.push(Trophy { season: y, comp: m.comp.clone(), date: m.date, detail: format!("{} {} {}", m.home, m.score(), m.away) });
                }
            }
        }
        out.sort_by_key(|t| t.date);
        out
    }
}

impl Season<'_> {
    pub fn played(&self) -> Vec<&Match> {
        self.matches.iter().copied().filter(|m| m.played() && !m.friendly).collect()
    }
    pub fn upcoming(&self) -> Vec<&Match> {
        self.matches.iter().copied().filter(|m| !m.played()).collect()
    }
    pub fn league_matches(&self) -> Vec<&Match> {
        let Some(l) = self.league else { return Vec::new() };
        self.matches.iter().copied().filter(|m| m.played() && m.league && m.stage.is_empty() && m.comp == l.comp).collect()
    }
    /// Per competitie het record, de eigen competitie eerst.
    pub fn by_comp(&self) -> Vec<(String, Record)> {
        let mut map: Vec<(String, Record)> = Vec::new();
        for m in self.played() {
            let (f, a) = m.goals().unwrap();
            match map.iter_mut().find(|(c, _)| *c == m.comp) {
                Some((_, r)) => r.add(f, a),
                None => {
                    let mut r = Record::default();
                    r.add(f, a);
                    map.push((m.comp.clone(), r));
                }
            }
        }
        map.sort_by_key(|(_, r)| std::cmp::Reverse(r.p));
        map
    }
    /// Positie op de ranglijst na elke eigen competitiewedstrijd.
    pub fn positions(&self) -> Vec<(NaiveDate, usize, u32)> {
        let club = self.team.as_str();
        let Some(l) = self.league else { return Vec::new() };
        self.league_matches()
            .iter()
            .filter_map(|m| {
                let rows = table(&l.games, Some(m.date));
                let pos = position(&rows, club)?;
                Some((m.date, pos, rows[pos - 1].rec.points()))
            })
            .collect()
    }
    pub fn cup_runs(&self) -> Vec<CupRun> {
        let mut out: Vec<CupRun> = Vec::new();
        for m in self.matches.iter().filter(|m| !m.friendly && (!m.league || !m.stage.is_empty())) {
            if !m.played() {
                continue;
            }
            let (f, a) = m.goals().unwrap();
            let stage = if m.stage.is_empty() { m.comp_short.clone() } else { m.stage.clone() };
            let won = if m.is_final { m.tie_won() } else { None };
            match out.iter_mut().find(|c| c.comp == m.comp) {
                Some(c) => {
                    c.rec.add(f, a);
                    c.stage = stage;
                    c.won_final = won;
                }
                None => {
                    let mut rec = Record::default();
                    rec.add(f, a);
                    out.push(CupRun { comp: m.comp.clone(), stage, won_final: won, rec });
                }
            }
        }
        out
    }
}

/// Onderlinge resultaten per tegenstander, meeste duels eerst.
pub fn head_to_head(ms: &[&Match]) -> Vec<(String, Record)> {
    let mut map: HashMap<&str, Record> = HashMap::new();
    for m in ms {
        if let Some((f, a)) = m.goals() {
            map.entry(m.opponent()).or_default().add(f, a);
        }
    }
    let mut v: Vec<(String, Record)> = map.into_iter().map(|(k, r)| (k.to_string(), r)).collect();
    v.sort_by(|a, b| b.1.p.cmp(&a.1.p).then_with(|| a.0.cmp(&b.0)));
    v
}

pub fn money(v: i64) -> String {
    let (a, sign) = (v.unsigned_abs() as f64, if v < 0 { "-" } else { "" });
    if a >= 1e9 {
        format!("{sign}£{:.2}B", a / 1e9)
    } else if a >= 1e6 {
        format!("{sign}£{:.1}M", a / 1e6)
    } else if a >= 1e3 {
        format!("{sign}£{:.0}K", a / 1e3)
    } else {
        format!("{sign}£{a:.0}")
    }
}

// ---- lezen uit FM ----------------------------------------------------------------------------

pub mod load {
    use super::*;
    use crate::fm::{self, is_ptr, u16_at, u32_at, u64_at};
    use anyhow::{Result, anyhow};
    use serde_json::json;

    const REC: usize = 0x80;

    struct Names {
        teams: HashMap<u64, Option<(String, String, u32)>>,
        fixture: HashMap<u64, FixName>,
        strings: HashMap<u64, Option<String>>,
    }

    #[derive(Clone, Default)]
    struct FixName {
        comp_ptr: u64,
        comp: String,
        comp_short: String,
        stage: String,
        leg: String,
    }

    impl Names {
        fn string(&mut self, p: u64) -> Option<String> {
            if !is_ptr(p) && p < 0x10000 {
                return None;
            }
            self.strings.entry(p).or_insert_with(|| fm::fm_string(p)).clone()
        }
        /// (naam, korte naam, club-UID) van een team; None voor nationale elftallen en rommel.
        fn team(&mut self, t: u64) -> Option<(String, String, u32)> {
            if let Some(v) = self.teams.get(&t) {
                return v.clone();
            }
            let v = (|| {
                let club = u64_at(&fm::read(t + 0x30, 8).ok()?, 0);
                if !is_ptr(club) {
                    return None;
                }
                let c = fm::read(club, 0xD0).ok()?;
                let name = self.string(u64_at(&c, 0xC0))?;
                let short = self.string(u64_at(&c, 0xC8)).unwrap_or_else(|| name.clone());
                Some((name, short, u32_at(&c, 0x0C)))
            })();
            self.teams.insert(t, v.clone());
            v
        }
        fn stage(&mut self, p: u64, short: bool) -> String {
            if !is_ptr(p) {
                return String::new();
            }
            let Ok(b) = fm::read(p, 0x30) else { return String::new() };
            self.string(u64_at(&b, if short { 0x20 } else { 0x18 })).unwrap_or_default()
        }
    }

    fn fm_date(b: &[u8], off: usize) -> Option<NaiveDate> {
        let (doy, year) = (u16_at(b, off) & 0x1FF, u16_at(b, off + 2));
        if !(1990..=2200).contains(&year) {
            return None;
        }
        NaiveDate::from_yo_opt(year as i32, doy as u32)
    }

    fn goal(v: u8) -> Option<u8> {
        (v <= 60).then_some(v)
    }

    /// Leest alles wat de app nodig heeft. `progress` krijgt korte statusregels.
    pub fn dataset(progress: &dyn Fn(&str)) -> Result<Dataset> {
        progress(crate::t!("Club en datum lezen…", "Reading club and date…"));
        let chain = fm::request(json!({"cmd": "chain"}))?;
        let team = chain.get("team").and_then(fm::parse_addr).ok_or_else(|| anyhow!("no team"))?;
        let club = fm::request(json!({"cmd": "club"}))?;
        let mut names = Names { teams: HashMap::new(), fixture: HashMap::new(), strings: HashMap::new() };
        let (club_name, club_short, club_uid) = names.team(team).ok_or_else(|| anyhow!("club name not readable"))?;
        // de carrière van de manager: zijn UID en de dag waarop hij bij deze club begon (contract, op beide platforms gelijk)
        let (mut manager_uid, mut joined) = (0u32, None);
        if let Some(contract) = chain.get("contract").and_then(fm::parse_addr) {
            if let Ok(c) = fm::read(contract, 0x50) {
                joined = fm_date(&c, 0x4C);
                let owner = u64_at(&c, 0x08);
                if is_ptr(owner) {
                    manager_uid = fm::read(owner, 0x10).map(|o| u32_at(&o, 0x0C)).unwrap_or(0);
                }
            }
        }
        let today = fm::request(json!({"cmd": "game_date"})).ok().and_then(|d| NaiveDate::from_yo_opt(d.get("year")?.as_i64()? as i32, d.get("doy")?.as_u64()? as u32));

        progress(crate::t!("Wedstrijden zoeken…", "Finding matches…"));
        let vts = fm::vtables("sicomps::FIXTURE_RESULT")?;
        if vts.is_empty() {
            return Err(anyhow!("{}", crate::t!("Wedstrijden niet gevonden in deze FM-versie.", "Matches not found in this FM version.")));
        }
        let mut addrs = Vec::new();
        for vt in &vts {
            addrs.extend(fm::find_u64(*vt, 1_000_000)?);
        }
        progress(&crate::t!(format!("{} wedstrijdrecords lezen…", addrs.len()), format!("Reading {} match records…", addrs.len())));
        let recs = fm::read_records(addrs, REC);

        // geldige records, ontdubbeld op (datum, thuis, uit): FM houdt tijdelijke kopieën bij
        struct Raw {
            date: NaiveDate,
            home: u64,
            away: u64,
            b: Vec<u8>,
        }
        let mut uniq: HashMap<(NaiveDate, u64, u64), Vec<Raw>> = HashMap::new();
        for (_, b) in recs {
            let (home, away, fx) = (u64_at(&b, 0x08), u64_at(&b, 0x10), u64_at(&b, 0x20));
            if !is_ptr(home) || !is_ptr(away) || !is_ptr(fx) || home == away {
                continue;
            }
            let Some(date) = fm_date(&b, 0x4C) else { continue };
            uniq.entry((date, home, away)).or_default().push(Raw { date, home, away, b });
        }
        // per wedstrijd de meest voorkomende uitslag onder de kopieën (een lopende wedstrijd heeft er meerdere)
        let mut raws: Vec<Raw> = Vec::new();
        for (_, mut copies) in uniq {
            let mut votes: HashMap<(u8, u8), usize> = HashMap::new();
            for c in &copies {
                if c.b[0x64] != 0xFF && c.b[0x69] != 0xFF {
                    *votes.entry((c.b[0x64], c.b[0x69])).or_default() += 1;
                }
            }
            let pick = votes.into_iter().max_by_key(|(s, n)| (*n, s.0 as usize + s.1 as usize)).map(|(s, _)| s);
            let i = pick.and_then(|s| copies.iter().position(|c| (c.b[0x64], c.b[0x69]) == s)).unwrap_or(0);
            raws.push(copies.swap_remove(i));
        }
        raws.sort_by_key(|r| r.date);

        // namen van de competities: alle FIXTURE_NAME-records in één keer
        progress(crate::t!("Competities lezen…", "Reading competitions…"));
        // wedstrijden van vóór zijn komst horen bij de club, niet bij de manager
        let mine: Vec<&Raw> = raws.iter().filter(|r| (r.home == team || r.away == team) && joined.is_none_or(|j| r.date >= j)).collect();
        let fx_ptrs: Vec<u64> = mine.iter().map(|r| u64_at(&r.b, 0x20)).collect();
        for (p, f) in fm::read_records(fx_ptrs, 0x48) {
            let comp_ptr = u64_at(&f, 0x18);
            let mut n = FixName { comp_ptr, ..Default::default() };
            if is_ptr(comp_ptr) {
                if let Ok(c) = fm::read(comp_ptr, 0x50) {
                    n.comp = names.string(u64_at(&c, 0x40)).unwrap_or_default();
                    n.comp_short = names.string(u64_at(&c, 0x48)).unwrap_or_else(|| n.comp.clone());
                }
            }
            let (phase, round) = (u64_at(&f, 0x20), u64_at(&f, 0x28));
            n.stage = names.stage(if is_ptr(round) { round } else { phase }, true);
            n.leg = names.stage(u64_at(&f, 0x30), true);
            names.fixture.insert(p, n);
        }

        // de eigen wedstrijden
        let mut matches: Vec<Match> = Vec::new();
        let mut main_fx: HashMap<(u16, u64), usize> = HashMap::new(); // (seizoen, FIXTURE_NAME) van competitieduels
        let mut meta: Vec<(u64, u16)> = Vec::new(); // per wedstrijd: FIXTURE_NAME en (fase, ronde)
        for r in &mine {
            let b = &r.b;
            let fx = u64_at(b, 0x20);
            let Some(n) = names.fixture.get(&fx).cloned() else { continue };
            let (Some(h), Some(a)) = (names.team(r.home), names.team(r.away)) else { continue };
            let season = u16_at(b, 0x30);
            let friendly = season == 0;
            let league = b[0x39] == 0xFF && !friendly;
            if league && n.stage.is_empty() {
                *main_fx.entry((season, fx)).or_default() += 1;
            }
            let (hg, ag) = (goal(b[0x64]), goal(b[0x69]));
            let played = hg.is_some() && ag.is_some();
            meta.push((fx, ((b[0x38] as u16) << 8) | b[0x39] as u16));
            matches.push(Match {
                date: r.date,
                season,
                comp: n.comp.clone(),
                comp_short: n.comp_short.clone(),
                stage: n.stage.clone(),
                leg: n.leg.clone(),
                home: h.1,
                away: a.1,
                hg: if played { hg } else { None },
                ag: if played { ag } else { None },
                pens: match (goal(b[0x66]), goal(b[0x6B])) {
                    (Some(x), Some(y)) if played => Some((x, y)),
                    _ => None,
                },
                agg: match (goal(b[0x67]), goal(b[0x6C])) {
                    (Some(x), Some(y)) if played => Some((x, y)),
                    _ => None,
                },
                league,
                round: if league { 0 } else { b[0x39] },
                matchday: b[0x3B],
                friendly,
                is_final: false,
                attendance: u32_at(b, 0x5C),
                at_home: r.home == team,
                team: club_short.clone(),
            });
        }
        // finale: de hoogste (fase, ronde) van dat toernooi in dat seizoen over alle clubs, met precies één duel
        let mut top: HashMap<(u16, u64), (u16, usize)> = HashMap::new();
        let mut comp_of: HashMap<u64, u64> = HashMap::new();
        let comps_mine: std::collections::HashSet<u64> = names.fixture.values().map(|n| n.comp_ptr).collect();
        let all_fx: Vec<u64> = raws.iter().map(|r| u64_at(&r.b, 0x20)).collect();
        for (p, f) in fm::read_records(all_fx, 0x48) {
            comp_of.insert(p, u64_at(&f, 0x18));
        }
        let key = |b: &[u8]| ((b[0x38] as u16) << 8) | b[0x39] as u16;
        for r in &raws {
            let season = u16_at(&r.b, 0x30);
            let Some(&c) = comp_of.get(&u64_at(&r.b, 0x20)) else { continue };
            if season == 0 || r.b[0x39] == 0xFF || !comps_mine.contains(&c) {
                continue;
            }
            let (k, e) = (key(&r.b), top.entry((season, c)).or_insert((0, 0)));
            if k > e.0 {
                *e = (k, 1);
            } else if k == e.0 {
                e.1 += 1;
            }
        }
        for (m, (fx, k)) in matches.iter_mut().zip(meta.iter()) {
            if m.league || m.friendly {
                continue;
            }
            let c = comp_of.get(fx).copied().unwrap_or(0);
            m.is_final = top.get(&(m.season, c)) == Some(&(*k, 1));
        }

        // oefenduels: het seizoen van de eerstvolgende officiële wedstrijd, anders de vorige
        let dated: Vec<(NaiveDate, u16)> = matches.iter().filter(|m| !m.friendly).map(|m| (m.date, m.season)).collect();
        for m in matches.iter_mut().filter(|m| m.friendly) {
            m.season = dated.iter().find(|(d, _)| *d >= m.date).or(dated.last()).map(|(_, s)| *s).unwrap_or(m.date.year() as u16);
        }

        // standen: alle wedstrijden met dezelfde FIXTURE_NAME en hetzelfde seizoen als de eigen competitie
        progress(crate::t!("Standen opbouwen…", "Building league tables…"));
        let mut leagues: Vec<League> = Vec::new();
        let mut seasons: Vec<u16> = main_fx.keys().map(|k| k.0).collect();
        seasons.sort_unstable();
        seasons.dedup();
        for s in seasons {
            let Some((&(_, fx), _)) = main_fx.iter().filter(|(k, _)| k.0 == s).max_by_key(|(_, n)| **n) else { continue };
            let comp = names.fixture.get(&fx).map(|n| n.comp.clone()).unwrap_or_default();
            let mut l = League { season: s, comp, games: Vec::new(), open: 0 };
            for r in raws.iter().filter(|r| u64_at(&r.b, 0x20) == fx && u16_at(&r.b, 0x30) == s) {
                let (Some(h), Some(a)) = (names.team(r.home), names.team(r.away)) else { continue };
                match (goal(r.b[0x64]), goal(r.b[0x69])) {
                    (Some(hg), Some(ag)) if today.is_none_or(|t| r.date < t) => l.games.push(Game { date: r.date, home: h.1, away: a.1, hg, ag }),
                    _ => l.open += 1,
                }
            }
            leagues.push(l);
        }
        // een wedstrijd van vandaag kan nog bezig zijn: niet als gespeeld tellen
        if let Some(t) = today {
            for m in matches.iter_mut().filter(|m| m.date >= t) {
                (m.hg, m.ag, m.pens, m.agg) = (None, None, None, None);
            }
        }

        progress(crate::t!("Selectie lezen…", "Reading squad…"));
        let snapshot = today.map(|date| Snapshot { club: club_uid, ..snapshot(date, &club) });
        Ok(Dataset { club: club_name, club_short, club_uid, today, manager_uid, joined, matches, leagues, snapshots: snapshot.into_iter().collect() })
    }

    fn snapshot(date: NaiveDate, club: &serde_json::Value) -> Snapshot {
        let n = |k: &str| club.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
        let mut overall: HashMap<u64, f32> = HashMap::new();
        if let Ok(t) = fm::request(json!({"cmd": "tactics"})) {
            for p in t.get("players").and_then(|p| p.as_array()).into_iter().flatten() {
                let vals: Vec<f32> = p.get("attributes").and_then(|a| a.as_object()).into_iter().flatten().filter_map(|(_, v)| v.as_f64()).map(|v| v as f32).collect();
                // de beste 20 attributen: een keeper wordt niet afgerekend op zijn veldspelerscijfers en andersom
                let mut top = vals.clone();
                top.sort_by(|a, b| b.total_cmp(a));
                top.truncate(20);
                if let (Some(uid), false) = (p.get("uid").and_then(|u| u.as_u64()), top.is_empty()) {
                    overall.insert(uid, top.iter().sum::<f32>() / top.len() as f32);
                }
            }
        }
        let mut players = Vec::new();
        if let Ok(sq) = fm::request(json!({"cmd": "squad"})) {
            let list = sq.as_array().cloned().or_else(|| sq.get("players").and_then(|p| p.as_array()).cloned()).unwrap_or_default();
            for p in list {
                let uid = p.get("uid").and_then(|u| u.as_u64()).unwrap_or(0);
                players.push(Player {
                    uid,
                    name: p.get("name").and_then(|s| s.as_str()).unwrap_or("?").to_string(),
                    value: p.get("value").and_then(|v| v.as_i64()).unwrap_or(0),
                    wage: p.get("wage").and_then(|v| v.as_i64()).unwrap_or(0),
                    morale: p.get("morale").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
                    overall: overall.get(&uid).copied().unwrap_or(0.0),
                });
            }
        }
        Snapshot { club: 0, date, balance: n("balance"), transfer_budget: n("transfer_budget"), wage_budget: n("wage_budget"), players }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn m(date: NaiveDate, home: &str, away: &str, hg: u8, ag: u8) -> Match {
        Match { date, season: 2028, comp: "Eredivisie".into(), comp_short: "Eredivisie".into(), home: home.into(), away: away.into(), hg: Some(hg), ag: Some(ag), league: true, at_home: home == "Ajax", team: "Ajax".into(), ..Default::default() }
    }

    #[test]
    fn table_orders_by_points_then_goal_difference() {
        let g = |h: &str, a: &str, hg, ag| Game { date: d(2028, 9, 1), home: h.into(), away: a.into(), hg, ag };
        let rows = table(&[g("Ajax", "PSV", 2, 0), g("Feyenoord", "AZ", 5, 0), g("PSV", "AZ", 1, 1)], None);
        assert_eq!(rows.iter().map(|r| r.team.as_str()).collect::<Vec<_>>(), ["Feyenoord", "Ajax", "PSV", "AZ"]);
        assert_eq!(position(&rows, "Ajax"), Some(2));
        assert_eq!(rows[2].rec, Record { p: 2, w: 0, d: 1, l: 1, gf: 1, ga: 3 });
    }

    #[test]
    fn table_until_a_date_keeps_every_team() {
        let g = |day, h: &str, a: &str| Game { date: d(2028, 9, day), home: h.into(), away: a.into(), hg: 1, ag: 0 };
        let rows = table(&[g(1, "Ajax", "PSV"), g(8, "AZ", "Ajax")], Some(d(2028, 9, 2)));
        assert_eq!(rows.len(), 3);
        assert_eq!(position(&rows, "Ajax"), Some(1));
        assert_eq!(rows.iter().find(|r| r.team == "AZ").unwrap().rec.p, 0);
    }

    #[test]
    fn streaks_and_results_from_our_side() {
        let ms = [m(d(2028, 8, 1), "Ajax", "PSV", 2, 0), m(d(2028, 8, 8), "AZ", "Ajax", 0, 3), m(d(2028, 8, 15), "Ajax", "NEC", 1, 1), m(d(2028, 8, 22), "FC Twente", "Ajax", 2, 1)];
        let refs: Vec<&Match> = ms.iter().collect();
        assert_eq!(ms[1].result(), Some(Res::W));
        assert_eq!(ms[3].goals(), Some((1, 2)));
        let (n, first, last) = longest(&refs, |m| m.result() == Some(Res::W)).unwrap();
        assert_eq!((n, first.date, last.date), (2, d(2028, 8, 1), d(2028, 8, 8)));
        assert_eq!(longest(&refs, |m| m.result() != Some(Res::L)).unwrap().0, 3);
        assert_eq!(current(&refs, |m| m.result() == Some(Res::L)), 1);
        assert_eq!(Record::of(refs.iter().copied()), Record { p: 4, w: 2, d: 1, l: 1, gf: 7, ga: 3 });
    }

    #[test]
    fn two_legged_tie_and_penalties_decide_the_winner() {
        let mut second = m(d(2029, 3, 7), "Ajax", "Parthenope", 3, 1);
        second.agg = Some((8, 4));
        assert_eq!(second.tie_won(), Some(true));
        let mut away = m(d(2029, 3, 7), "PSG", "Ajax", 1, 0);
        away.agg = Some((2, 3));
        assert_eq!(away.tie_won(), Some(true));
        let mut fin = m(d(2028, 5, 17), "Ajax", "Roma", 1, 1);
        assert_eq!(fin.tie_won(), None);
        fin.pens = Some((4, 5));
        assert_eq!(fin.tie_won(), Some(false));
    }

    #[test]
    fn trophies_come_from_a_finished_league_and_a_won_final() {
        let mut ds = Dataset { club: "AFC Ajax".into(), club_short: "Ajax".into(), ..Default::default() };
        ds.matches = vec![m(d(2028, 8, 1), "Ajax", "PSV", 2, 0), m(d(2029, 5, 1), "PSV", "Ajax", 0, 1)];
        let mut fin = m(d(2029, 4, 20), "Ajax", "AZ", 2, 1);
        (fin.comp, fin.league, fin.is_final, fin.stage) = ("Dutch Cup".into(), false, true, "Final".into());
        ds.matches.push(fin);
        ds.matches.sort_by_key(|m| m.date);
        let games = vec![Game { date: d(2028, 8, 1), home: "Ajax".into(), away: "PSV".into(), hg: 2, ag: 0 }, Game { date: d(2029, 5, 1), home: "PSV".into(), away: "Ajax".into(), hg: 0, ag: 1 }];
        ds.leagues = vec![League { season: 2028, comp: "Eredivisie".into(), games: games.clone(), open: 1 }];
        assert_eq!(ds.trophies().iter().map(|t| t.comp.as_str()).collect::<Vec<_>>(), ["Dutch Cup"]);
        ds.leagues[0].open = 0;
        assert_eq!(ds.trophies().iter().map(|t| t.comp.as_str()).collect::<Vec<_>>(), ["Dutch Cup", "Eredivisie"]);
        assert_eq!(ds.season_label(2028), "2028/29");
        let s = ds.season(2028);
        assert_eq!(s.positions().last().map(|p| (p.1, p.2)), Some((1, 6)));
        assert_eq!(s.cup_runs(), vec![CupRun { comp: "Dutch Cup".into(), stage: "Final".into(), won_final: Some(true), rec: Record { p: 1, w: 1, d: 0, l: 0, gf: 2, ga: 1 } }]);
    }

    #[test]
    fn money_is_short() {
        assert_eq!(money(40_768_373), "£40.8M");
        assert_eq!(money(-1_216_592), "-£1.2M");
        assert_eq!(money(74_547), "£75K");
    }
}
