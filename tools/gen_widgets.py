"""Writes the built-in widget packages into ../widgets: python -I tools/gen_widgets.py. Translations: tr.py."""
import math, os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'widgets')
HEAD = '<widget xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="../../schemas/layout.xsd">\n'

import sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from tr import T, LANGS  # noqa: E402


def toml_str(s):
    return '"' + s.replace('\\', '\\\\').replace('"', '\\"') + '"'


def pkg(folder, name, category, size, layout, keys=(), variant=None, card=True, font=('Nunito', 400),
        allow=('size', 'background', 'theme', 'font'), assets=None, options=(), background='frosted', radius=26):
    d = os.path.join(ROOT, folder)
    os.makedirs(d, exist_ok=True)
    t = ['#:schema ../../schemas/widget.schema.json', f'id = "dev.widget-engine.{folder}"', f'name = {toml_str(name)}']
    if variant:
        t.append(f'variant = {toml_str(variant)}')
    t += [f'category = "{category}"', 'engine = "^0.1"', f'size = [{size[0]}, {size[1]}]', '']
    if card:
        t += ['[card]', f'radius = {radius}', f'background = "{background}"', 'border = false', 'shadow = true', '']
    t += ['[font]', f'family = "{font[0]}"', f'weight = {font[1]}', '']
    t += ['[customize]', 'allow = [' + ', '.join(f'"{a}"' for a in allow) + ']', '']
    keys = [name] + ([variant] if variant else []) + list(keys)
    # options: (key, label, [(value, label)] or None, default, kind)
    for key, label, values, default, kind in options:
        t += ['[[option]]', f'key = "{key}"', f'label = {toml_str(label)}']
        if kind:
            t.append(f'kind = "{kind}"')
        if values:
            t.append('values = [' + ', '.join(f'[{toml_str(v)}, {toml_str(lb)}]' for v, lb in values) + ']')
            keys += [lb for _, lb in values if lb in T]
        if default:
            t.append(f'default = {toml_str(default)}')
        t.append('')
        keys.append(label)
    seen = []
    for k in keys:
        if k not in seen:
            seen.append(k)
    for col, lang in enumerate(LANGS):
        t.append(f'[strings.{lang}]')
        t += [f'{toml_str(k)} = {toml_str(T[k][col])}' for k in seen]
        t.append('')
    open(os.path.join(d, 'widget.toml'), 'w', encoding='utf-8', newline='\n').write('\n'.join(t).rstrip() + '\n')
    open(os.path.join(d, 'layout.xml'), 'w', encoding='utf-8', newline='\n').write(HEAD + layout.strip('\n') + '\n</widget>\n')
    for name_, svg in (assets or {}).items():
        os.makedirs(os.path.join(d, 'assets'), exist_ok=True)
        open(os.path.join(d, 'assets', name_), 'w', encoding='utf-8', newline='\n').write(svg.strip() + '\n')


def svg(body, box=24):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {box} {box}">{body}</svg>'


# --- icons -----------------------------------------------------------------------------------
ICON = {
    'wifi': lambda c: f'<g fill="none" stroke="{c}" stroke-width="2.1" stroke-linecap="round"><path d="M2.8 9.2a13 13 0 0 1 18.4 0"/><path d="M6.1 12.6a8.3 8.3 0 0 1 11.8 0"/><path d="M9.4 16a3.7 3.7 0 0 1 5.2 0"/></g><circle cx="12" cy="19.4" r="1.6" fill="{c}"/>',
    'bluetooth': lambda c: f'<path d="M7 7.5l10 9-5 4.5V3l5 4.5-10 9" fill="none" stroke="{c}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>',
    'leaf': lambda c: f'<path d="M5.6 18.4C4.4 10.6 9.4 5.2 19.6 4.4c.6 10.4-5.2 15.6-13.4 14.4" fill="none" stroke="{c}" stroke-width="2" stroke-linejoin="round"/><path d="M4.6 19.4l8.2-8.2" stroke="{c}" stroke-width="2" stroke-linecap="round"/>',
    'airplane': lambda c: f'<path fill="{c}" d="M21 15.5v-1.8l-7.6-4.8V3.6a1.4 1.4 0 0 0-2.8 0v5.3L3 13.7v1.8l7.6-2.4v5.1l-2 1.5V21l3.4-1 3.4 1v-1.3l-2-1.5v-5.1z"/>',
}


def icon_assets(names):
    out = {}
    for n in names:
        out[f'{n}.svg'] = svg(ICON[n]('{foreground}'))
        out[f'{n}-on.svg'] = svg(ICON[n]('#FFFFFF'))
    return out


def toggle(flag, action, icon, d, i):
    """A round switch: accent when on."""
    return f'''<stack width="{d}" height="{d}" action="{action}" scale="1" pressed.scale="0.88" transition="spring">
      <ellipse if="app.{flag}" width="fill" height="fill" fill="accent"/>
      <ellipse if="!app.{flag}" width="fill" height="fill" fill="#80808030"/>
      <image if="app.{flag}" src="assets/{icon}-on.svg" width="{i}" height="{i}"/>
      <image if="!app.{flag}" src="assets/{icon}.svg" width="{i}" height="{i}"/>
    </stack>'''


def on_off(flag, size, indent='  '):
    return (f'{indent}<text if="app.{flag}" value="{{str.On}}" size="{size}" color="secondary"/>\n'
            f'{indent}<text if="!app.{flag}" value="{{str.Off}}" size="{size}" color="secondary"/>')


RADIOS = [('wifi', 'wifi', 'wifi', 'Wi-Fi'), ('bluetooth', 'bt', 'bluetooth', 'Bluetooth'),
          ('energy-saver', 'saver', 'leaf', 'Energy saver'), ('airplane', 'plane', 'airplane', 'Airplane mode')]
ACTION = {'wifi': 'wifi', 'bt': 'bluetooth', 'saver': 'saver', 'plane': 'airplane'}

for folder, flag, icon, label in RADIOS:
    pkg(folder, label, 'System', (170, 170), f'''
  <!-- The switch takes clicks; double-click and hold anywhere else moves the widget. -->
  <column padding="18" width="fill" height="fill">
    {toggle(flag, ACTION[flag], icon, 54, 26)}
    <spacer grow="1"/>
    <text value="{{str.{label}}}" size="16" weight="600" width="fill"/>
{on_off(flag, 13, '    ')}
  </column>''', keys=['On', 'Off'], assets=icon_assets([icon]))


def tile(flag, icon, label):
    return f'''    <row grow="1" height="fill" gap="10" align="center" padding="0 10" radius="18" fill="#8080801A">
      {toggle(flag, ACTION[flag], icon, 38, 19)}
      <column grow="1">
        <text value="{{str.{label}}}" size="12" weight="600" ellipsis="true" width="fill"/>
{on_off(flag, 12, '        ')}
      </column>
    </row>'''


