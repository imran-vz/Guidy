#!/usr/bin/env python3
"""Generate Guidy's brand assets deterministically.

Outputs (relative to the repo root):
  assets/resources/tray_*.png     menu-bar / tray glyphs (64x64, template-style)
  assets/resources/*_start.wav    feedback sounds ("marimba" and "pop" themes)
  assets/icons/app/guidy.{png,icns,ico}  application icon

Run: python3 scripts/gen_brand_assets.py
"""
import math
import os
import subprocess
import tempfile
import wave

import numpy as np
from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RES = os.path.join(ROOT, "assets", "resources")
APP = os.path.join(ROOT, "assets", "icons", "app")

ACCENT = (116, 173, 232, 255)  # Zed One Dark text.accent
BG = (40, 44, 51, 255)  # One Dark editor background
WARN = (222, 193, 132, 255)
RED = (208, 114, 119, 255)

# Waveform bar heights (fractions of the glyph height).
BARS = [0.30, 0.62, 1.0, 0.62, 0.30]


def waveform(draw, box, color, bars=BARS, gap_ratio=0.55):
    x0, y0, x1, y1 = box
    w, h = x1 - x0, y1 - y0
    n = len(bars)
    bar_w = w / (n + (n - 1) * gap_ratio)
    gap = bar_w * gap_ratio
    cy = y0 + h / 2
    for i, frac in enumerate(bars):
        bx = x0 + i * (bar_w + gap)
        bh = max(bar_w, h * frac)
        draw.rounded_rectangle(
            [bx, cy - bh / 2, bx + bar_w, cy + bh / 2], radius=bar_w / 2, fill=color
        )


def tray_icon(color, state, warning=False, size=64, scale=4):
    s = size * scale
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    pad = 8 * scale
    if state == "idle":
        waveform(d, (pad, pad, s - pad, s - pad), color)
    elif state == "recording":
        waveform(d, (pad, pad + 4 * scale, s - pad - 14 * scale, s - pad - 4 * scale), color)
        r = 9 * scale
        cx, cy = s - pad - r + 2 * scale, s / 2
        d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=color)
    elif state == "transcribing":
        waveform(d, (pad, pad, s - pad, s / 2 + 6 * scale), color, bars=[0.3, 0.6, 1.0, 0.6, 0.3])
        r = 3.5 * scale
        for i in range(3):
            cx = s / 2 + (i - 1) * 11 * scale
            cy = s - pad - r
            d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=color)
    if warning:
        # Small triangle badge in the corner.
        t = 22 * scale
        x1, y1 = s - 2 * scale, s - 2 * scale
        d.polygon([(x1 - t, y1), (x1, y1), (x1 - t / 2, y1 - t)], fill=color)
    return img.resize((size, size), Image.LANCZOS)


def write_tray_icons():
    white = (255, 255, 255, 255)
    black = (0, 0, 0, 255)
    for state in ["idle", "recording", "transcribing"]:
        # Light glyphs for dark menu bars; `_dark` = dark glyphs for light bars.
        tray_icon(white, state).save(os.path.join(RES, f"tray_{state}.png"))
        tray_icon(black, state).save(os.path.join(RES, f"tray_{state}_dark.png"))
    tray_icon(white, "idle", warning=True).save(os.path.join(RES, "tray_idle_warning.png"))
    tray_icon(black, "idle", warning=True).save(os.path.join(RES, "tray_idle_warning_dark.png"))
    # Full-colour variants (Linux trays).
    tray_icon(ACCENT, "idle").save(os.path.join(RES, "tray_colored.png"))
    tray_icon(RED, "recording").save(os.path.join(RES, "tray_colored_recording.png"))
    tray_icon(WARN, "transcribing").save(os.path.join(RES, "tray_colored_transcribing.png"))


