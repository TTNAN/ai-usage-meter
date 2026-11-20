# Icons

All service icons in this project are the **official brand marks**, used
as-is for identification only. The app icon and tray icon are original
designs by this project (see below).

## Service icons (official)

| File | Brand | Exact source |
| --- | --- | --- |
| `claude.svg` / `claude.png` | Claude (Anthropic) | Simple Icons — https://cdn.simpleicons.org/claude (SVG, fill `#D97757`), rendered white on an orange `#D97757` tile |
| `openai.svg` / `codex.svg` / `codex.png` | OpenAI / ChatGPT (OpenAI) | Simple Icons v13 — https://cdn.jsdelivr.net/npm/simple-icons@13/icons/openai.svg (the official OpenAI knot). Simple Icons no longer ships an OpenAI icon in its latest release, so v13 is used. The Codex card uses this OpenAI mark with the text label "Codex" underneath; it is not relabelled as a different brand |
| `grok.png` | Grok / xAI | Official x.ai favicon — https://x.ai/favicon.ico (48×48, black X on white). Simple Icons has no grok/xai icon in any release, so the official favicon is used instead. The white square background was made transparent and the mark recoloured white for the dark tile, so it reads correctly on the black Dock-style background; the mark shape itself is unchanged |

The `*_with_bar.png` variants are the same tiles with the coloured
progress bar from the original X post underneath (orange / blue / purple,
~40% fill in the asset files; the app draws the live value at runtime).

## App icon and tray icon (original, this project)

- `app-icon.svg` / `app-icon-512.png` / `app-icon-1024.png` — dark rounded
  square with three ascending gauge bars (orange / blue / purple) and a `%`.
  Original design, drawn for this project.
- `tray-icon.png` / `tray-icon-16.png` — monochrome white gauge bars on
  transparent, for light/dark menu bars.
- `src-tauri/icons/icon.icns` was generated from `app-icon-1024.png` with a
  small Python script (PNG-based icns elements); `icon.ico` is the
  multi-size Windows icon built from the same PNGs.

## Trademark

Claude is a trademark of Anthropic. ChatGPT / OpenAI are trademarks of
OpenAI. Grok / xAI are trademarks of xAI. All brand icons and trademarks
belong to their respective owners, are used here only to identify the
service each card refers to, and do not imply endorsement by any of them.

Design inspired by the X post by
[@Litx4i](https://x.com/Litx4i/status/2123400444458488080) showing the
black Dock with usage-meter bars.