pkg('connectivity', 'Connectivity', 'System', (352, 170), f'''
  <column padding="14" gap="10" width="fill" height="fill">
    <row gap="10" width="fill" grow="1">
  {tile('wifi', 'wifi', 'Wi-Fi')}
  {tile('bt', 'bluetooth', 'Bluetooth')}
    </row>
    <row gap="10" width="fill" grow="1">
  {tile('saver', 'leaf', 'Saver')}
  {tile('plane', 'airplane', 'Airplane')}
    </row>
  </column>''', keys=['Wi-Fi', 'Bluetooth', 'Saver', 'Airplane', 'On', 'Off'],
    assets=icon_assets(['wifi', 'bluetooth', 'leaf', 'airplane']))

BOLT = svg('<path fill="#34C759" d="M13.2 2 4.5 13.4h6.3L9.9 22l8.6-11.4h-6.3z"/>')
STATUS = '''<text if="app.charging" value="{str.Charging}" size="13" color="secondary"/>
    <text if="app.plugged" value="{str.Plugged in}" size="13" color="secondary"/>
    <text if="app.unplugged" value="{str.On battery}" size="13" color="secondary"/>'''
# The level arc: app.battery_dash is the level times the circumference (r = 42).
RING = '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <circle cx="50" cy="50" r="42" fill="none" stroke="{foreground}" stroke-opacity="0.12" stroke-width="9"/>
  <circle cx="50" cy="50" r="42" fill="none" stroke="{app.battery_color}" stroke-width="9" stroke-linecap="round"
          stroke-dasharray="{app.battery_dash} 400" transform="rotate(-90 50 50)"/>
</svg>'''
pkg('battery', 'Battery', 'System', (170, 170), f'''
  <column padding="14" gap="8" width="fill" height="fill" align="center" justify="center">
    <stack width="100" height="100">
      <image if="app.battery" src="assets/ring.svg" width="fill" height="fill"/>
      <column align="center">
        <image if="app.charging" src="assets/bolt.svg" width="16" height="16"/>
        <text value="{{app.percent}}" size="24" weight="600" features="tnum"/>
      </column>
    </stack>
    {STATUS}
  </column>''', variant='Small', keys=['Charging', 'Plugged in', 'On battery'], assets={'ring.svg': RING, 'bolt.svg': BOLT})

# A battery glyph; app.battery_w is the level times the inner width (50).
GLYPH = '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 30">
  <rect x="1" y="1" width="56" height="28" rx="8" fill="none" stroke="{foreground}" stroke-opacity="0.35" stroke-width="2"/>
  <rect x="59" y="10" width="3.5" height="10" rx="1.75" fill="{foreground}" fill-opacity="0.35"/>
  <rect x="4" y="4" width="{app.battery_w}" height="22" rx="5" fill="{app.battery_color}"/>
</svg>'''
pkg('battery-wide', 'Battery', 'System', (352, 170), f'''
  <column padding="18 22" width="fill" height="fill">
    <row width="fill" align="center" gap="6">
      <text value="{{str.Battery}}" size="15" weight="600"/>
      <image if="app.charging" src="assets/bolt.svg" width="15" height="15"/>
    </row>
    <spacer grow="1"/>
    <row width="fill" align="center" gap="14">
      <column grow="1">
        <text value="{{app.percent}}" size="50" weight="300" features="tnum"/>
        {STATUS}
      </column>
      <image if="app.battery" src="assets/battery.svg" width="128" height="60"/>
    </row>
  </column>''', variant='Wide', keys=['Charging', 'Plugged in', 'On battery'], assets={'battery.svg': GLYPH, 'bolt.svg': BOLT})

# --- photos ----------------------------------------------------------------------------------
PHOTO_ICON = svg('<rect x="2.5" y="4.5" width="19" height="15" rx="3" fill="none" stroke="#FFFFFF" stroke-width="1.8"/>'
                 '<path d="M3 17l5.5-5.5 4 4 2.5-2.5 6 6" fill="none" stroke="#FFFFFF" stroke-width="1.8" stroke-linejoin="round"/>'
                 '<circle cx="16" cy="9" r="1.8" fill="#FFFFFF"/>')
PHOTO_OPTIONS = [('photo', 'Photo', None, None, 'image'), ('folder', 'Folder', None, None, 'folder')]
PROMPT_ICON = svg('<rect x="2.5" y="4.5" width="19" height="15" rx="3" fill="none" stroke="#FFFFFF" stroke-width="1.8"/>'
                  '<path d="M3 17l5.5-5.5 4 4 2.5-2.5 6 6" fill="none" stroke="#FFFFFF" stroke-width="1.8" stroke-linejoin="round"/>'
                  '<circle cx="16" cy="9" r="1.8" fill="#FFFFFF"/>')
for folder, variant, size, round_ in [('photo-square', 'Square', (170, 170), False), ('photo-round', 'Round', (170, 170), True),
                                      ('photo-wide', 'Wide', (352, 170), False), ('photo-tall', 'Tall', (170, 352), False)]:
    r = f'radius="{size[0] // 2}" corners="round"' if round_ else 'radius="26"'
    disc = ''
    title_color, hint_color = 'foreground', 'secondary'
    pad = '0 26' if round_ else '0 16'
    pkg(folder, 'Photo', 'Photos', size, f'''
  <!-- The photo picked in the menu; else one from the folder picked there, a new one every
       15 minutes (the app picks it); else how to pick one. -->
  <stack width="fill" height="fill">
    <image if="opt.photo" src="{{opt.photo}}" width="fill" height="fill" {r}/>
    <stack if="!opt.photo" width="fill" height="fill">
      <image if="app.photo" src="{{app.photo}}" width="fill" height="fill" {r}/>{disc}
      <column if="!app.photo" width="fill" align="center" gap="5" padding="{pad}">
        <stack width="42" height="42">
          <ellipse width="fill" height="fill" fill="accent"/>
          <image src="assets/photo.svg" width="22" height="22"/>
        </stack>
        <spacer height="3"/>
        <text value="{{str.Add a photo}}" size="15" weight="600" color="{title_color}" text-align="center" width="fill"/>
        <text value="{{str.Right-click, then Photo or Folder}}" size="12" color="{hint_color}" text-align="center" width="fill"/>
      </column>
    </stack>
  </stack>''', variant=variant, radius=size[0] // 2 if round_ else 26, keys=['Add a photo', 'Right-click, then Photo or Folder'], allow=('size', 'background', 'theme'),
        assets={'photo.svg': PROMPT_ICON}, options=PHOTO_OPTIONS)

# --- notes and tasks -------------------------------------------------------------------------
NOTES_ICON = svg('<rect x="3" y="3" width="18" height="18" rx="5" fill="#FFCC00"/><path d="M7.5 9h9M7.5 12.5h9M7.5 16h5.5" stroke="#FFFFFF" stroke-width="1.6" stroke-linecap="round"/>')
for folder, variant, size, text in [('notes-small', 'Small', (170, 170), 13), ('notes-medium', 'Medium', (352, 170), 14), ('notes-large', 'Large', (352, 352), 15)]:
    pkg(folder, 'Notes', 'Productivity', size, f'''
  <!-- The note is a text file the title opens in Notepad. -->
  <column padding="14 16 16 16" gap="8" width="fill" height="fill">
    <row gap="7" align="center" action="edit" scale="1" pressed.scale="0.95" transition="spring">
      <image src="assets/notes.svg" width="20" height="20"/>
      <text value="{{str.Notes}}" size="14" weight="700" color="#E3A600"/>
    </row>
    <text if="app.text" value="{{app.text}}" size="{text}" width="fill"/>
    <text if="!app.text" value="{{str.Click the title to write a note}}" size="13" color="secondary" width="fill"/>
  </column>''', variant=variant, keys=['Click the title to write a note'], assets={'notes.svg': NOTES_ICON})