def app_icon(size=1024):
    scale = 2
    s = size * scale
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    inset = int(s * 0.09)
    d.rounded_rectangle([inset, inset, s - inset, s - inset], radius=int(s * 0.2), fill=BG)
    # Subtle border, Zed-style hairline.
    d.rounded_rectangle(
        [inset, inset, s - inset, s - inset], radius=int(s * 0.2), outline=(70, 75, 87, 255), width=int(s * 0.006)
    )
    m = int(s * 0.27)
    waveform(d, (m, m, s - m, s - m), ACCENT)
    return img.resize((size, size), Image.LANCZOS)


def write_app_icon():
    os.makedirs(APP, exist_ok=True)
    icon = app_icon()
    icon.save(os.path.join(APP, "guidy.png"))
    icon.resize((256, 256), Image.LANCZOS).save(
        os.path.join(APP, "guidy.ico"), sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
    )
    with tempfile.TemporaryDirectory() as tmp:
        iconset = os.path.join(tmp, "guidy.iconset")
        os.makedirs(iconset)
        for base in [16, 32, 128, 256, 512]:
            icon.resize((base, base), Image.LANCZOS).save(os.path.join(iconset, f"icon_{base}x{base}.png"))
            icon.resize((base * 2, base * 2), Image.LANCZOS).save(os.path.join(iconset, f"icon_{base}x{base}@2x.png"))
        try:
            subprocess.run(["iconutil", "-c", "icns", iconset, "-o", os.path.join(APP, "guidy.icns")], check=True)
        except FileNotFoundError:
            print("iconutil not found; skipping .icns")


SR = 44100


def envelope(n, attack=0.004, decay=6.0):
    t = np.arange(n) / SR
    env = np.exp(-decay * t)
    a = int(attack * SR)
    env[:a] *= np.linspace(0, 1, a)
    return env


def mallet(freq, dur):
    """Marimba-like tone: fundamental plus the 4x and 10x partials, fast decay."""
    n = int(dur * SR)
    t = np.arange(n) / SR
    tone = (
        1.0 * np.sin(2 * math.pi * freq * t)
        + 0.35 * np.sin(2 * math.pi * freq * 4 * t) * np.exp(-30 * t)
        + 0.12 * np.sin(2 * math.pi * freq * 10 * t) * np.exp(-60 * t)
    )
    return tone * envelope(n, decay=9.0)


def blip(f0, f1, dur):
    """Pop: short pitch glide with a soft tail."""
    n = int(dur * SR)
    t = np.arange(n) / SR
    freq = np.linspace(f0, f1, n)
    phase = 2 * math.pi * np.cumsum(freq) / SR
    return np.sin(phase) * envelope(n, attack=0.002, decay=28.0)


def sequence(parts, gap):
    out = np.zeros(0)
    for i, p in enumerate(parts):
        if i:
            out = np.concatenate([out, np.zeros(int(gap * SR))])
        out = np.concatenate([out, p])
    return out


def write_wav(path, samples, gain=0.45):
    samples = samples / max(1e-9, np.max(np.abs(samples))) * gain
    pcm = (samples * 32767).astype("<i2")
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())


def write_sounds():
    # Rising two-note figure to start, falling to stop (A5/E6 pentatonic).
    write_wav(os.path.join(RES, "marimba_start.wav"), sequence([mallet(880, 0.18), mallet(1318.5, 0.32)], 0.0))
    write_wav(os.path.join(RES, "marimba_stop.wav"), sequence([mallet(1318.5, 0.18), mallet(880, 0.32)], 0.0))
    write_wav(os.path.join(RES, "pop_start.wav"), blip(520, 980, 0.12))
    write_wav(os.path.join(RES, "pop_stop.wav"), blip(980, 520, 0.12))


def main():
    write_tray_icons()
    write_app_icon()
    write_sounds()
    for stale in ["handy.png", "handy_warning.png", "recording.png", "transcribing.png", "default_settings.json"]:
        p = os.path.join(RES, stale)
        if os.path.exists(p):
            os.remove(p)
    print("brand assets written")


if __name__ == "__main__":
    main()
