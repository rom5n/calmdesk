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

### 1. Widget store in the gallery

A "Community" category in the gallery, listing packages from a public index.

- **Index:** new repo `rom5n/calmdesk-widgets`. One `index.json` built by CI from
  `packages/<id>/` folders. Each entry has `id`, `name`, `author`, `category`, `version`,
  `size`, `download` (release asset URL of a `.cdwidget`), `sha256`, `icon` and `preview`
  (PNG). Authors list a widget with a PR. CI checks the package with `widget-dev --check`
  (new flag, engine repo: load the package, exit non-zero on errors). CI rejects an id that
  collides with an existing package by another author.
- **Icon required:** every community package must have an icon: `icon = "assets/icon.svg"` in
  `widget.toml` (SVG, or a PNG of at least 64×64 px, square). CI rejects packages without
  one, or with a missing or wrong-size file. The gallery shows the icon on the store tile,
  in search results, in the install dialog (item 2), and in update notices. Built-in widgets
  don't need one; local packages without an icon get a generic one.
- **Manager:** `crates/manager/src/store.rs`. Fetches `index.json` at most once a day, caches it
  in `data_dir()\store\index.json`, caches previews in `data_dir()\store\previews\`.
  Install: download, check `sha256`, extract with `tar.exe -xf` into a temp folder, validate
  with `package_info`, then move into `%LOCALAPPDATA%\Widgets\<id>`. Reuses item 2's installer.
- **Gallery:** `gallery.rs` gets a "Community" category. Tiles show the preview image (not a
  live widget, the package is not on disk yet), name, author and an Install button. Installed
  packages move to their own category with a live preview, as today.
- **Updates:** on each index refresh, compare `version` with the installed `widget.toml`.
  Newer: a dot on the tile and an "Update" button; "Update all" in the Community header.
  Positions and settings survive because they key on `id`.
- **Engine:** `widget.toml` gains optional `author`, `version`, `homepage`, `icon` (schema in
  `schemas/widget.schema.json`, `PackageInfo` fields). Copy the schema here afterwards.

Done when: a package merged into the index repo appears in the gallery within a day, installs
in one click, updates in one click, and the gallery still opens instantly offline.

Risks: GitHub raw/API rate limits (cache, one fetch a day, raw URLs instead of the API);
malicious packages (declarative only, size cap on download and on extracted files, extraction
into a temp folder with path checks, `sha256` from the index).

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

### 7. Author credit and stats

Builds on the `author`/`homepage` fields from item 1.

- **Gallery:** every tile shows "by Name"; the tile's right-click (or an info button) opens
  `homepage` and an optional `sponsor` link (new optional `widget.toml` field).
- **Stats:** install count from GitHub release asset `download_count`, summed by the index CI
  into `index.json` (`installs`). Shown on store tiles; sorts the Community list
  ("Popular", "New"). No telemetry from Calmdesk itself.
- **Author page:** index CI also writes `authors/<name>.md` on the index repo with the
  author's packages and totals, so authors have a link to share.

Done when: tiles show the author and install count, and an author can see their totals
without asking anyone.

## Phase 2: bring users back

### 6. Desktop profiles

- **Data:** a profile is a folder `data_dir()\profiles\<name>\` with `instances.txt` and
  `widgets.ini`, the same files Export writes. Reuse `copy_data` and the export/import
  restart flow in `main.rs`.
- **Tray:** "Profiles" submenu: the list, "Save current as...", "Delete". Switching saves the
  current state into its profile first.
- **Automatic switching (later step):** rules in `settings.ini`: by time of day, by monitor
  count, by Windows focus/game mode. Checked on the existing minute tick.
- **Sharing:** "Share profile" writes a `.cdprofile` zip (profile files plus the ids of the
  packages it uses). Opening one imports it as a new profile and offers to install missing
  packages from the store.

Done when: a user can keep "Work" and "Gaming" layouts and switch in one click without losing
positions or settings, and a shared profile works on another PC.

### 8. Featured shelf

- **Index repo:** `featured.json`: a few entries with `title`, `package ids`, an optional
  banner image, `from`/`to` dates. Edited by hand, no Calmdesk release needed.
- **Gallery:** the first category becomes "Featured" when the index has an active shelf:
  "Widget of the week" and seasonal packs (New Year countdown, exam season).
- **What's new:** after an app update, the gallery opens once with a "What's new" card (text
  from the release notes, fetched by `update.rs` along with the tag).

Done when: changing `featured.json` changes the gallery for every user within a day.

## Phase 3: more authors

### 3. Remix any widget

- **Gallery:** right-click a tile, "Remix". Copies the package to
  `%LOCALAPPDATA%\Widgets\<name>-remix`, rewrites `id` to `local.<user>.<name>` and `name` to
  "<name> (remix)", then opens the folder's `layout.xml` in the default editor.
- **Live reload in Calmdesk:** the manager watches `%LOCALAPPDATA%\Widgets` with
  `ReadDirectoryChangesW` and reloads placed instances of a changed package, so a remix
  updates on the desktop as it is saved. Errors show as a small toast with file and line
  (same message `widget-dev` prints), the last good version stays.
- Built-in widgets that need a feed keep it: remixes of `weather`, `media` and the rest are
  fed by their original package's feed, keyed by the original id (store it as `remix_of` in
  the copied `widget.toml`).

Done when: a user who never installed Rust can change the look of the weather widget and see
it update on the desktop on every save.

### 4. Data sources without code

New bindings that the manager feeds, declared in `widget.toml`:

```toml
[[source]]
key = "btc"
url = "https://api.coinbase.com/v2/prices/BTC-USD/spot"
json = "data.amount"
every = "5m"
```

`{src.btc}` in the layout. Kinds:

- `url` + `json` path (dotted, with `[n]` for arrays);
- `url` + `rss = true` (`{src.news.1.title}` ... for the first items);
- `file` (a local text file, first line or `json` path), reloaded on change;
- `command` (output of a local command), **only** for packages in `%LOCALAPPDATA%\Widgets`
  that the user installed by hand, never for store or `.cdwidget` installs, and only after
  the user allows it once per package. A command is code; the store stays code-free.

Implementation: `crates/manager/src/sources.rs`, one worker thread like `feeds.rs`, minimum
interval one minute, shared fetch for the same URL, values sent with the existing
`widgine::set`. Last values cached so widgets show data at start and offline. Options
can feed sources (`url = "https://.../{opt.symbol}"`).

Engine: allow `[[source]]` in the schema and `{src.*}` as a binding namespace (parsed by the
engine, values supplied by the host).

Done when: a crypto price, a GitHub star count and a news feed widget can each be written with
only `widget.toml` and `layout.xml`, and the store accepts them.

Risks: users' IPs hitting arbitrary URLs is expected behavior, but list the URLs a store
package uses on its tile so users see where it connects.

## Phase 4: discovery

### 9. winget

- **Installer:** add a `--silent` switch (no window, no launch prompt) if it doesn't have one
  yet. Each version needs a stable download URL (the release asset).
- **winget:** open a manifest PR to `microsoft/winget-pkgs` for each release. Automate it with
  `wingetcreate update` in the release workflow.
- **Release workflow:** one GitHub Actions job that builds, uploads the release asset and
  updates winget.
- **Not planned:** a Microsoft Store listing. It needs a code-signing certificate, which we
  don't have. Revisit if one is bought later.
- **Without signing:** SmartScreen warns on first run of the unsigned installer. The README
  should say so and show "More info", then "Run anyway".

Done when: `winget install Calmdesk` installs the latest release, and each release updates
winget without manual steps.

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