CHECK = svg('<path d="M5.5 12.5l4 4 9-9.5" fill="none" stroke="#FFFFFF" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"/>')
for folder, variant, size, rows in [('tasks-small', 'Small', (170, 170), 4), ('tasks-large', 'Large', (352, 352), 9)]:
    items = '\n'.join(f'''    <row if="app.t{i}" gap="9" align="center" width="fill">
      <stack width="18" height="18" action="task:{i}" scale="1" pressed.scale="0.8" transition="spring">
        <ellipse if="!app.d{i}" width="fill" height="fill" fill="transparent" stroke="secondary" stroke-width="1.5"/>
        <ellipse if="app.d{i}" width="fill" height="fill" fill="accent"/>
        <image if="app.d{i}" src="assets/check.svg" width="12" height="12"/>
      </stack>
      <text if="!app.d{i}" value="{{app.t{i}}}" size="13" ellipsis="true" grow="1"/>
      <text if="app.d{i}" value="{{app.t{i}}}" size="13" color="secondary" ellipsis="true" grow="1"/>
    </row>''' for i in range(rows))
    pkg(folder, 'Tasks', 'Productivity', size, f'''
  <!-- Tasks are lines of a text file the title opens in Notepad; "[x] " marks done ones.
       A circle ticks its task off. -->
  <column padding="14 16" gap="8" width="fill" height="fill">
    <row width="fill" align="center" action="edit">
      <text value="{{str.Tasks}}" size="14" weight="700" color="accent"/>
      <spacer grow="1"/>
      <text if="app.t0" value="{{app.count}}" size="14" weight="700" color="accent"/>
    </row>
    <text if="!app.t0" value="{{str.Click the title to add tasks}}" size="13" color="secondary" width="fill"/>
{items}
  </column>''', variant=variant, keys=['Click the title to add tasks'], assets={'check.svg': CHECK})

# --- weather ---------------------------------------------------------------------------------
CLOUD = 'M7 19.5h10.4a4.2 4.2 0 0 0 .5-8.4 6 6 0 0 0-11.6 1.7A3.4 3.4 0 0 0 7 19.5z'
SMALL_CLOUD = 'M8.5 20h9a3.6 3.6 0 0 0 .4-7.2 5.2 5.2 0 0 0-10 1.5A2.9 2.9 0 0 0 8.5 20z'
GREY = '#B6C2CF'
rays = ''.join(f'<line x1="{12 + 7.2 * math.cos(a):.2f}" y1="{12 + 7.2 * math.sin(a):.2f}" x2="{12 + 9.6 * math.cos(a):.2f}" y2="{12 + 9.6 * math.sin(a):.2f}"/>'
               for a in [k * math.pi / 4 for k in range(8)])
WEATHER = {
    'sun.svg': svg(f'<circle cx="12" cy="12" r="4.8" fill="#FFCC00"/><g stroke="#FFCC00" stroke-width="2" stroke-linecap="round">{rays}</g>'),
    'moon.svg': svg('<path d="M15.2 3.4a8.6 8.6 0 1 0 5.4 13 7.2 7.2 0 0 1-5.4-13z" fill="#F7D774"/>'),
    'partly.svg': svg(f'<circle cx="9" cy="9" r="4.2" fill="#FFCC00"/><path d="{SMALL_CLOUD}" fill="{GREY}"/>'),
    'cloud.svg': svg(f'<path d="{CLOUD}" fill="{GREY}"/>'),
    'fog.svg': svg(f'<g stroke="{GREY}" stroke-width="2.2" stroke-linecap="round"><line x1="4" y1="8" x2="20" y2="8"/><line x1="6" y1="12.5" x2="18" y2="12.5"/><line x1="4" y1="17" x2="20" y2="17"/></g>'),
    'rain.svg': svg(f'<path d="M7 15.5h10.4a4.2 4.2 0 0 0 .5-8.4 6 6 0 0 0-11.6 1.7A3.4 3.4 0 0 0 7 15.5z" fill="{GREY}"/><g stroke="#4DA3FF" stroke-width="2" stroke-linecap="round"><line x1="8" y1="18" x2="7" y2="21"/><line x1="12" y1="18" x2="11" y2="21"/><line x1="16" y1="18" x2="15" y2="21"/></g>'),
    'snow.svg': svg(f'<path d="M7 15.5h10.4a4.2 4.2 0 0 0 .5-8.4 6 6 0 0 0-11.6 1.7A3.4 3.4 0 0 0 7 15.5z" fill="{GREY}"/><g fill="#8FCBFF"><circle cx="8" cy="19.5" r="1.4"/><circle cx="12" cy="20.5" r="1.4"/><circle cx="16" cy="19.5" r="1.4"/></g>'),
    'storm.svg': svg(f'<path d="M7 15.5h10.4a4.2 4.2 0 0 0 .5-8.4 6 6 0 0 0-11.6 1.7A3.4 3.4 0 0 0 7 15.5z" fill="{GREY}"/><path d="M12.6 13.5 9.4 18h2.8l-1 4.2 3.6-5.2H12z" fill="#FFCC00"/>'),
}
CONDS = [('clear', 'Clear'), ('partly', 'Partly cloudy'), ('cloud', 'Cloudy'), ('fog', 'Fog'), ('rain', 'Rain'), ('snow', 'Snow'), ('storm', 'Thunderstorm')]
def conds(size, align):
    return '\n'.join(f'        <text if="app.k_{k}" value="{{str.{label}}}" size="{size}" weight="500" ellipsis="true" text-align="{align}" width="fill"/>'
                     for k, label in CONDS)


cond_texts = conds(13, 'end')
WEATHER_CITIES = ['Los Angeles', 'Chicago', 'New York', 'Toronto', 'São Paulo', 'London', 'Amsterdam', 'Paris', 'Madrid', 'Rome',
                  'Berlin', 'Prague', 'Vienna', 'Warsaw', 'Lviv', 'Kyiv', 'Kharkiv', 'Odesa', 'Minsk', 'Istanbul', 'Saint Petersburg',
                  'Moscow', 'Kazan', 'Yekaterinburg', 'Novosibirsk', 'Almaty', 'Dubai', 'New Delhi', 'Bangkok', 'Singapore', 'Beijing',
                  'Seoul', 'Tokyo', 'Sydney']
