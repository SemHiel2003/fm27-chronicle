//! FM27 Chronicle: seizoensoverzicht en carrièrekroniek voor Football Manager 27, live uit het spel gelezen.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod data;
mod fm;
mod i18n;
mod store;
mod theme;
mod ui;
mod widgets;

use std::path::PathBuf;

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_else(|_| ".".into()))
}

/// Instellingen en het archief per save.
pub fn data_dir() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into())).join("FM27 Chronicle")
    } else {
        home().join("Library/Application Support/FM27 Chronicle")
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        // onderzoek en tests: de hele uitlezing als JSON (naar een bestand of stdout)
        Some("dump") => {
            let ds = data::load::dataset(&|s| eprintln!("{s}"))?;
            let json = serde_json::to_string(&ds)?;
            match args.get(1) {
                Some(p) => std::fs::write(p, json)?,
                None => println!("{json}"),
            }
            Ok(())
        }
        Some("help" | "--help" | "-h") => {
            println!("fm27-chronicle            start the app\nfm27-chronicle dump [file]  read everything from the running FM27 as JSON");
            Ok(())
        }
        _ => ui::run(),
    }
}
