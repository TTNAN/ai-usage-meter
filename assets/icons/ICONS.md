# ai-usage-meter — Icons

Original icon assets for the open-source macOS/Windows tray app
**ai-usage-meter**. Inspired by an X post by [@Litx4i](https://x.com/Litx4i)
showing a black Dock with AI icons turned into usage meters (a coloured
progress bar under each icon).

## Originality note

All marks in this folder are **original simplified designs** drawn for this
project. They are not copies of official brand logos, and they do not reuse
any official logo artwork:

- **Claude** — a plain white open "C" arc with a spark dot on an
  orange (`#D97757`) rounded square. Not the official Claude logo.
- **Codex** — two interlocking dark rings with a centre dot on a white
  rounded square. Not any official ChatGPT / Codex logo.
- **Grok** — a purple (`#A855F7`) open "G" arc with a crossbar on a black
  rounded square. Not the official Grok logo.
- **App icon** — a dark rounded square with three ascending gauge bars
  (orange / blue / purple) on a track, plus a "%" readout.

Colours used: Claude orange-brown `#D97757`, Codex blue `#3B82F6`,
Grok purple `#A855F7`, app background `#0A0A0A`.

## Files

| File | Size | What it is |
| --- | --- | --- |
| `app-icon.svg` | 1024×1024 (vector) | App-icon concept: dark rounded square, three gauge bars, "%" |
| `app-icon-1024.png` | 1024×1024 | App icon, largest render |
| `app-icon-512.png` | 512×512 | App icon |
| `app-icon-256.png` | 256×256 | App icon |
| `app-icon-128.png` | 128×128 | App icon |
| `app-icon-64.png` | 64×64 | App icon |
| `app-icon-32.png` | 32×32 | App icon, smallest render (bars + "%" still read) |
| `app.ico` | 16/32/48/64/128/256 | Windows icon, multi-size, built from the PNGs with Pillow |
| `tray-icon.png` | 32×32 | Tray icon: monochrome white gauge bars on transparent |
| `tray-icon-16.png` | 16×16 | Tray icon, small size |
| `claude.svg` / `claude.png` | 128×128 | Claude service mark (icon only) |
| `codex.svg` / `codex.png` | 128×128 | Codex service mark (icon only) |
| `grok.svg` / `grok.png` | 128×128 | Grok service mark (icon only) |
| `claude_with_bar.png` | 128×160 | Claude icon + usage bar underneath, ~40% filled, orange |
| `codex_with_bar.png` | 128×160 | Codex icon + usage bar underneath, ~40% filled, blue |
| `grok_with_bar.png` | 128×160 | Grok icon + usage bar underneath, ~40% filled, purple |

Notes on the split:

- The base service PNGs (`claude.png`, `codex.png`, `grok.png`) are the
  128×128 square marks on their own, for use as Dock / launcher icons.
- The `*_with_bar.png` variants are 128×160: the same 128×128 mark on top
  with a dark track and a coloured fill bar underneath at ~40%, i.e. the
  progress-bar treatment from the post. The app should redraw the fill
  width at runtime to reflect live usage; 40% is just the static example.
- The tray icons are monochrome white-on-transparent so they stay legible
  on both light and dark menu bars / system trays.

## macOS .icns

`app.icns` is **not** included: building one needs macOS `iconutil`
(or `png2icns`) from an `.iconset` of the PNGs above. On a Mac:

```sh
mkdir AppIcon.iconset
# copy the PNGs we have (32/64/128/256/512/1024) as icon_<size>x<size>.png;
# make the 16px entry by downscaling app-icon-32.png, and @2x entries
# from the next size up, e.g.:
cp app-icon-32.png  AppIcon.iconset/icon_16x16@2x.png
cp app-icon-128.png AppIcon.iconset/icon_128x128.png
# ... repeat for each size ...
iconutil -c icns AppIcon.iconset -o app.icns
```

## Regenerating

`generate.py` (Pillow) redraws every PNG and the `.ico` from the same
design; the SVGs are hand-written text and match the PNG designs.