REGIONS = {'Americas': ['Los Angeles', 'Chicago', 'New York', 'Toronto', 'São Paulo'],
           'Europe': ['London', 'Amsterdam', 'Paris', 'Madrid', 'Rome', 'Berlin', 'Prague', 'Vienna', 'Warsaw', 'Istanbul'],
           'Eastern Europe': ['Lviv', 'Kyiv', 'Kharkiv', 'Odesa', 'Minsk', 'Saint Petersburg', 'Moscow', 'Kazan'],
           'Asia & Pacific': ['Yekaterinburg', 'Novosibirsk', 'Almaty', 'Dubai', 'New Delhi', 'Bangkok', 'Singapore', 'Beijing',
                              'Seoul', 'Tokyo', 'Sydney']}
assert sorted(sum(REGIONS.values(), [])) == sorted(WEATHER_CITIES)
# "Europe/Paris": Paris in the menu's Europe submenu; the app looks the city up by its name.
WEATHER_OPTIONS = [('city', 'City', [('auto', 'Automatic')] + [(f'{r}/{c}', c) for r, cs in REGIONS.items() for c in cs], 'auto', None)]
days = '\n'.join(f'''      <column width="36" align="center" gap="2">
        <text value="{{app.w{i}}}" size="11" weight="500" color="secondary"/>
        <image if="app.i{i}" src="assets/{{app.i{i}}}.svg" width="20" height="20"/>
        <text value="{{app.h{i}}}" size="12" weight="600" features="tnum"/>
      </column>''' for i in range(1, 5))
pkg('weather', 'Weather', 'Weather', (352, 170), f'''
  <!-- From the app: the city picked in the menu (or the one of the IP address), forecast from
       Open-Meteo, every 30 minutes. -->
  <column padding="14 18 12 18" gap="6" width="fill" height="fill">
    <row width="fill" align="start" gap="10">
      <column grow="1">
        <text value="{{app.city}}" size="15" weight="600" ellipsis="true" width="fill"/>
        <text if="app.temp" value="{{app.temp}}" size="40" weight="300" features="tnum"/>
        <text if="!app.ready" value="{{str.Updating…}}" size="13" color="secondary"/>
        <text if="app.nonet" value="{{str.No connection}}" size="13" color="secondary"/>
      </column>
      <column width="150" align="end" gap="2">
        <image if="app.icon" src="assets/{{app.icon}}.svg" width="28" height="28"/>
{cond_texts}
        <text if="app.temp" value="↑{{app.hi}}  ↓{{app.lo}}" size="12" color="secondary" features="tnum"/>
        <text if="app.stale" value="{{str.Offline}}" size="11" color="secondary"/>
      </column>
    </row>
    <spacer grow="1"/>
    <row if="app.temp" width="fill" justify="space-between">
{days}
    </row>
  </column>''', variant='Wide', keys=['Updating…', 'No connection', 'Offline'] + [label for _, label in CONDS], assets=WEATHER, options=WEATHER_OPTIONS)
pkg('weather-square', 'Weather', 'Weather', (170, 170), f'''
  <column padding="14 16 14 16" width="fill" height="fill">
    <text value="{{app.city}}" size="14" weight="600" ellipsis="true" width="fill"/>
    <text if="app.temp" value="{{app.temp}}" size="42" weight="300" features="tnum"/>
    <text if="!app.ready" value="{{str.Updating…}}" size="13" color="secondary"/>
    <text if="app.nonet" value="{{str.No connection}}" size="13" color="secondary"/>
    <spacer grow="1"/>
    <image if="app.icon" src="assets/{{app.icon}}.svg" width="26" height="26"/>
{conds(12, "start")}
    <row if="app.temp" gap="4">
      <text value="↑{{app.hi}}  ↓{{app.lo}}" size="12" color="secondary" features="tnum"/>
      <text if="app.stale" value="· {{str.Offline}}" size="12" color="secondary"/>
    </row>
  </column>''', variant='Square', keys=['Updating…', 'No connection', 'Offline'] + [label for _, label in CONDS], assets=WEATHER, options=WEATHER_OPTIONS)

# --- clocks ----------------------------------------------------------------------------------
DISPLAY = ('Nunito', 400)
SHADOW = 'shadow="0 1 8 #00000070"'
BARE = ('size', 'theme', 'font', 'colors')
ROUNDED = ('Nunito', 900)
# Image 1: dark digits with a light rim, drawn as hard shadows in eight directions.
RIM = ', '.join(f'{x} {y} 0 #B4B4B8' for x, y in [(1.6, 0), (-1.6, 0), (0, 1.6), (0, -1.6), (1.1, 1.1), (-1.1, 1.1), (1.1, -1.1), (-1.1, -1.1)])
pkg('clock-outline', 'Digital Clock', 'Time', (352, 170), f'''
  <!-- No card: dark digits with a light rim sit on the wallpaper. -->
  <column width="fill" height="fill" align="center" justify="center">
    <text fit="true" value="{{clock.time}}" size="118" weight="800" features="tnum" color="#3A3A3C" shadow="0 3 12 #00000050, {RIM}"/>
  </column>''', variant='Outline', card=False, font=ROUNDED, allow=('size', 'font'))
pkg('clock-bold', 'Digital Clock', 'Time', (352, 170), '''
  <column width="fill" height="fill" align="center" justify="center">
    <text fit="true" value="{clock.time}" size="120" weight="900" features="tnum" shadow="0 2 14 #00000050"/>
  </column>''', variant='Bold', card=False, font=ROUNDED, allow=('size', 'theme', 'font', 'text'))
pkg('clock-bold-stacked', 'Digital Clock', 'Time', (170, 170), '''
  <column width="fill" height="fill" align="center" justify="center">
    <text fit="true" value="{clock.hour}" size="62" weight="900" features="tnum" shadow="0 2 12 #00000050"/>
    <text fit="true" value="{clock.minute}" size="62" weight="900" features="tnum" shadow="0 2 12 #00000050"/>
  </column>''', variant='Bold, stacked', card=False, font=ROUNDED, allow=('size', 'theme', 'font', 'text'))

# Image 2: a light card, bold time over the weekday and date.
pkg('clock-digital-small', 'Digital Clock', 'Time', (170, 170), '''
  <column padding="20 20 18 20" width="fill" height="fill">
    <text fit="true" value="{clock.time}" size="44" weight="800" features="tnum" tracking="-1"/>
    <spacer grow="1"/>
    <text value="{clock.weekday}," size="17" weight="600" color="secondary" ellipsis="true" width="fill"/>
    <text value="{clock.month_day}" size="17" weight="600" color="secondary" ellipsis="true" width="fill"/>
  </column>''', variant='Small', font=('Nunito', 700), background='solid')
pkg('clock-digital-wide', 'Digital Clock', 'Time', (352, 170), '''
  <row padding="20 24" width="fill" height="fill" align="center" gap="18">
    <text fit="true" value="{clock.time}" size="70" weight="300" features="tnum" tracking="-2" grow="1"/>
    <column align="end">
      <text value="{clock.weekday}" size="15" weight="600" color="accent"/>
      <text value="{clock.day}" size="40" weight="700"/>
      <text value="{clock.month}" size="14" color="secondary"/>
    </column>
  </row>''', variant='Wide', font=DISPLAY)
