# Icons

## Service marks (official, used for identification only)

| File | Brand | Source |
| --- | --- | --- |
| `claude.svg` | Claude (Anthropic) | Simple Icons — https://cdn.simpleicons.org/claude |
| `openai.svg` / `codex.svg` | OpenAI | Simple Icons v13 — https://cdn.jsdelivr.net/npm/simple-icons@13/icons/openai.svg |
| `grok.png` | Grok / xAI | Official x.ai favicon, background made transparent, mark recoloured white |

In the app (`src/icons/`) each mark is used as a CSS mask and drawn white on
a small tile, so the shapes are unchanged.

## App icon and tray icon (original, this project)

- `app-icon.svg` / `app-icon-512.png` / `app-icon-1024.png` — graphite
  squircle with three rounded bars in the service accents. The PNGs and
  `src-tauri/icons/*` (icns, ico, png) are rendered from the same design.
- The menu-bar / tray icon is **drawn at runtime** (`src-tauri/src/tray_icon.rs`):
  three tiny bars filled to the share of each quota that is left. On macOS it
  is a template image, so it follows light and dark menu bars.

## Trademark

Claude is a trademark of Anthropic. ChatGPT / OpenAI / Codex are trademarks of
OpenAI. Grok / xAI are trademarks of xAI. Brand marks are used only to
identify the service each row refers to and imply no endorsement.
