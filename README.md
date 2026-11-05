# Calmdesk

Desktop widgets for Windows 11, built on [widget-engine](https://github.com/rom5n/widget-engine).

![Clocks, calendar, smart stack, timers](docs/images/time.png)

![Battery, switches, screen time, system monitor, device battery](docs/images/system.png)

![Notes, tasks, photos, weather, events, Now Playing, exchange rates](docs/images/more.png)

*Every built-in widget, in liquid glass with the light theme.*

**Download:** the installer is on the [Releases](https://github.com/rom5n/calmdesk/releases) page.

A tray app that hosts the widgets: a gallery of every widget package on the PC (built-in
and the ones other apps install) with a search field, drag a widget out onto the desktop or
click it to add it. A new widget starts with the style last picked for that kind of widget.
It starts with Windows and brings back the widgets that were on the desktop.

Built-in widgets: clocks (digital with and without a background, analog, both together,
world clock), calendar, Now Playing, weather, battery, Wi-Fi / Bluetooth / energy saver /
airplane mode switches, photos (a picked photo or folder, else the Pictures folder), notes
and tasks (text files the widget opens in Notepad), screen time, timer, stopwatch and
pomodoro, system monitor (processor, memory, graphics), Bluetooth device battery, exchange
rates, calendar events (iCalendar links, such as a Google or Outlook calendar's secret
address) and a smart stack (clock, weather and load in turns). The app feeds them
(`crates/manager/src/feeds.rs`, `timers.rs`, `sensors.rs`, `events.rs`); weather comes from
Open-Meteo for the city of the PC's IP address (ipinfo.io), and the last forecast is shown
offline; rates by Exchange Rate API (open.er-api.com).

The tray menu's Settings: start with Windows, hide widgets during fullscreen apps (only on
that app's monitor), language, export and import (through `Documents\Calmdesk backup`), the
log folder (`%LOCALAPPDATA%\calmdesk\calmdesk.log`, crash reports next to it) and the update
check (GitHub releases of this repository, once a day).

Fonts in `fonts/` (Google Fonts, SIL Open Font License, licenses next to them) ship with the
app; widgets can pick them like installed fonts.

```bash
cargo build --release -p calmdesk
```

```bash
cargo build --release -p calmdesk-setup
```

`target/release/calmdesk-setup.exe` is the installer: one exe with the manager and the
built-in widgets inside. It installs for the current user (no admin rights) to
`%LOCALAPPDATA%\Programs\Calmdesk`, adds a Start menu shortcut and an Apps & features
entry, and turns on the start with Windows. Uninstall from Apps & features. Build the
app first: the installer packs the `calmdesk.exe` it finds next to it. It replaces an
install from when the app was called Widget Center or Widget Manager; the app moves the widgets
over.

Signing: with a code-signing certificate in the user's store, set `CALMDESK_CERT` to its
thumbprint and run `tools\sign.ps1 target\release\calmdesk.exe` before building the
installer, then `tools\sign.ps1 target\release\calmdesk-setup.exe`.

## Building

Calmdesk builds against a checkout of widget-engine next to this one:

```
some-folder/
  engine/     https://github.com/rom5n/widget-engine
  calmdesk/   this repository
```

```bash
git clone https://github.com/rom5n/widget-engine engine
```

```bash
git clone https://github.com/rom5n/calmdesk calmdesk
```

## Layout of this repository

| Path | What |
|---|---|
| `crates/manager` | Calmdesk: tray, widget gallery, package scan, autostart, data feeds |
| `crates/installer` | `calmdesk-setup.exe`: the app's installer |
| `crates/media` | Now Playing feed (Windows media session API) for `widgets/media`, also a standalone app |
| `widgets/` | The built-in widget packages |
| `tools/` | `gen_widgets.py` writes most of `widgets/`; `tr.py` holds their translations |
| `fonts/` | Shipped fonts (SIL Open Font License) |
| `schemas/` | Copies of the engine's schemas, for editors |

## License

MIT, see [LICENSE](LICENSE).