pkg('clock-digital-bold', 'Digital Clock', 'Time', (170, 170), '''
  <column width="fill" height="fill" align="center" justify="center">
    <text fit="true" value="{clock.hour}" size="60" weight="800" features="tnum" tracking="-2"/>
    <text fit="true" value="{clock.minute}" size="60" weight="800" features="tnum" tracking="-2" color="accent"/>
  </column>''', variant='Big digits', font=DISPLAY)


def polar(r, deg):
    a = math.radians(deg - 90)
    return 50 + r * math.cos(a), 50 + r * math.sin(a)


def ticks(r1, r2, every, width, opacity, skip=None):
    out = []
    for k in range(0, 360, every):
        if skip and k % skip == 0:
            continue
        (x1, y1), (x2, y2) = polar(r1, k), polar(r2, k)
        out.append(f'<line x1="{x1:.2f}" y1="{y1:.2f}" x2="{x2:.2f}" y2="{y2:.2f}" stroke-width="{width}" stroke-opacity="{opacity}"/>')
    return '<g stroke="{foreground}" stroke-linecap="round">' + ''.join(out) + '</g>'


def hand(y2, width, color, tail=50, cap=0):
    c = f'<circle cx="50" cy="50" r="{cap}" fill="{color}"/>' if cap else ''
    return svg(f'<line x1="50" y1="{tail}" x2="50" y2="{y2}" stroke="{color}" stroke-width="{width}" stroke-linecap="round"/>{c}', 100)


def analog(folder, variant, dial, hands, numerals=False):
    nums = '''
      <column width="fill" height="fill" padding="7" align="center">
        <text value="12" size="17" weight="600"/>
        <spacer grow="1"/>
        <text value="6" size="17" weight="600"/>
      </column>
      <row width="fill" height="fill" padding="0 11" align="center">
        <text value="9" size="17" weight="600"/>
        <spacer grow="1"/>
        <text value="3" size="17" weight="600"/>
      </row>''' if numerals else ''
    pkg(folder, 'Dial', 'Time', (170, 170), f'''
  <!-- Hands are full-box overlays that turn around the dial center. -->
  <style class="hand" shadow="0 0 3 #00000040"/>
  <stack width="fill" height="fill">
    <stack width="146" height="146">
      <image src="assets/dial.svg" width="fill" height="fill"/>{nums}
      <image class="hand" src="assets/hand-hour.svg" width="fill" height="fill" rotate="{{clock.hour_angle}}" spin="0.008333"/>
      <image class="hand" src="assets/hand-minute.svg" width="fill" height="fill" rotate="{{clock.minute_angle}}" spin="0.1"/>
      <image class="hand" src="assets/hand-second.svg" width="fill" height="fill" rotate="{{clock.second_angle}}" spin="6"/>
    </stack>
  </stack>''', variant=variant, font=DISPLAY, assets={'dial.svg': dial, **hands})


analog('clock-analog-classic', 'Classic',
       svg('<circle cx="50" cy="50" r="49" fill="{foreground}" fill-opacity="0.05"/>' + ticks(46, 43.5, 6, 0.9, 0.45, skip=30) + ticks(46, 39, 30, 2.4, 0.9), 100),
       {'hand-hour.svg': hand(26, 4.6, '{foreground}', tail=55),
        'hand-minute.svg': hand(11, 3.2, '{foreground}', tail=56, cap=3.4),
        'hand-second.svg': hand(8, 1.2, '#FF9500', tail=61, cap=2.2)})
analog('clock-analog-minimal', 'Minimal',
       svg(''.join('<circle cx="{:.2f}" cy="{:.2f}" r="{}" fill="{{foreground}}" fill-opacity="{}"/>'.format(*polar(43, k), 2.4 if k % 90 == 0 else 1.5, 0.85 if k % 90 == 0 else 0.45) for k in range(0, 360, 30)), 100),
       {'hand-hour.svg': hand(29, 3.4, '{foreground}'),
        'hand-minute.svg': hand(13, 2.2, '{foreground}', cap=2.6),
        'hand-second.svg': hand(9, 1.0, '{accent}', tail=58, cap=1.8)})
analog('clock-analog-modern', 'Modern',
       svg('<circle cx="50" cy="50" r="49" fill="{foreground}" fill-opacity="0.07"/>' + ticks(45, 43, 6, 0.8, 0.3, skip=90), 100),
       {'hand-hour.svg': hand(30, 6, '{foreground}'),
        'hand-minute.svg': hand(15, 4.2, '{foreground}'),
        'hand-second.svg': svg('<line x1="50" y1="60" x2="50" y2="12" stroke="{accent}" stroke-width="1.6" stroke-linecap="round"/>'
                               '<circle cx="50" cy="50" r="4" fill="{accent}"/><circle cx="50" cy="50" r="1.6" fill="#FFFFFF"/>', 100)},
       numerals=True)

# Digital and analog together, square.
CLOCK = os.path.join(ROOT, 'clock', 'assets')
pkg('clock-dual', 'Clock', 'Time', (170, 170), '''
  <style class="hand" shadow="0 0 3 #00000059"/>
  <column padding="12 14 12 14" width="fill" height="fill" align="center" gap="2">
    <stack width="84" height="84">
      <image src="assets/dial.svg" width="fill" height="fill"/>
      <image class="hand" src="assets/hand-hour.svg" width="fill" height="fill" rotate="{clock.hour_angle}" spin="0.008333"/>
      <image class="hand" src="assets/hand-minute.svg" width="fill" height="fill" rotate="{clock.minute_angle}" spin="0.1"/>
      <image class="hand" src="assets/hand-second.svg" width="fill" height="fill" rotate="{clock.second_angle}" spin="6"/>
    </stack>
    <spacer grow="1"/>
    <text fit="true" value="{clock.time}" size="24" weight="600" features="tnum"/>
    <text value="{clock.weekday}" size="12" color="secondary"/>
  </column>''', variant='Square', font=('Nunito', 600),
    assets={n: open(os.path.join(CLOCK, n), encoding='utf-8').read() for n in ['dial.svg', 'hand-hour.svg', 'hand-minute.svg', 'hand-second.svg']})
CITIES = ['Los Angeles', 'New York', 'São Paulo', 'London', 'Paris', 'Berlin', 'Kyiv', 'Istanbul', 'Moscow', 'Dubai', 'New Delhi', 'Bangkok', 'Beijing', 'Tokyo', 'Sydney']
world_cols = '\n'.join(f'''    <column grow="1" align="center" gap="1">
      <stack width="66" height="66">
        <image src="assets/dial.svg" width="fill" height="fill"/>
        <image class="hand" src="assets/hand-hour.svg" width="fill" height="fill" rotate="{{app.a{i}}}"/>
        <image class="hand" src="assets/hand-minute.svg" width="fill" height="fill" rotate="{{app.m{i}}}"/>
        <image class="hand" src="assets/hand-second.svg" width="fill" height="fill" rotate="{{clock.second_angle}}" spin="6"/>
      </stack>
      <spacer height="5"/>
      <text value="{{app.n{i}}}" size="12" weight="600" ellipsis="true" text-align="center" width="fill"/>
      <text value="{{app.d{i}}}" size="11" color="secondary" ellipsis="true" text-align="center" width="fill"/>
      <text value="{{app.o{i}}}" size="11" color="secondary" features="tnum"/>
    </column>''' for i in range(4))
