# Calmdesk growth plan

Eight features, plus groundwork, that keep Calmdesk on users' desktops and give widget authors a reason to build
for it. Ordered so each phase builds on the one before. Every feature names the files it
touches, what "done" means, and the risks.

Ground rules for all of it:

- No new crate dependencies. Network goes through `feeds::curl` (`curl.exe`), archives through
  `tar.exe` (both ship with Windows 11), JSON through `feeds::field` or a small parser if the
  index outgrows it.
- Packages stay declarative: `widget.toml`, `layout.xml`, assets. Nothing downloaded ever runs
  code.
- Everything that talks to the network is opt-out in Settings and fails quietly offline.

## Phase 0: groundwork

### Rename widget-engine to widgine, publish on crates.io

Every later phase changes the engine, so the rename goes first.

- **Engine repo:** rename the GitHub repo `rom5n/widget-engine` to `rom5n/widgine` (GitHub
  redirects the old URL). Crate `widget-engine` becomes `widgine` (`crates/engine/Cargo.toml`
  `name`, and `use widget_engine::` becomes `use widgine::` everywhere). The C library stays
  `widget.dll` / `widget.h` with `we_*` functions, so C hosts don't break.
- **Registry key:** keep reading `HKCU\Software\WidgetEngine\Packages` so installed apps keep
  working; also read `HKCU\Software\Widgine\Packages` and document the new one.
- **Calmdesk:** the dependency becomes `widgine = "0.1"` from crates.io instead of the
  `../engine` path. Local development overrides it with `[patch.crates-io]` pointing at the
  checkout.
- **crates.io:** fill in `description`, `license = "MIT"`, `repository`, `readme`,
  `keywords` (`widgets`, `desktop`, `windows`, `direct2d`), `categories` (`gui`), and set
  `[package.metadata.docs.rs]` `default-target = "x86_64-pc-windows-msvc"`. Check with
  `cargo publish --dry-run`, then `cargo publish`. The `widget-dev` binary ships in the same
  crate, so `cargo install widgine` gives authors the previewer without cloning.
- **Docs:** both READMEs, `docs/guide.md`, `docs/authoring.md`, `docs/api.md`, the schema
  `$id`, and the clone URLs switch to widgine. Guide step 6 uses `widgine = "0.1"`.

Done when: `cargo add widgine` works, `cargo install widgine` installs `widget-dev`, Calmdesk
builds from crates.io, and no doc says widget-engine except a "formerly widget-engine" note.

### Gallery header cleanup

- `gallery.rs`: remove the logo from the top left corner (`LOGO` and its drawing, and the
  `logo.png` / `logo-dark.png` loading that only the gallery uses).
- The search field starts at the left margin and is slightly tighter: about 28 DIPs tall
  instead of 36, with the same left and right margins as the category list.
- `SEARCH` gets its own constants instead of deriving from `LOGO`. The category list
  (`LIST_TOP`) and grid (`GRID_TOP`) move up to match.

Done when: the gallery opens with no logo, a slimmer search field aligned with the categories,
and nothing clipped in light and dark mode.

## Phase 1: reach users

### 1. Widget store in the gallery (done)

A "Community" group in the gallery, listing packages from a public index.

- **Index:** repo `rom5n/calmdesk-widgets`. Authors add `packages/<id>/` with a PR. CI (`tools/build.py`) runs
  `widget-dev --check`, requires `author`, `version` and a square icon (SVG, or PNG of at least
  64×64 px), rejects reserved `dev.widget-engine.` ids and a folder not named after the id,
  then packs every package and uploads `<id>-<version>.cdwidget` and `index.txt` to the
  `packages` release. `index.txt`: a `#` comment line, then one line per package with
  tab-separated id, sha256, url, version, author, category and name. Plain text instead of
  JSON: no parser needed.
