# Package-manager distribution

The GitHub Release ships three installers — `AI.Usage.Meter_<ver>_universal.dmg`
(macOS, Apple silicon + Intel in one image), `AI.Usage.Meter_<ver>_x64-setup.exe`
(Windows, NSIS, per-user) and `AI.Usage.Meter_<ver>_amd64.deb` (Linux x64). This
directory holds the manifests that point the usual package managers at them.

Every manifest pins the same release URL scheme
`…/releases/download/v<version>/<artifact>`; bumping a version means one line in
each manifest — or nothing at all, once the auto-update action below is in.

| Directory | Manager | Install |
| --------- | ------- | ------- |
| `homebrew/` | Homebrew (macOS) | `brew install --cask ai-usage-meter` |
| `scoop/` | Scoop (Windows) | `scoop install ai-usage-meter` |
| `aur/` | AUR (Arch Linux) | `yay -S ai-usage-meter-bin` (or build with `makepkg -si`) |

## Homebrew: `TTNAN/homebrew-tap`

A cask can live in a tap you own. One-time setup:

```bash
# create the tap repository on GitHub, then:
brew tap-new TTNAN/homebrew-tap
cp packaging/homebrew/ai-usage-meter.rb "$(brew --repository)/Library/Taps/ttnan/homebrew-tap/Casks/"
brew tap TTNAN/homebrew-tap
brew install --cask ttnan/homebrew-tap/ai-usage-meter
# or, once the tap has a tap_migrations.json / CI of its own:
# brew install --cask ai-usage-meter   (after `brew tap TTNAN/homebrew-tap`)
```

Users then install with:

```bash
brew tap TTNAN/homebrew-tap
brew install --cask ai-usage-meter
```

The cask strips the macOS quarantine attribute in its `postflight` (the app is
not notarized), so it launches straight away — the same job the README's
`xattr -cr` one-liner does by hand. `sha256` in the cask is the hash of the
`.dmg` the version names; `brew audit --cask` recomputes it on every bump.

## Scoop: `TTNAN/scoop-bucket`

One-time setup:

```powershell
# create the bucket repository on GitHub (any name ending in -bucket), then:
 scoop bucket add ttnan https://github.com/TTNAN/scoop-bucket
git clone https://github.com/TTNAN/scoop-bucket
cp packaging/scoop/ai-usage-meter.json scoop-bucket/
git -C scoop-bucket commit -am "ai-usage-meter"
```

The manifest drives the NSIS installer in its silent mode (`/S`) and uninstalls
the same way; the app itself installs per-user into
`%LOCALAPPDATA%\Programs\AI Usage Meter` and manages its own autostart and
settings, so there is nothing to shim. `checkver: github` + `autoupdate` make
` scoop update ai-usage-meter` track new releases without manual edits.

## AUR: `ai-usage-meter-bin`

`packaging/aur/PKGBUILD` unpacks the official `.deb` (so the binary is exactly
the one CI built), declares the runtime libraries the deb itself declares
(`webkit2gtk-4.1`, `gtk3`, `libayatana-appindicator`) and lays the payload onto
`/`:

```bash
cd packaging/aur
makepkg -si           # builds ./ai-usage-meter-bin-<ver>-1-x86_64.pkg.tar.zst
```

Publishing it on the AUR means pushing a git repository named
`ai-usage-meter-bin` containing this PKGBUILD plus a generated `.SRCINFO`
(`makepkg --printsrcinfo > .SRCINFO`). Bumping a version: edit `pkgver`, clear
`pkgrel` to 1, replace `sha256sums` with the new deb's hash, regenerate
`.SRCINFO`.

## Optional: update the manifests on every release

A small job appended to `.github/workflows/build.yml` bumps the three manifests
whenever the Release job publishes, opening a PR with the new hashes:

```yaml
  update-packaging:
    name: Bump packaging manifests
    needs: release
    if: startsWith(github.ref, 'refs/tags/v')
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          ref: main
      - name: Read the version from the tag
        run: echo "VERSION=${GITHUB_REF_NAME#v}" >> "$GITHUB_ENV"
      - name: Bump cask / bucket / PKGBUILD
        run: |
          hash_dmg=$(curl -fsSL "https://github.com/TTNAN/ai-usage-meter/releases/download/v${VERSION}/AI.Usage.Meter_${VERSION}_universal.dmg" | sha256sum | cut -d' ' -f1)
          hash_exe=$(curl -fsSL "https://github.com/TTNAN/ai-usage-meter/releases/download/v${VERSION}/AI.Usage.Meter_${VERSION}_x64-setup.exe" | sha256sum | cut -d' ' -f1)
          hash_deb=$(curl -fsSL "https://github.com/TTNAN/ai-usage-meter/releases/download/v${VERSION}/AI.Usage.Meter_${VERSION}_amd64.deb" | sha256sum | cut -d' ' -f1)
          sed -i "s/^\(  version \).*/\1\"${VERSION}\"/; s/^\(  sha256 \).*/\1\"${hash_dmg}\"/" packaging/homebrew/ai-usage-meter.rb
          sed -i "s/^\(    \"version\": \).*/\1\"${VERSION}\"/; s/^\(    \"hash\": \).*/\1\"${hash_exe}\"/" packaging/scoop/ai-usage-meter.json
          sed -i "s/^pkgver=.*/pkgver=${VERSION}/; s/^sha256sums=(.*/sha256sums=('${hash_deb}')/" packaging/aur/PKGBUILD
          makepkg --printsrcinfo > packaging/aur/.SRCINFO || true
      - name: Open a PR with the bump
        uses: peter-evans/create-pull-request@v6
        with:
          commit-message: "chore(packaging): bump manifests to ${VERSION}"
          title: "packaging: ${VERSION}"
          branch: packaging/bump-${{ github.ref_name }}
```

(The `update-packaging` job runs after `release`, which already publishes the
artifacts, so the hashes it downloads are the ones the release carries.)