pkg('clock-world', 'World Clock', 'Time', (352, 170), f'''
  <!-- The app sets the hands (app.a0, app.m0), city (app.n0), day (app.d0) and offset (app.o0). -->
  <style class="hand" shadow="0 0 2 #00000050"/>
  <row padding="16 10 12 10" width="fill" height="fill">
{world_cols}
  </row>''', font=('Nunito', 400), background='solid',
    options=[(f'city{i}', f'City {i + 1}', [(c, c) for c in CITIES], d, None)
             for i, d in enumerate(['New Delhi', 'Dubai', 'London', 'Paris'])],
    assets={'dial.svg': svg('<circle cx="50" cy="50" r="49" fill="{foreground}" fill-opacity="0.08"/>' + ticks(45, 38, 30, 3, 0.75), 100),
            'hand-hour.svg': hand(27, 6, '{foreground}'),
            'hand-minute.svg': hand(12, 4.4, '{foreground}', cap=4.5),
            'hand-second.svg': hand(10, 1.6, '#FF9500', tail=60, cap=2.4)})

# --- timers ----------------------------------------------------------------------------------
BTN = {
    'play': svg('<path d="M8 5.2v13.6a1 1 0 0 0 1.5.86l11-6.8a1 1 0 0 0 0-1.72l-11-6.8A1 1 0 0 0 8 5.2z" fill="{foreground}"/>'),
    'pause': svg('<rect x="6" y="4.5" width="4.2" height="15" rx="1.4" fill="{foreground}"/><rect x="13.8" y="4.5" width="4.2" height="15" rx="1.4" fill="{foreground}"/>'),
    'reset': svg('<path d="M5 12a7 7 0 1 0 2.1-5" fill="none" stroke="{foreground}" stroke-width="2.2" stroke-linecap="round"/><path d="M6.4 2.8v4.6H11" fill="none" stroke="{foreground}" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/>'),
    'skip': svg('<path d="M5 5.6v12.8a.9.9 0 0 0 1.4.75L15.5 13a1.2 1.2 0 0 0 0-2L6.4 4.85A.9.9 0 0 0 5 5.6z" fill="{foreground}"/><rect x="16.8" y="4.5" width="3" height="15" rx="1.2" fill="{foreground}"/>'),
}
BTN = {f'{k}.svg': v for k, v in BTN.items()}
# The arc: app.dash is the part left times the circumference (r = 42: 263.9).
TIMER_RING = '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <circle cx="50" cy="50" r="42" fill="none" stroke="{foreground}" stroke-opacity="0.12" stroke-width="7"/>
  <circle cx="50" cy="50" r="42" fill="none" stroke="{app.color}" stroke-width="7" stroke-linecap="round"
          stroke-dasharray="{app.dash} 400" transform="rotate(-90 50 50)"/>