- **Manager:** `store.rs`. Fetches `index.txt` at most once a day (release download URL, no API
  rate limit). The gallery lists every package from the index alone; a package is downloaded
  only when its tile is shown or picked (curl, 20 MB cap, `sha256` checked with certutil) and
  unpacked into `data_dir()\store\packages\<sha256>` for a live preview. Downloads that left
  the index or are older than 30 days are deleted. Offline: the cached index.
- **Gallery:** a Community button at the bottom of the sidebar opens the store's list on the
  right (scrolls, grouped by category, most downloaded first, search works inside it).
  Widgets the user installed into `%LOCALAPPDATA%\Widgets` (store, `.cdwidget`, remixes) have
  their own "Installed" row; right-click one to delete it (its folder goes to the Recycle Bin,
  its widgets leave the desktop). The other rows are grouped under "Built-in" and "Other apps".
  Clicking or dragging a store tile installs it, then adds it like any widget. The index is
  fetched at most once an hour.
- **Updates:** a store package newer than the installed one (`version`) shows in Community
  with "Update available"; picking it replaces the installed one. "Update all" not done.
- **Engine:** `widget.toml` gained optional `author`, `version`, `homepage`, `icon` (schema,
  `PackageInfo` fields; the icon must be a file in the package). Old engines reject these
  fields (`deny_unknown_fields`), so store packages need widgine 0.1.1.

Not done yet: showing the icon in the gallery (the live preview fills the tile); "Update all";
a sidebar that scrolls when there are many categories.

### 2. `.cdwidget` files, double-click to install (done)

A `.cdwidget` is a zip of the package folder.

- **Installer:** `crates/installer` registers `.cdwidget` under
  `HKCU\Software\Classes` (ProgID `Calmdesk.Widget`, icon from `app.ico`, open command
  `calmdesk.exe --install "%1"`). Uninstall removes it.
- **Manager:** `main.rs` handles `--install <file>`: forwards it to the running instance
  (same path as the existing single-instance handling) or starts and installs. Shared install
  code in `install.rs` (the store reuses it): extract to temp, reject entries with `..` or absolute paths, validate,
  confirm with a small dialog ("Install Weather by Name?"), move into
  `%LOCALAPPDATA%\Widgets\<id>`, then open the gallery on the new widget.
- **Dropping:** dropping a `.cdwidget` on the gallery installs it too. (Not done yet: needs drag-and-drop in engine panels.)
- **Authors:** `widget-dev pack <dir>` (engine repo) writes `<id>-<version>.cdwidget`.
- **Docs:** README "For widget authors" and engine `docs/guide.md` step 8 mention `.cdwidget`.

Done when: double-clicking a `.cdwidget` from Downloads installs it and shows it in the
gallery, on a PC where Calmdesk was not running.

### 7. Author credit and stats (done)

- **Gallery:** store tiles show "Author · ↓downloads"; other apps' tiles show `author` when the
  package sets it. The Community list is sorted by downloads.
- **Stats:** download counts of the release assets, summed per id by the index CI (daily
  schedule) into the 8th column of `index.txt`. They count preview downloads too. No telemetry
  from Calmdesk itself.
- **Author page:** the index CI writes `authors/<name>.md` (widgets, versions, downloads,
  total) and commits it.

Not done: opening `homepage` from a tile, a `sponsor` field, a "New" sort.

## Phase 2: bring users back

### 6. Desktop profiles (done: manual switching)

