//! Het archief per save: wedstrijden, standen en momentopnames op schijf, zodat de kroniek ook zonder
//! FM te bekijken is en de ontwikkeling van selectie en financiën over de tijd zichtbaar wordt.

use crate::data::{Dataset, Snapshot};
use std::path::PathBuf;

pub fn dir() -> PathBuf {
    crate::data_dir().join("saves")
}

/// Herkent een save aan de club en zijn eerste officiële wedstrijden (per save anders).
pub fn key(ds: &Dataset) -> String {
    // één archief per manager: zijn carrière loopt door als hij van club wisselt
    if ds.manager_uid != 0 {
        return format!("m{}", ds.manager_uid);
    }
    let mut h: u64 = 0xcbf29ce484222325;
    let mut eat = |s: &str| {
        for b in s.bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
    };
    for m in ds.matches.iter().filter(|m| !m.friendly && m.played()).take(6) {
        eat(&format!("{}{}{}{}", m.date, m.home, m.away, m.score()));
    }
    format!("{}-{:08x}", ds.club_uid, h as u32)
}

pub fn load(key: &str) -> Option<Dataset> {
    serde_json::from_slice(&std::fs::read(dir().join(format!("{key}.json"))).ok()?).ok()
}

pub fn save(key: &str, ds: &Dataset) -> std::io::Result<()> {
    std::fs::create_dir_all(dir())?;
    let tmp = dir().join(format!("{key}.json.tmp"));
    std::fs::write(&tmp, serde_json::to_vec(ds)?)?;
    std::fs::rename(tmp, dir().join(format!("{key}.json")))
}

/// Het archief voor deze uitlezing: dat van de manager, anders (eenmalig) het nieuwste archief van vóór de
/// archieven per manager dat niet verder is dan de speldatum en niet van de huidige club.
pub fn find(key: &str, new: &Dataset) -> Option<Dataset> {
    load(key).or_else(|| {
        let mut old: Vec<Dataset> = std::fs::read_dir(dir())
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json") && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('m')))
            .filter_map(|p| serde_json::from_slice::<Dataset>(&std::fs::read(p).ok()?).ok())
            .filter(|d| d.manager_uid == 0 && d.club_uid != new.club_uid && d.today.is_some() && d.today <= new.today)
            .collect();
        old.sort_by_key(|d| d.today);
        old.pop()
    })
}

/// Het laatst bijgewerkte archief (om bij het starten iets te tonen zonder FM).
pub fn latest() -> Option<Dataset> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    serde_json::from_slice(&std::fs::read(&files.first()?.1).ok()?).ok()
}

/// Voegt een verse uitlezing bij het archief. Wat FM nog weet wint; wat FM vergeten is blijft staan.
/// Een momentopname komt erbij als de vorige minstens `gap` dagen oud is; op dezelfde dag vervangt hij.
pub fn merge(old: Option<Dataset>, mut new: Dataset, gap: i64) -> Dataset {
    let Some(old) = old else { return new };
    for mut m in old.matches {
        if m.team.is_empty() {
            m.team = old.club_short.clone();
        }
        // wedstrijden van de huidige club van vóór de komst van de manager zijn niet van hem
        if m.team == new.club_short && new.joined.is_some_and(|j| m.date < j) {
            continue;
        }
        if !new.matches.iter().any(|n| n.date == m.date && n.home == m.home && n.away == m.away) && new.today.is_none_or(|t| m.date < t) {
            new.matches.push(m);
        }
    }
    new.matches.sort_by_key(|m| m.date);
    for l in old.leagues {
        if !new.leagues.iter().any(|n| n.season == l.season && n.comp == l.comp) {
            new.leagues.push(l);
        }
    }
    new.leagues.sort_by_key(|l| l.season);
    let fresh: Vec<Snapshot> = std::mem::take(&mut new.snapshots);
    new.snapshots = old.snapshots;
    for o in new.snapshots.iter_mut().filter(|o| o.club == 0) {
        o.club = old.club_uid;
    }
    for s in fresh {
        // terug in de tijd (een oudere save geladen): latere momentopnames horen niet meer bij dit verloop
        new.snapshots.retain(|o| o.date <= s.date);
        match new.snapshots.last_mut() {
            Some(last) if last.date == s.date => *last = s,
            Some(last) if last.club == s.club && (s.date - last.date).num_days() < gap => {}
            _ => new.snapshots.push(s),
        }
    }
    new
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Match;
    use chrono::NaiveDate;

    fn d(m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2028, m, day).unwrap()
    }
    fn game(date: NaiveDate, hg: u8) -> Match {
        Match { date, season: 2028, home: "Ajax".into(), away: "PSV".into(), hg: Some(hg), ag: Some(0), at_home: true, league: true, ..Default::default() }
    }
    fn snap(date: NaiveDate, balance: i64) -> Snapshot {
        Snapshot { date, balance, ..Default::default() }
    }

    #[test]
    fn merge_keeps_forgotten_matches_and_spaces_snapshots() {
        let old = Dataset { matches: vec![game(d(8, 1), 1), game(d(8, 8), 2)], snapshots: vec![snap(d(8, 1), 10)], ..Default::default() };
        let new = Dataset { today: Some(d(8, 20)), matches: vec![game(d(8, 8), 3), game(d(8, 15), 4)], snapshots: vec![snap(d(8, 4), 20)], ..Default::default() };
        let m = merge(Some(old.clone()), new.clone(), 7);
        assert_eq!(m.matches.iter().map(|x| x.hg.unwrap()).collect::<Vec<_>>(), [1, 3, 4]);
        assert_eq!(m.snapshots.len(), 1, "drie dagen later: nog geen nieuwe momentopname");
        let mut later = new.clone();
        later.snapshots = vec![snap(d(8, 9), 30)];
        assert_eq!(merge(Some(old.clone()), later, 7).snapshots.iter().map(|s| s.balance).collect::<Vec<_>>(), [10, 30]);
        let mut same = new;
        same.snapshots = vec![snap(d(8, 1), 99)];
        assert_eq!(merge(Some(old), same, 7).snapshots.iter().map(|s| s.balance).collect::<Vec<_>>(), [99]);
    }

    #[test]
    fn loading_an_older_save_drops_later_snapshots() {
        let old = Dataset { snapshots: vec![snap(d(8, 1), 1), snap(d(9, 1), 2), snap(d(10, 1), 3)], ..Default::default() };
        let new = Dataset { snapshots: vec![snap(d(9, 10), 4)], ..Default::default() };
        assert_eq!(merge(Some(old), new, 7).snapshots.iter().map(|s| s.balance).collect::<Vec<_>>(), [1, 2, 4]);
    }
}