</svg>'''


def button(action, icon, d=34, i=16):
    if icon == 'toggle':
        img = (f'<image if="app.running" src="assets/pause.svg" width="{i}" height="{i}"/>\n'
               f'        <image if="!app.running" src="assets/play.svg" width="{i}" height="{i}"/>')
    else:
        img = f'<image src="assets/{icon}.svg" width="{i}" height="{i}"/>'
    return f'''<stack width="{d}" height="{d}" action="timer:{action}" scale="1" pressed.scale="0.86" transition="spring">
        <ellipse width="fill" height="fill" fill="#80808030"/>
        {img}
      </stack>'''


def ring_widget(folder, name, category, second, label, options):
    pkg(folder, name, category, (170, 170), f'''
  <!-- The app counts down (on its UI thread) and sets app.time, app.dash and app.running. -->
  <column padding="12" gap="8" width="fill" height="fill" align="center">
    <stack width="102" height="102">
      <image src="assets/ring.svg" width="fill" height="fill"/>
      <column width="74" align="center">
        <text fit="true" value="{{app.time}}" size="24" weight="700" features="tnum"/>{label}
      </column>
    </stack>
    <row gap="16">
      {button('toggle', 'toggle')}
      {button(second, second)}
    </row>
  </column>''', keys=['Focus', 'Break'] if label else (), assets={'ring.svg': TIMER_RING, **BTN}, options=options,
        font=('Nunito', 600))


def mins(ns):
    return [(str(n), f'{n} min') for n in ns]


ring_widget('timer', 'Timer', 'Time', 'reset', '', [('minutes', 'Duration', mins([1, 2, 3, 5, 10, 15, 20, 30, 45, 60, 90]), '5', None)])
ring_widget('pomodoro', 'Pomodoro', 'Productivity', 'skip', '''
        <text if="app.focus" value="{str.Focus}" size="11" weight="600" color="secondary"/>
        <text if="!app.focus" value="{str.Break}" size="11" weight="600" color="secondary"/>''',
            [('focus', 'Focus length', mins([15, 20, 25, 30, 45, 50, 60, 90]), '25', None),
             ('break', 'Break length', mins([3, 5, 10, 15, 20, 30]), '5', None)])
pkg('stopwatch', 'Stopwatch', 'Time', (170, 170), f'''
  <column padding="16 16 14 16" width="fill" height="fill">
    <text value="{{str.Stopwatch}}" size="13" weight="700" color="#FF9500"/>
    <spacer grow="1"/>
    <text value="{{app.time}}" size="34" weight="600" features="tnum"/>
    <spacer grow="1"/>
    <row gap="14">
      {button('toggle', 'toggle', 38, 17)}
      {button('reset', 'reset', 38, 17)}
    </row>
  </column>''', assets=BTN, font=('Nunito', 600))

# --- screen time -----------------------------------------------------------------------------
# 24 bars, one an hour; app.b0.. are heights (minutes * 56 / 60), app.y0.. their tops (56 - height).
BW, BH, SLOT = 316, 56, 316 / 24
bars = ''.join(f'<rect x="{i * SLOT + 2.3:.2f}" y="{{app.y{i}}}" width="8.5" height="{{app.b{i}}}" rx="2.5" fill="{{accent}}"/>'
               f'<rect x="{i * SLOT + 2.3:.2f}" y="{BH - 2}" width="8.5" height="2" rx="1" fill="{{foreground}}" fill-opacity="0.15"/>'
               for i in range(24))
hours = '\n'.join(f'      <text value="{h}" size="10" color="secondary" width="79"/>' for h in (0, 6, 12, 18))
pkg('screen-time', 'Screen time', 'System', (352, 170), f'''
  <!-- Time this PC was in use today (input in the last 5 minutes), counted by the app. -->
  <column padding="14 18 12 18" gap="6" width="fill" height="fill">
    <row width="fill" align="end">
      <column grow="1">
        <text value="{{str.Screen time}}" size="13" weight="600" color="secondary"/>
        <text if="app.hours" value="{{app.hours}} {{str.h}} {{app.mins}} {{str.min}}" size="28" weight="700" features="tnum"/>
        <text if="!app.hours" value="{{app.mins}} {{str.min}}" size="28" weight="700" features="tnum"/>
      </column>
      <text value="{{str.Today}}" size="12" color="secondary"/>
    </row>
    <image src="assets/bars.svg" width="{BW}" height="{BH}"/>
    <row width="fill">
{hours}
    </row>
  </column>''', keys=['h', 'min', 'Today'], assets={'bars.svg': f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {BW} {BH}">{bars}</svg>'})

# Same bar heights (56 units an hour), squeezed into 138 x 44 by the group transform.
SQ = 138 / 24
small_bars = ''.join(f'<rect x="{i * SQ + 1.1:.2f}" y="{{app.y{i}}}" width="3.6" height="{{app.b{i}}}" rx="1.6" fill="{{accent}}"/>'
                     f'<rect x="{i * SQ + 1.1:.2f}" y="{BH - 2}" width="3.6" height="2" rx="1" fill="{{foreground}}" fill-opacity="0.15"/>'
                     for i in range(24))
small_hours = '\n'.join(f'      <text value="{h}" size="9" color="secondary" width="34.5"/>' for h in (0, 6, 12, 18))
pkg('screen-time-square', 'Screen time', 'System', (170, 170), f'''
  <column padding="14 16 12 16" gap="4" width="fill" height="fill">
    <text value="{{str.Screen time}}" size="12" weight="600" color="secondary" ellipsis="true" width="fill"/>
    <text if="app.hours" fit="true" value="{{app.hours}} {{str.h}} {{app.mins}} {{str.min}}" size="24" weight="700" features="tnum"/>
    <text if="!app.hours" fit="true" value="{{app.mins}} {{str.min}}" size="24" weight="700" features="tnum"/>
    <spacer grow="1"/>
    <image src="assets/bars.svg" width="138" height="44"/>
    <row width="fill">
{small_hours}
    </row>
  </column>''', variant='Square', keys=['h', 'min'],
    assets={'bars.svg': f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 138 44"><g transform="scale(1 {44 / 56:.4f})">{small_bars}</g></svg>'})

# --- now playing, square ------------------------------------------------------------------------
MEDIA = os.path.join(ROOT, 'media', 'assets')
pkg('media-square', 'Now Playing', 'Media', (170, 170), '''
  <style class="button" padding="5" radius="18" corners="round" fill="transparent" hover.fill="#8080802E"
         scale="1" pressed.scale="0.86" transition="spring"/>
  <column padding="14 14 8 14" gap="3" width="fill" height="fill">
    <stack width="50" height="50" shadow="0 2 8 #00000030">
      <rect if="!app.art" width="fill" height="fill" radius="9" fill="linear(160deg, #FFFFFF38, #FFFFFF1A)"/>
      <image if="!app.art" src="assets/note.svg" width="24" height="24" opacity="0.7"/>
      <image if="app.art" src="{app.art}" width="50" height="50" radius="9"/>
    </stack>
    <spacer grow="1"/>
    <text if="app.source" value="{app.title}" size="14" weight="600" ellipsis="true" width="fill"/>
    <text if="!app.source" value="{app.title}" size="14" weight="500" color="secondary" ellipsis="true" width="fill"/>
    <text value="{app.artist}" size="12" color="secondary" ellipsis="true" width="fill"/>
    <spacer height="1"/>
    <image drag="seek" src="assets/progress.svg" hover.src="assets/progress-hover.svg" pressed.src="assets/progress-drag.svg" width="fill" height="10" opacity="{app.controls}"/>
    <row justify="center" align="center" gap="6" width="fill" opacity="{app.controls}">
      <stack class="button" action="previous">
        <image src="assets/backward.svg" width="20" height="20"/>
      </stack>
      <stack class="button" action="playpause">
        <image if="app.playing" src="assets/pause.svg" width="24" height="24"/>
        <image if="!app.playing" src="assets/play.svg" width="24" height="24"/>
      </stack>
      <stack class="button" action="next">
        <image src="assets/forward.svg" width="20" height="20"/>
      </stack>
    </row>
  </column>''', variant='Square', font=('Nunito', 400), background='glass', allow=('size', 'background', 'theme', 'shape', 'font'),
    assets={n: open(os.path.join(MEDIA, n), encoding='utf-8').read() for n in os.listdir(MEDIA)})

# --- the hand-written packages: wide size, fonts, all languages ---------------------------------
for folder, keys in [('clock', ['Clock', 'Wide', 'Dial', 'Hour and minute hands', 'Second hand', 'Date']),
                     ('calendar', ['Calendar']), ('media', ['Now Playing', 'Wide'])]:
    p = os.path.join(ROOT, folder, 'widget.toml')
    text = open(p, encoding='utf-8').read()
    text = text[:text.index('[strings.')].rstrip() + '\n'
    if folder == 'media' and 'variant =' not in text:
        text = text.replace('name = "Now Playing"\n', 'name = "Now Playing"\nvariant = "Wide"\n')
    text = text.replace('size = [340, 160]', 'size = [352, 170]').replace('size = [340, 184]', 'size = [352, 184]')
    text = text.replace('allow = ["size", "background", "theme"]', 'allow = ["size", "background", "theme", "font"]')
    text = text.replace('allow = ["size", "background", "theme", "shape"]', 'allow = ["size", "background", "theme", "shape", "font"]')
    for col, lang in enumerate(LANGS):
        text += f'\n[strings.{lang}]\n' + ''.join(f'{toml_str(k)} = {toml_str(T[k][col])}\n' for k in keys)
    open(p, 'w', encoding='utf-8', newline='\n').write(text)

# The wide clock: no hover zoom on the dial; without the dial it is a square.
p = os.path.join(ROOT, 'clock', 'layout.xml')
text = open(p, encoding='utf-8').read().replace(' scale="1" hover.scale="1.06" transition="spring"', '')
open(p, 'w', encoding='utf-8', newline='\n').write(text)
p = os.path.join(ROOT, 'clock', 'widget.toml')
text = open(p, encoding='utf-8').read()
if 'sizes' not in text:
    text = text.replace('key = "dial"\nlabel = "Dial"\n', 'key = "dial"\nlabel = "Dial"\nsizes = { off = [170, 170] }\n')
    open(p, 'w', encoding='utf-8', newline='\n').write(text)

# The clock's digits stay on one line in any font too.
p = os.path.join(ROOT, 'clock', 'layout.xml')
text = open(p, encoding='utf-8').read()
if 'fit=' not in text:
    open(p, 'w', encoding='utf-8', newline='\n').write(text.replace('<text value="{clock.time}" size="56"', '<text fit="true" value="{clock.time}" size="56"'))

# --- system monitor, device battery, exchange rates, events, smart stack ---------------------------
def bar(key):
    return ('<svg xmlns="http://www.w3.org/2000/svg" width="100%" height="100%">'
            '<rect width="100%" height="100%" rx="3" fill="{foreground}" fill-opacity="0.14"/>'
            f'<rect width="{{app.{key}}}%" height="100%" rx="3" fill="{{accent}}"/></svg>')


def meter(key, label, indent='    '):
    lines = [f'<column if="app.{key}_t" gap="3" width="fill">',
             '  <row width="fill">',
             f'    <text value="{{str.{label}}}" size="12" weight="500" ellipsis="true" grow="1"/>',
             f'    <text value="{{app.{key}_t}}" size="12" weight="700" features="tnum"/>',
             '  </row>',
             f'  <image src="assets/{key}.svg" width="fill" height="6"/>',
             '</column>']
    return '\n'.join(indent + l for l in lines)


METERS = [('cpu', 'Processor'), ('ram', 'Memory'), ('gpu', 'Graphics')]
pkg('system-monitor', 'System monitor', 'System', (170, 170), '''
  <!-- Load of this PC, read by the app every two seconds. -->
  <column padding="14 16 16 16" gap="7" width="fill" height="fill">
    <text value="{str.System monitor}" size="13" weight="600" color="secondary" ellipsis="true" width="fill"/>
    <spacer grow="1"/>