- **Data:** `data_dir()\profiles\<name>\` holds `instances.txt` and `widgets.ini`. Notes,
  tasks and settings stay shared. Backups (Export) leave out profiles and store downloads.
- **Tray:** "Profiles" submenu: the profiles (current one checked), "Save as new profile"
  (named "Profile N"), "Open profiles folder" (rename or delete a profile there). Switching
  restarts Calmdesk with `--profile <name>`: the current desktop is saved into its profile
  first, then the other one is copied in.

Not done: automatic switching rules (time of day, monitors, game mode) and `.cdprofile`
sharing.

### 8. Featured shelf (done)

- **Index repo:** `featured.txt`, edited by hand: title, first day, last day, package ids,
  tab-separated. CI checks the dates and ids and publishes it with the index.
- **Gallery:** a "Featured" group at the top of the sidebar, one row per shelf showing today,
  with the installed or store version of each widget.
- **What's new:** after an update, the tray menu has "What's new in X" until it is opened; it
  opens the release notes on GitHub.

Not done: banner images; a "What's new" card in the gallery.

## Phase 3: more authors

### 3. Remix any widget (done)

- **Gallery:** right-click a tile, "Remix" (panels now get `Event::RightClick` from the
  engine). Copies the package to `%LOCALAPPDATA%\Widgets\<folder>-remix`, sets `id` to the
  original id plus `.remix` (`.remix2`, ...) and adds " (remix)" to `name`, then opens
  `layout.xml` in Notepad and the folder in Explorer.
- **Live reload:** the engine already reloads placed widgets when their package changes; a
  broken edit keeps the last good version (the error goes to the log).
- **Feeds:** `remix::original` strips the `.remix` suffix, so weather, media and the other
  feeds treat a remix like its original. No `remix_of` field needed.

Not done: an error toast with file and line.

### 4. Data sources without code (done)

```toml
[[source]]
key = "btc"
url = "https://api.coinbase.com/v2/prices/BTC-USD/spot"
json = "data.amount"
every = "5m"
```

`{src.btc}` in the layout. Kinds: `url` (first line), `url` + `json` path (dotted, `[n]` for
arrays), `url` + `rss = true` (`{src.news.1.title}`, `.link`, first 10 items, RSS or Atom),
`file` (first line or `json` path, read every 15 s). `url` may use `{opt.<key>}`.

- **Engine:** `[[source]]` in the manifest and schema (`PackageInfo::sources`), `src.*` host
  values readable as `{src.*}`.
- **Calmdesk:** `sources.rs`, one worker thread, minimum interval one minute, one fetch per
  address per round, last values in `data_dir()\sources.txt` for start and offline.
  Uses `serde_json`.
- **Examples:** `engine/examples/sources`: Bitcoin price, GitHub stars, Hacker News. All three
  checked live.

Not done: the `command` kind (code; the store stays code-free), listing a store package's URLs
on its tile.

## Phase 4: discovery

### 9. winget (done, first submission by hand)

- **Installer:** `--silent` (or `/S`) installs without a window and exits with 0 or 1.
- **Release workflow:** `.github/workflows/release.yml`. Publishing a release on GitHub builds
  the installer and attaches it as `Calmdesk-Setup.exe`.
- **winget:** the first version must be submitted by hand (`wingetcreate new` with the release
  URL, installer type `exe`, silent switch `--silent`). After that, with a `WINGET_TOKEN`
  secret (a token that may open PRs on `microsoft/winget-pkgs`), the workflow runs
  `wingetcreate update rom5n.Calmdesk` for each release.
- **Not planned:** a Microsoft Store listing. It needs a code-signing certificate.
- **Without signing:** the README tells users about SmartScreen ("More info", "Run anyway").

## Order and size

| Order | Feature | Repos | Size |
|---|---|---|---|
| 0 | Rename to widgine, crates.io | engine, calmdesk | M |
| 0 | Gallery header cleanup | calmdesk | S |
| 1 | 2. `.cdwidget` install | calmdesk, engine (`pack`) | S |
| 2 | 1. Store (index, list, install, update) | calmdesk, engine, new index repo | L |
| 3 | 7. Author credit and stats | calmdesk, index repo | S |
| 4 | 8. Featured shelf | calmdesk, index repo | S |
| 5 | 6. Profiles (manual, then rules, then sharing) | calmdesk | M |
| 6 | 3. Remix and live reload | calmdesk | M |
| 7 | 4. Data sources | engine, calmdesk | L |
| 8 | 9. winget | calmdesk, release workflow | S |

`.cdwidget` comes first because the store installs the same files. Data sources come late
because they change the package format: they ship with an `engine = "^0.2"` bump, so the index
CI can keep 0.1 packages working for users who have not updated.