#[cfg(test)]
mod career_tests {
    use super::*;
    use crate::data::{League, Match};
    use chrono::NaiveDate;

    #[test]
    fn a_new_club_continues_the_same_career() {
        let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();
        let game = |date, home: &str, away: &str, team: &str| Match { date, season: if date < d(2029, 7, 1) { 2028 } else { 2029 }, comp: if team == "Ajax" { "Eredivisie" } else { "Bundesliga" }.into(), home: home.into(), away: away.into(), hg: Some(2), ag: Some(0), league: true, at_home: home == team, team: team.into(), ..Default::default() };
        // het oude archief (zonder clubnaam per wedstrijd) en een uitlezing bij de nieuwe club
        let mut ajax = game(d(2029, 5, 12), "Ajax", "N.E.C.", "");
        ajax.team.clear();
        let old = Dataset { club: "AFC Ajax".into(), club_short: "Ajax".into(), club_uid: 992, today: Some(d(2029, 6, 16)), matches: vec![ajax], leagues: vec![League { season: 2028, comp: "Eredivisie".into(), ..Default::default() }], ..Default::default() };
        let new = Dataset { club: "FC Schalke 04".into(), club_short: "Schalke 04".into(), club_uid: 920, manager_uid: 7, joined: Some(d(2029, 6, 18)), today: Some(d(2029, 9, 1)), matches: vec![game(d(2029, 8, 20), "Schalke 04", "BVB", "Schalke 04")], leagues: vec![League { season: 2029, comp: "Bundesliga".into(), ..Default::default() }], ..Default::default() };
        assert_eq!(key(&new), "m7");
        let m = merge(Some(old), new, 7);
        assert_eq!(m.matches.iter().map(|x| x.team.as_str()).collect::<Vec<_>>(), ["Ajax", "Schalke 04"]);
        assert_eq!(m.leagues.len(), 2);
        assert_eq!((m.season(2028).team.as_str(), m.season(2029).team.as_str()), ("Ajax", "Schalke 04"));
        assert_eq!(m.season(2029).club, "FC Schalke 04");
        // de selectie van de vorige club telt niet mee in de vergelijking bij de nieuwe
        let snap = |club, day| Snapshot { club, date: d(2029, 6, day), ..Default::default() };
        let old = Dataset { club_uid: 992, snapshots: vec![snap(0, 10), snap(0, 16)], ..Default::default() };
        let new = Dataset { club_uid: 920, snapshots: vec![snap(920, 19)], ..Default::default() };
        let m = merge(Some(old), new, 7);
        assert_eq!(m.snapshots.iter().map(|s| s.club).collect::<Vec<_>>(), [992, 992, 920]);
        assert_eq!(m.club_snapshots().len(), 1);
    }
}