''' + '\n'.join(meter(k, lb) for k, lb in METERS) + '''
  </column>''', keys=[lb for _, lb in METERS], allow=('size', 'background', 'theme', 'font', 'colors'),
    assets={f'{k}.svg': bar(k) for k, _ in METERS})

devices = '\n'.join(f'''    <column if="app.n{i}" gap="3" width="fill">
      <row width="fill" gap="6">
        <text value="{{app.n{i}}}" size="12" weight="500" ellipsis="true" grow="1"/>
        <text value="{{app.p{i}}}%" size="12" weight="700" features="tnum"/>
      </row>
      <image src="assets/b{i}.svg" width="fill" height="6"/>
    </column>''' for i in range(3))
pkg('bluetooth-battery', 'Device battery', 'System', (170, 170), f'''
  <!-- Battery levels Bluetooth devices report (headphones, mice, keyboards), every minute. -->
  <column padding="14 16 16 16" gap="7" width="fill" height="fill">
    <text value="{{str.Device battery}}" size="13" weight="600" color="secondary" ellipsis="true" width="fill"/>
    <spacer grow="1"/>
    <text if="!app.n0" value="{{str.No Bluetooth devices with a battery}}" size="12" color="secondary" width="fill"/>
{devices}
  </column>''', keys=['No Bluetooth devices with a battery'], allow=('size', 'background', 'theme', 'font', 'colors'),
    assets={f'b{i}.svg': bar(f'b{i}') for i in range(3)})

CURRENCIES = ['USD', 'EUR', 'GBP', 'JPY', 'CNY', 'CHF', 'PLN', 'CAD']
rates = '\n'.join(f'''    <row if="app.c{i}" width="fill">
      <text value="{{app.c{i}}}" size="13" weight="600" color="secondary" grow="1"/>
      <text value="{{app.r{i}}}" size="13" weight="700" features="tnum"/>
    </row>''' for i in range(4))
pkg('currency', 'Exchange rates', 'Productivity', (170, 170), f'''
  <!-- European Central Bank rates (frankfurter.dev) for one unit of the base currency, hourly. -->
  <column padding="14 16 14 16" gap="5" width="fill" height="fill">
    <row width="fill">
      <text value="{{str.Exchange rates}}" size="13" weight="600" color="secondary" ellipsis="true" grow="1"/>
      <text value="1 {{app.base}}" size="13" weight="700" color="accent"/>
    </row>
    <spacer grow="1"/>
    <text if="!app.c0" value="{{str.Updating…}}" size="12" color="secondary"/>
{rates}
  </column>''', keys=['Updating…'], allow=('size', 'background', 'theme', 'font', 'colors'),
    options=[('base', 'Base currency', [(c, c) for c in CURRENCIES], 'USD', None)])

events = '\n'.join(f'''    <row if="app.e{i}" gap="10" width="fill" align="center">
      <column width="86">
        <text value="{{app.d{i}}}" size="11" weight="600" color="accent" ellipsis="true" width="fill"/>
        <text if="app.h{i}" value="{{app.h{i}}}" size="11" color="secondary" features="tnum"/>
        <text if="!app.h{i}" value="{{str.All day}}" size="11" color="secondary"/>
      </column>
      <text value="{{app.e{i}}}" size="13" weight="500" ellipsis="true" grow="1"/>
    </row>''' for i in range(3))
pkg('events', 'Events', 'Productivity', (352, 170), f'''
  <!-- Upcoming events of the iCalendar links in a text file the title opens in Notepad (for
       example the secret address of a Google or Outlook calendar), every 30 minutes. -->
  <column padding="14 18" gap="8" width="fill" height="fill">
    <row width="fill" align="center" action="edit">
      <text value="{{str.Events}}" size="14" weight="700" color="accent"/>
    </row>
    <text if="!app.linked" value="{{str.Click the title to add a calendar link}}" size="13" color="secondary" width="fill"/>
    <text if="app.empty" value="{{str.No upcoming events}}" size="13" color="secondary" width="fill"/>
{events}
  </column>''', keys=['Click the title to add a calendar link', 'No upcoming events', 'All day'])

dots = '\n'.join(f'''      <ellipse if="app.p{i}" width="6" height="6" fill="accent"/>
      <ellipse if="!app.p{i}" width="6" height="6" fill="#8080805A"/>''' for i in range(3))
stack_meters = '\n'.join(meter(k, lb, '      ') for k, lb in METERS[:2])
pkg('smart-stack', 'Smart stack', 'Time', (170, 170), f'''
  <!-- Clock, weather and system load in turns, every 15 seconds; a click shows the next. -->
  <column padding="14 16 10 16" gap="4" width="fill" height="fill" action="page">
    <column if="app.p0" grow="1" width="fill" justify="center" gap="2">
      <text fit="true" value="{{clock.time}}" size="44" weight="700" features="tnum"/>
      <text value="{{clock.date}}" size="13" color="secondary" ellipsis="true" width="fill"/>
    </column>
    <column if="app.p1" grow="1" width="fill" gap="2">
      <text value="{{app.city}}" size="13" weight="600" ellipsis="true" width="fill"/>
      <text if="app.temp" value="{{app.temp}}" size="38" weight="300" features="tnum"/>
      <text if="!app.temp" value="{{str.Updating…}}" size="12" color="secondary"/>
      <spacer grow="1"/>
      <row gap="6" align="center">
        <image if="app.icon" src="assets/{{app.icon}}.svg" width="22" height="22"/>
        <text if="app.temp" value="↑{{app.hi}}  ↓{{app.lo}}" size="12" color="secondary" features="tnum"/>
      </row>
    </column>
    <column if="app.p2" grow="1" width="fill" gap="7" justify="center">
{stack_meters}
    </column>
    <row gap="5" justify="center" width="fill">
{dots}
    </row>
  </column>''', keys=['Updating…'] + [lb for _, lb in METERS[:2]],
    assets={**WEATHER, **{f'{k}.svg': bar(k) for k, _ in METERS[:2]}},
    options=[('rotate', 'Rotate automatically', None, 'on', None)])

import shutil
for gone in ['clock-clear', 'clock-clear-stacked', 'clock-clear-weekday']:
    shutil.rmtree(os.path.join(ROOT, gone), ignore_errors=True)
print('ok')
