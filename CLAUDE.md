# FM27 Chronicle: overdracht voor Claude

Losse app naast FM27 Trainer (`/Users/semhiel/Documents/fm27`), begonnen op 17 nov 2026. De gebruiker (Sem) is
Nederlandstalig: antwoord in het Nederlands, kort en concreet. Lees ook de `CLAUDE.md` van FM27 Trainer voor de
module, de veiligheidsregels en de vaste plekken per FM-build.

## Wat het is

Seizoensoverzicht en carrièrekroniek voor Football Manager 27, live uit het spel gelezen. **De app leest alleen**:
hij stuurt nooit een schrijfopdracht. Aanleiding: marktonderzoek van 17 nov 2026 ("season reviews" en de
"trophies tab" zijn de functies die spelers in Steam-reviews als gemist noemen; bestaande tools FM Season Hub en
FM Dossier hebben geen Mac-versie).

## Opbouw

```
agent/        ongewijzigde kopie van crate fm27-agent uit FM27 Trainer (module op de Mac, lezen van buitenaf op Windows)
app/src/
  main.rs     CLI: (geen) = app, `dump [bestand]` = hele uitlezing als JSON
  fm.rs       verbinding (Mac: socket van de module in de map van FM27 Trainer; Windows: fm27_agent::request),
              FM starten, geheugen lezen (read, find_ptr op een vtable, FM-strings)
  data.rs     model + rekenlogica (stand, reeksen, prijzen, bekerreizen) met tests; onderaan `load::dataset`
  store.rs    archief per save in de datamap (`saves/<club-uid>-<hash>.json`), samenvoegen met tests
  ui.rs       de app: zijbalk, verversen op de achtergrond, alle pagina's, deelkaart
  widgets.rs  getekende bouwstenen (grafiek, vormstrook, balken, gloed)
  theme.rs, i18n.rs  uit FM27 Trainer (t!("nl", "en"))
```

De app gebruikt alleen de algemene opdrachten van de module (`chain`, `club`, `squad`, `tactics`, `game_date`,
`census`, `find_ptr`, `read`). Daardoor werkt hij met elke module vanaf 0.2.7 en maakt het op de Mac niet uit of
FM met Trainer of met Chronicle gestart is (zelfde dylib, zelfde socket). **De `agent/`-map hier niet wijzigen**:
verbeteringen horen in FM27 Trainer en worden hierheen gekopieerd.

## Wat er uit het geheugen komt (gemeten op macOS arm64, Steam 26620374, Ajax-save, alleen lezen)

- `sicomps::FIXTURE_RESULT`: record van 0x80 bytes, vtable op +0. `+0x08` thuisteam, `+0x10` uitteam (db::TEAM),
  `+0x20` db::FIXTURE_NAME, `+0x30` u16 seizoen (0 = oefenduel), `+0x38` u8 fase, `+0x39` u8 ronde-index
  (0xFF = competitievorm), `+0x3B` u8 speelronde, `+0x4C` datum (u16 dag van het jaar, lage 9 bits, + u16 jaar),
  `+0x5C` u32 toeschouwers, `+0x64` thuisdoelpunten (0xFF = niet gespeeld), `+0x69` uitdoelpunten,
  `+0x67`/`+0x6C` totaal over twee duels. `+0x66`/`+0x6B` worden als strafschoppen gelezen: **aanname, niet
  bevestigd** (geen finale met strafschoppen in de save).
- `db::FIXTURE_NAME` (0x48 bytes): `+0x18` db::COMP, `+0x20` pad/fase, `+0x28` ronde, `+0x30` heen/terug
  (db::STAGE_NAME: `+0x18` naam, `+0x20` kort, `+0x28` meervoud). db::COMP `+0x40` naam, `+0x48` kort.
- FM bewaart alle wedstrijden van alle clubs sinds het begin van de save (229.000 records na drie seizoenen),
  met tijdelijke kopieën ertussen; een lopende wedstrijd heeft kopieën met verschillende standen. De app
  ontdubbelt op (datum, thuis, uit), neemt de meest voorkomende uitslag en telt wedstrijden van vandaag niet mee.
- Finale = hoogste (fase, ronde) van dat toernooi in dat seizoen over alle clubs, met precies één duel.
  Supercups (één wedstrijd, geen rondenaam) vallen daar vanzelf onder.
- De stand wordt berekend uit alle uitslagen met dezelfde FIXTURE_NAME en hetzelfde seizoen als de eigen
  competitie: punten, doelsaldo, doelpunten voor.
