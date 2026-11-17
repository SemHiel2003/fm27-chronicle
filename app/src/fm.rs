//! Verbinding met FM27. macOS: via de socket van de module die met FM meedraait (dezelfde module en
//! dezelfde socket als FM27 Trainer, zodat het niet uitmaakt welke app FM gestart heeft). Windows: de
//! app leest fm.exe zelf (crate fm27-agent). Deze app stuurt **alleen leesopdrachten**.

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::{Command, Stdio};

const STEAM_APP_ID: &str = "4530370";

/// Map van de module en zijn socket: gedeeld met FM27 Trainer.
pub fn module_dir() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into())).join("FM27 Trainer")
    } else {
        crate::home().join("Library/Application Support/FM27 Trainer")
    }
}

#[cfg(target_os = "macos")]
fn socket_path() -> PathBuf {
    module_dir().join("agent.sock")
}

#[cfg(windows)]
pub fn request(req: Value) -> Result<Value> {
    fm27_agent::request(&req).map_err(|e| anyhow!(e))
}

#[cfg(target_os = "macos")]
pub fn request(req: Value) -> Result<Value> {
    use std::io::{BufRead, BufReader, Write};
    let mut s = std::os::unix::net::UnixStream::connect(socket_path())
        .map_err(|_| anyhow!("{}", crate::t!("module niet bereikbaar (draait FM27 met de module?)", "module not reachable (is FM27 running with the module?)")))?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(120)))?;
    writeln!(s, "{req}")?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line)?;
    let v: Value = serde_json::from_str(&line)?;
    if v.get("ok").and_then(|x| x.as_bool()) == Some(true) {
        Ok(v.get("data").cloned().unwrap_or(Value::Null))
    } else {
        bail!("{}", v.get("error").and_then(|e| e.as_str()).unwrap_or("?"))
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn request(_req: Value) -> Result<Value> {
    bail!("not available on this system")
}

#[cfg(not(windows))]
fn pgrep(name: &str) -> bool {
    Command::new("pgrep").args(["-x", name]).stdout(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

pub fn fm_running() -> bool {
    #[cfg(windows)]
    {
        fm27_agent::fm_pid().is_some()
    }
    #[cfg(not(windows))]
    {
        pgrep("Football Manager 27")
    }
}

/// Of er iets is om mee te verbinden.
pub fn module_ready() -> bool {
    #[cfg(target_os = "macos")]
    {
        socket_path().exists() && fm_running()
    }
    #[cfg(not(target_os = "macos"))]
    {
        fm_running()
    }
}

#[cfg(target_os = "macos")]
fn fm_executable() -> Option<PathBuf> {
    let p = crate::home().join("Library/Application Support/Steam/steamapps/common/Football Manager 27/fm.app/Contents/MacOS/Football Manager 27");
    if p.exists() {
        return Some(p);
    }
    let out = Command::new("mdfind").arg("kMDItemCFBundleIdentifier == 'com.SportsInteractive.FootballManager27'").output().ok()?;
    let app = String::from_utf8_lossy(&out.stdout).lines().next()?.to_string();
    let p = PathBuf::from(app).join("Contents/MacOS/Football Manager 27");
    p.exists().then_some(p)
}

#[cfg(target_os = "macos")]
fn agent_source() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join("libfm27_agent.dylib"), dir.join("../Resources/libfm27_agent.dylib"), dir.join("../release/libfm27_agent.dylib")]
        .into_iter()
        .find(|c| c.exists())
}

/// Start FM27. macOS: met de module (FM mag nog niet draaien, Steam moet open zijn). Windows: via Steam.
#[cfg(target_os = "macos")]
pub fn start_fm() -> Result<String> {
    use std::os::unix::process::CommandExt;
    use std::time::Duration;
    if fm_running() {
        bail!("{}", crate::t!("FM27 draait al. Sla je spel op, sluit FM27 en probeer opnieuw.", "FM27 is already running. Save your game, quit FM27 and try again."));
    }
    let exe = fm_executable().ok_or_else(|| anyhow!("Football Manager 27 not found"))?;
    let src = agent_source().ok_or_else(|| anyhow!("libfm27_agent.dylib not found next to the app"))?;
    std::fs::create_dir_all(module_dir())?;
    let dst = module_dir().join("fm27_agent.dylib");
    std::fs::copy(&src, &dst)?;
    let _ = Command::new("xattr").args(["-d", "com.apple.quarantine"]).arg(&dst).stderr(Stdio::null()).status();
    let _ = std::fs::remove_file(socket_path());
    if !pgrep("steam_osx") {
        let _ = Command::new("open").args(["-g", "-a", "Steam"]).status();
        for _ in 0..90 {
            if pgrep("steam_osx") {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        std::thread::sleep(Duration::from_secs(8));
    }
    Command::new(&exe)
        .env("DYLD_INSERT_LIBRARIES", &dst)
        .env("SteamAppId", STEAM_APP_ID)
        .env("SteamGameId", STEAM_APP_ID)
        .current_dir(exe.parent().unwrap_or(&exe))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    Ok(crate::t!("FM27 wordt gestart. Laad je save; de app verbindt vanzelf.", "FM27 is starting. Load your save; the app connects automatically.").into())
}

#[cfg(windows)]
pub fn start_fm() -> Result<String> {
    use std::os::windows::process::CommandExt;
    if fm_running() {
        return Ok(crate::t!("FM27 draait al. Laad je save; de app verbindt vanzelf.", "FM27 is already running. Load your save; the app connects automatically.").into());
    }
    Command::new("cmd")
        .args(["/C", "start", "", &format!("steam://rungameid/{STEAM_APP_ID}")])
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(crate::t!("FM27 wordt gestart via Steam. Laad je save; de app verbindt vanzelf.", "FM27 is starting through Steam. Load your save; the app connects automatically.").into())
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn start_fm() -> Result<String> {
    bail!("not available on this system")
}

// ---- geheugen lezen --------------------------------------------------------------------------

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).filter_map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()).collect()
}

pub fn parse_addr(v: &Value) -> Option<u64> {
    u64::from_str_radix(v.as_str()?.trim_start_matches("0x"), 16).ok()
}

/// Leest `len` bytes (in stukken van 64 KiB, de limiet van de module).
pub fn read(addr: u64, len: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(len);
    let mut a = addr;
    while out.len() < len {
        let k = (len - out.len()).min(1 << 16);
        let r = request(json!({"cmd": "read", "addr": format!("{a:#x}"), "len": k}))?;
        let b = unhex(r.get("hex").and_then(|h| h.as_str()).ok_or_else(|| anyhow!("read"))?);
        if b.len() != k {
            bail!("short read");
        }
        out.extend(b);
        a += k as u64;
    }
    Ok(out)
}

pub fn u64_at(b: &[u8], off: usize) -> u64 {
    b.get(off..off + 8).map(|s| u64::from_le_bytes(s.try_into().unwrap())).unwrap_or(0)
}
pub fn u32_at(b: &[u8], off: usize) -> u32 {
    b.get(off..off + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).unwrap_or(0)
}
pub fn u16_at(b: &[u8], off: usize) -> u16 {
    b.get(off..off + 2).map(|s| u16::from_le_bytes(s.try_into().unwrap())).unwrap_or(0)
}

pub fn is_ptr(v: u64) -> bool {
    v > 0x10000 && v < 0x8000_0000_0000 && v % 8 == 0
}

/// FM-string: u32 lengte + UTF-8, soms via nog een pointer.
pub fn fm_string(ptr: u64) -> Option<String> {
    if ptr < 0x10000 {
        return None;
    }
    let direct = |p: u64| -> Option<String> {
        let head = read(p, 4).ok()?;
        let n = u32_at(&head, 0) as usize;
        if n == 0 || n > 200 {
            return None;
        }
        let s = String::from_utf8(read(p + 4, n).ok()?).ok()?;
        (!s.chars().any(|c| c.is_control())).then_some(s)
    };
    direct(ptr).or_else(|| {
        let p = u64_at(&read(ptr, 8).ok()?, 0);
        is_ptr(p).then(|| direct(p)).flatten()
    })
}

/// Alle adressen in de heap waar precies deze 8 bytes staan (bv. een vtable: alle objecten van een klasse).
pub fn find_u64(target: u64, limit: usize) -> Result<Vec<u64>> {
    let r = request(json!({"cmd": "find_ptr", "target": format!("{target:#x}"), "limit": limit}))?;
    Ok(r.as_array().map(|a| a.iter().filter_map(parse_addr).collect()).unwrap_or_default())
}

/// De vtables van een klasse (RTTI), op naam.
pub fn vtables(class: &str) -> Result<Vec<u64>> {
    // de module filtert op de ruwe (gemangelde) naam: zoek op het laatste deel, vergelijk de hele naam
    let r = request(json!({"cmd": "census", "filter": class.rsplit("::").next().unwrap_or(class)}))?;
    let mut out = Vec::new();
    for c in r.as_array().into_iter().flatten() {
        if c.get("class").and_then(|n| n.as_str()) == Some(class) {
            out.extend(c.get("vtables").and_then(|v| v.as_array()).into_iter().flatten().filter_map(parse_addr));
        }
    }
    Ok(out)
}

/// Leest veel records van vaste grootte; buren worden in één leesopdracht gehaald.
pub fn read_records(mut addrs: Vec<u64>, size: usize) -> Vec<(u64, Vec<u8>)> {
    addrs.sort_unstable();
    addrs.dedup();
    let mut out = Vec::with_capacity(addrs.len());
    let mut i = 0;
    while i < addrs.len() {
        let start = addrs[i];
        let mut j = i;
        while j + 1 < addrs.len() && addrs[j + 1] + size as u64 - start <= 1 << 16 {
            j += 1;
        }
        let span = (addrs[j] + size as u64 - start) as usize;
        match read(start, span) {
            Ok(b) => {
                for &a in &addrs[i..=j] {
                    let o = (a - start) as usize;
                    out.push((a, b[o..o + size].to_vec()));
                }
            }
            Err(_) => {
                // een gat in het geheugen binnen de reeks: dan per record
                for &a in &addrs[i..=j] {
                    if let Ok(b) = read(a, size) {
                        out.push((a, b));
                    }
                }
            }
        }
        i = j + 1;
    }
    out
}
