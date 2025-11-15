# eve-l-preview

Rust reimplementation of my Python [EVE-L-Preview](https://github.com/h0lylag/Py-EVE-L_Preview) project. This is basically a Linux clone of the EVE-O Preview tool that Windows players use to manage multiple EVE clients.

Big thanks to [ilveth](https://github.com/ilveth/eve-l-preview) for figuring out the X11 window thumbnail rendering that I was stuck on. Their implementation got me unstuck and made this project possible.

## What it does

Creates small preview windows for each EVE client you have running. Each preview shows what's happening in the actual game window in real-time. Click a preview to focus that client, drag them around to organize your screen, and use Tab/Shift+Tab to quickly cycle between characters.

Works great on Wayland as long as you're running EVE through Wine/Proton in XWayland mode (which is the default).

## Features

- Real-time thumbnails of each EVE client window
- Click a preview to switch to that client
- Drag previews around to organize them however you want
- Tab/Shift+Tab hotkeys for quick character switching
- Shows character names on each preview
- Remembers where you put each character's preview
- Optional edge/corner snapping when dragging
- Hide all previews when you alt-tab out of EVE
- Uses almost no RAM or CPU
- Configure everything via a TOML file or environment variables

## Configuration

The config file lives at `~/.config/eve-l-preview/eve-l-preview.toml` and gets created automatically the first time you run the program. You can edit it to change colors, sizes, positions, whatever. Environment variables will override the TOML settings if you want to test something quickly.

### Example config

```toml
# How see-through the previews are (0-100)
opacity_percent = 75

# Border around the active client's preview
border_size = 3
border_color = "#7FFF0000"  # Red with 50% transparency

# Character name positioning and color
text_x = 10
text_y = 10
text_color = "#FFFFFFFF"  # White
text_size = 18

# Hide previews when you're not in an EVE window
hide_when_no_focus = false

# Snap previews to edges when dragging (pixels)
snap_threshold = 15

# Only allow Tab cycling when an EVE window is focused
hotkey_require_eve_focus = true

# Tab/Shift+Tab cycling order
# Put your character names here in the order you want to cycle through them
hotkey_order = ["Main", "Hauler Alt", "Scout"]

# Per-character settings (these get saved automatically when you drag previews around)
[characters."Main"]
x = 100
y = 200
width = 480
height = 270

[characters."Hauler Alt"]
x = 600
y = 200
width = 480
height = 270
```

### Config options

| Setting | Type | Default | What it does |
|-----------|------|----------|-------------|
| `opacity_percent` | 0-100 | 80 | How transparent the previews are |
| `border_size` | number | 5 | Border width in pixels around focused preview |
| `border_color` | hex | `#7FFF0000` | Border color (AARRGGBB format) |
| `text_x` | number | 10 | Where to draw character name (horizontal) |
| `text_y` | number | 10 | Where to draw character name (vertical) |
| `text_color` | hex | `#FFFFFFFF` | Character name text color |
| `text_size` | number | 18 | Character name font size |
| `hide_when_no_focus` | true/false | false | Hide previews when no EVE window is focused |
| `snap_threshold` | number | 15 | How close to snap to edges (0 = off) |
| `hotkey_require_eve_focus` | true/false | true | Only allow Tab cycling when EVE is focused |
| `hotkey_order` | list | `[]` | Order of characters for Tab cycling |

The per-character settings (`x`, `y`, `width`, `height`) get saved automatically when you drag previews around or when you first log into a character.

### Environment variable overrides

You can override settings temporarily with environment variables:

```bash
OPACITY=0xC0000000 BORDER_COLOR=0xFF00FF00 eve-l-preview
```

Supported variables: `OPACITY`, `BORDER_SIZE`, `BORDER_COLOR`, `TEXT_X`, `TEXT_Y`, `TEXT_COLOR`, `HIDE_WHEN_NO_FOCUS`

Colors can be hex (`0xAARRGGBB` or `#AARRGGBB`) or decimal.

## Usage

Just run it. Doesn't matter if you start it before or after launching your EVE clients - it'll pick them up either way.

### Hotkeys

The Tab/Shift+Tab cycling requires you to be in the `input` group:

```bash
sudo usermod -a -G input $USER
```

Then log out and back in. After that:
- **Tab** - Next character
- **Shift+Tab** - Previous character  
- **Left-click a preview** - Focus that client
- **Right-click and drag** - Move the preview around

By default (`hotkey_require_eve_focus = true`), Tab cycling only works when you're focused on an EVE window. This prevents accidentally cycling when you're tabbed out to a browser or whatever. Set it to `false` if you want Tab to work globally.

The cycle order follows your `hotkey_order` list in the config. Characters get added automatically when they log in, but you should edit the list to put them in the order you want.

If you don't set up the input group permissions, the program still works fine - you just won't have Tab cycling. Click-to-focus still works normally.

### Debug logging

If something's not working right:

```bash
LOG_LEVEL=debug eve-l-preview
```

Levels: `trace`, `debug`, `info` (default), `warn`, `error`
