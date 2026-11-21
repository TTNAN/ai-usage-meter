# Icons

## Service marks (official, used for identification only)

| File | Brand | Source |
| --- | --- | --- |
| `claude.svg` | Claude (Anthropic) | Simple Icons — https://cdn.simpleicons.org/claude |
| `openai.svg` / `codex.svg` | OpenAI | Simple Icons v13 — https://cdn.jsdelivr.net/npm/simple-icons@13/icons/openai.svg |
| `grok.svg` | Grok / xAI | Official grok.com favicon (`https://grok.com/images/favicon.svg`), background removed — white orbit ring with the diagonal slash |
| `gemini.svg` | Google Gemini | Official 2025 icon, Wikimedia Commons `File:Google_Gemini_icon_2025.svg` (the four-colour spark shown on gemini.google.com) |
| `copilot.svg` | GitHub Copilot | GitHub Primer Octicons `copilot-24.svg` — https://unpkg.com/@primer/octicons/build/svg/copilot-24.svg |

In the app (`src/icons/`) the single-colour marks (Claude, OpenAI, Grok, Copilot)
are used as CSS masks and drawn white on a small tile, so the shapes are unchanged.
Gemini keeps its own four colours, so it is an `<img>` on a near-white tile —
that is what makes the spark read at 17 px. The same files live in
`assets/icons/` for the README and for anyone re-using them.

## App icon and tray icon (original, this project)

- `app-icon.svg` / `app-icon-512.png` / `app-icon-1024.png` — graphite
  squircle with three rounded bars in the service accents. The PNGs and
  `src-tauri/icons/*` (icns, ico, png) are rendered from the same design.
- The menu-bar / tray icon is **drawn at runtime** (`src-tauri/src/tray_icon.rs`):
  three tiny bars filled to the share of each quota that is left. On macOS it
  is a template image, so it follows light and dark menu bars; on Windows each bar uses its service's accent, including a colour picked in
  Settings (`set_custom_colors` → `tray_icon::render`).

## Trademark

Claude is a trademark of Anthropic. ChatGPT / OpenAI / Codex are trademarks of
OpenAI. Grok / xAI are trademarks of xAI. Gemini / Google are trademarks of
Google. GitHub Copilot is a trademark of GitHub. Brand marks are used only to
identify the service each row refers to and imply no endorsement.