- **Niet gevonden**: doelpuntenmakers en speelminuten per speler. `fixture+0x78` is geen betrouwbare pointer,
  `db::PLAYING_HISTORY` is na het laden een lege tabel, en naar een spelerobject wijst bijna niets.
  Kandidaten voor later: `GAME_PLAYER_SEASON_KEY_STATS`, `PLAYER_STATS_MANAGER`, `GAME_TEAM_COMP_STATS`
  (142.900 objecten van 0x1E0, niet bekeken).

## De carrière volgt de manager, niet de club (17 nov 2026, na een melding van Sem)

Sem stapte over van Ajax naar Schalke 04 en de app toonde toen de historie van Schalke (met een Conference League
die Schalke onder de computer won). Nu: het archief staat per manager (`saves/m<manager-uid>.json`; de UID is
`u32(contract.owner + 0x0C)`), wedstrijden van vóór `contract + 0x4C` (datum "bij club") tellen niet, elke wedstrijd
onthoudt de club van toen (`Match.team`) en momentopnames horen bij een club (`Snapshot.club`): selectie en geld
vergelijken alleen binnen de huidige club. Archieven van vóór deze wijziging (per club) worden één keer overgenomen
(`store::find`). Live gezien: na de overstap blijven de 179 Ajax-wedstrijden en zeven prijzen staan. Een seizoen
met wedstrijden bij de nieuwe club is nog niet gezien; een overstap midden in een seizoen ook niet.

## Getest

- 8 tests (`cargo test -p fm27-chronicle`): stand, reeksen, twee duels en strafschoppen, prijzen, archief.
- Live op de Mac (17 nov 2026): 179 wedstrijden, drie Eredivisie-seizoenen (306 duels elk), zeven prijzen
  herkend (o.a. Europa League 2027/28, twee landstitels). **Niet door Sem tegen FM's schermen nagekeken.**
  Een volledige uitlezing duurt ~10 s; de app leest alleen opnieuw als de speldatum veranderd is (controle
  elke 20 s).
- **Windows: niet gebouwd en niet getest.** De posities van FIXTURE_RESULT op Windows zijn onbekend
  (clubs/competities waren tot nu toe op beide platforms gelijk; dat dit ook hier geldt is een aanname).
- Selectie en financiën door de tijd: alleen met één momentopname gezien; de grafieken met meerdere punten
  zijn niet bekeken.

## Testhulpen

`FMC_DATA=<dump.json>` (toont een uitlezing zonder FM), `FMC_PAGE=season|matches|table|career|squad|money|card|settings`,
`FMC_LANG=nl|en`, `FMC_SEASON=2027`, `FMC_WINDOW=1360x1500`, `FMC_SNAPSHOT=pad.png` (opname van het eigen venster,
dan afsluiten; pakt de focus niet af). Datamap: `~/Library/Application Support/FM27 Chronicle`
(Windows `%APPDATA%\FM27 Chronicle`).

## Uitgebracht: 0.1.0 (17 nov 2026)

- Openbare repo `SemHiel2003/fm27-chronicle` (remote `origin`), tag `v0.1.0`. Windows-zip via GitHub Actions
  (`.github/workflows/release.yml`, bij een tag `v*`; notes in `docs/releases/<tag>.md`), Mac-DMG via
  `./release.sh` (universeel, Developer ID, genotariseerd) en `gh release upload`. Vaste downloadlinks:
  `…/releases/latest/download/FM27-Chronicle-macOS.dmg` en `…/FM27-Chronicle-Windows.zip`.
- `./release.sh` niet door `head` of een andere afkappende pipe sturen: dan stopt het script halverwege.
- Sortitoutsi: post 78652 (type download, "Tools, Editors and Spreadsheets"; daar is een geüpload bestand
  verplicht, dus de zip en de DMG staan er ook: **bij elke release opnieuw uploaden**, anders loopt hun kopie
  achter). De Trainer-post (78370) verwijst naar Chronicle en andersom; beide hebben de Ko-fi-link.
- Deelkaart: keuze seizoen of hele carrière (`CardData`, `season_card`, `career_card` in `ui.rs`).
- **Windows blijft ongetest** (alleen `cargo check --target x86_64-pc-windows-msvc` en de build in Actions).

## Nog te doen

- Windows live controleren (posities van FIXTURE_RESULT); doelpuntenmakers zoeken.
- Niet pushen of uitbrengen zonder dat Sem het vraagt.
