# Guidy website

The marketing site. Static HTML, CSS and a little JS, no build step.

```sh
python3 -m http.server 4173 -d website   # then open http://localhost:4173
```

## Deploy

The site is served at https://guidy.imran.codes by the `guidy-website` Cloudflare Worker (static assets only, no script). Configuration is in `wrangler.jsonc`; `.assetsignore` keeps the config and this README out of the upload, and `_headers` sets caching and security headers.

```sh
cd website && npx wrangler deploy
```

## Design system

The site uses the app's design language (`docs/ARCHITECTURE.md` §1.3). The tokens at the top of `styles.css` are the same values as `crates/guidy-app/src/ui/design.rs`, `assets/themes/guidy.json` and `docs/design/guidy-mock.html`. If you change a colour in the app, change it here too. Only the type steps are larger, to suit a web page.

| App | Site |
|---|---|
| Ink well inside the graphite frame | `main.well` inside the `body` frame; title bar on top, status bar as the footer |
| `design::eyebrow`, `design::mono`, `design::chip` | `.eyebrow`, `.mono`, `.chip` |
| `design::panel` with rim light, `section` | `.panel.rim`, `.sec`, `.row` |
| `design::stat_grid` | `.stats.c3` / `.stats.c4` |
| `design::blueprint`, crosshair marks | `.bp`, `.x` |
| `design::illustration` (lines, soft fill, accent) | `.ill` with three masked layers from `assets/illustrations/` |
| `design::callout` | `.callout` |
| `design::sheen`, `shimmer_text` | `.sheen`, `.shimmer` (static under reduced motion) |
| `design::Pill`, key caps, `segmented` | `.btn.p` / `.btn.s`, `.kbd`, `.seg` |
| Overlay pill | `.pill`, `.pill.live` |

The theme follows the OS appearance until the visitor picks one with the toggle, like the app's "system" setting.

## Assets

`assets/fonts` and `assets/illustrations` are copies of the files in the repo's top-level `assets/`. Geist and Geist Mono are under the SIL OFL (licence files included). If the illustrations are regenerated with `scripts/illustrations.py`, copy them again.

## Brand assets

The mark is the Signal Path figure in miniature: outlined capsule bars on a dashed baseline, with the middle three in signal yellow.

| File | What |
|---|---|
| `assets/brand/guidy-icon.svg` | Master icon (1024, macOS tile grid): ink tile, blueprint grid, crosshairs, bars |
| `favicon.svg` | Favicon, simplified for 16–32 px (no grid or crosshairs) |
| `.mark` in `index.html` / `404.html` | Title-bar mark, coloured by the theme's CSS variables |
| `og/og.html` | Source of the 1200×630 share card |
| `assets/og.png`, `assets/favicon-32.png`, `assets/apple-touch-icon.png`, `assets/guidy-512.png` | Rendered from the above |

To re-render the PNGs, serve the folder (`python3 -m http.server 4173 -d website`) and screenshot with headless Chrome, for example:

```sh
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
"$CHROME" --headless=new --hide-scrollbars --force-prefers-reduced-motion \
  --window-size=1200,630 --screenshot="$PWD/website/assets/og.png" http://localhost:4173/og/og.html
```

`og/` is excluded from the deploy by `.assetsignore`.
