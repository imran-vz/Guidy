"""Generate the onboarding permission drawings from 3D geometry.

Usage: python3 scripts/illustrations.py assets/illustrations

One orthographic camera (elevation E) for every object, so the mic and the
keycaps share a projection. Writes three layers per illustration (lines,
soft fill, accent) as single-colour SVGs; the app tints each layer.
"""
import math, sys, os
E = math.radians(22)
VB = (0, 0, 480, 210)
OUT = sys.argv[1] if len(sys.argv) > 1 else 'assets/illustrations'

def P(x, y, z, ox, oy):
    # y up, z toward the viewer; camera looks down by E.
    return (ox + x, oy - y * math.cos(E) + z * math.sin(E))

def poly(pts, close=False):
    s = 'M' + ' L'.join(f'{x:.1f} {y:.1f}' for x, y in pts)
    return s + (' Z' if close else '')

def ring(r, y, ox, oy, a0=0, a1=360, n=72):
    """Horizontal circle at height y; angles in degrees, 90 = toward viewer."""
    pts = []
    for i in range(n + 1):
        a = math.radians(a0 + (a1 - a0) * i / n)
        pts.append(P(r * math.cos(a), y, r * math.sin(a), ox, oy))
    return pts

# ---------------- microphone ----------------
def microphone(ox, oy, active):
    R = 24
    body_bot, grille_bot, cyl_top = -78, -30, 0
    lines, fill, acc = [], [], []
    grille = acc if active else lines
    # body cylinder below the grille
    lines.append(poly([P(-R, grille_bot, 0, ox, oy), P(-R, body_bot, 0, ox, oy)]))
    lines.append(poly([P(R, grille_bot, 0, ox, oy), P(R, body_bot, 0, ox, oy)]))
    lines.append(poly(ring(R, body_bot, ox, oy, 0, 180)))
    for y in (-36, -41):  # collar
        lines.append(poly(ring(R, y, ox, oy, 0, 180)))
    lines.append(poly(ring(R * 0.62, -62, ox, oy, 25, 155)))  # badge line
    # grille: cylinder part + dome, outline and mesh
    sil = [P(-R, grille_bot, 0, ox, oy), P(-R, cyl_top, 0, ox, oy)]
    for i in range(0, 181, 5):  # dome silhouette (sphere outline is a circle)
        a = math.radians(180 - i)
        sil.append((ox + R * math.cos(a), oy - cyl_top * math.cos(E) - R * math.sin(a)))
    sil.append(P(R, grille_bot, 0, ox, oy))
    bottom = ring(R, grille_bot, ox, oy, 0, 180)
    outline = sil + bottom[::-1]
    grille.append(poly(sil) + ' ' + poly(bottom))
    fill.append(poly(outline, True))
    mesh = []
    for y in range(-24, 1, 8):
        mesh.append(poly(ring(R, y, ox, oy, 0, 180)))
    for lat in (30, 60):  # dome rings (front half)
        la = math.radians(lat)
        mesh.append(poly(ring(R * math.cos(la), cyl_top + R * math.sin(la), ox, oy, 0, 180, 48)))
    for az in (-60, -30, 0, 30, 60):  # meridians, from the grille base over the dome
        a = math.radians(90 + az)
        cx_, cz = math.cos(a), math.sin(a)
        pts = [P(R * cx_, grille_bot, R * cz, ox, oy), P(R * cx_, cyl_top, R * cz, ox, oy)]
        for i in range(1, 31):
            la = math.radians(90 * i / 30)
            r = R * math.cos(la)
            pts.append(P(r * cx_, cyl_top + R * math.sin(la), r * cz, ox, oy))
        mesh.append(poly(pts))
    grille.append(('mesh', ' '.join(mesh)))
    # yoke: a U in the x-y plane, pivoting on the body
    py, arm = -56, R + 9
    lines.append(poly([P(-arm, py, 0, ox, oy), P(-arm, -86, 0, ox, oy)] +
                      [P(-arm * math.cos(math.radians(t)), -86 - 16 * math.sin(math.radians(t)), 0, ox, oy) for t in range(0, 181, 6)] +
                      [P(arm, py, 0, ox, oy)]))
    for sx in (-1, 1):
        c = P(sx * arm, py, 0, ox, oy)
        lines.append(poly([(c[0] + 3.2 * math.cos(math.radians(t)), c[1] + 3.2 * math.sin(math.radians(t))) for t in range(0, 361, 20)], True))
        lines.append(poly([P(sx * (arm - 3.2), py, 0, ox, oy), P(sx * R, py, 0, ox, oy)]))
    # stem and weighted base
    lines.append(poly([P(0, -102, 0, ox, oy), P(0, -126, 0, ox, oy)]))
    lines.append(poly(ring(30, -126, ox, oy)))
    lines.append(poly(ring(30, -131, ox, oy, 0, 180)))
    lines.append(poly([P(-30, -126, 0, ox, oy), P(-30, -131, 0, ox, oy)]))
    lines.append(poly([P(30, -126, 0, ox, oy), P(30, -131, 0, ox, oy)]))
    # incoming sound: chevrons on the axis above the dome
    top = oy - cyl_top * math.cos(E) - R
    chev = ' '.join(f'M{ox-6:.1f} {top-34+i*9:.1f} l6 6 6-6' for i in range(3))
    (acc if active else lines).append(('chev', chev))
    anchor = (ox + R + 2, oy - (-14) * math.cos(E) + R * 0.0)
    return lines, fill, acc, anchor

# ---------------- keycap ----------------
def keycap(cx, cz, rot, glyph, oy, ox, active, ground=-131):
    """A keycap on the ground plane: tapered skirt, smaller top, glyph on top."""
    B, T, H = 50, 38, 15
    th = math.radians(rot)
    def w(u, v, y):  # key-local (u right, v toward viewer) -> world -> screen
        x = cx + u * math.cos(th) - v * math.sin(th)
        z = cz + u * math.sin(th) + v * math.cos(th)
        return P(x, y, z, ox, oy)
    sq = lambda s: [(-s/2, -s/2), (s/2, -s/2), (s/2, s/2), (-s/2, s/2)]
    base = [w(u, v, ground) for u, v in sq(B)]
    top = [w(u, v, ground + H) for u, v in sq(T)]
    lines, fill, acc = [], [], []
    hi = acc if active else lines
    # visible faces: projected winding (screen y down) tells which face fronts us
    for i in range(4):
        j = (i + 1) % 4
        quad = [base[i], base[j], top[j], top[i]]
        area = sum(quad[k][0] * quad[(k+1) % 4][1] - quad[(k+1) % 4][0] * quad[k][1] for k in range(4))
        if area > 0:
            lines.append(poly(quad, True))
    lines.append(poly(top, True))
    # a shallow dish: inset outline on the top face
    dish = [w(u, v, ground + H) for u, v in sq(T - 6)]
    lines.append(poly(dish, True))
    fill.append(poly(top, True))
    # glyph in top-face coordinates (u, v in -1..1)
    gs = (T - 10) / 2
    g = lambda pts: poly([w(u * gs, v * gs, ground + H) for u, v in pts])
    if glyph == 'cmd':
        # ⌘: an inner square whose sides run on into four open loops.
        q, c, r = 0.22, 0.5, 0.28
        parts = []
        for k in (-1, 1):
            parts.append(g([(k * q, -c), (k * q, c)]))
            parts.append(g([(-c, k * q), (c, k * q)]))
        for sx in (-1, 1):
            for sy in (-1, 1):
                ccx, ccy = sx * c, sy * c
                a1 = math.atan2(0, -sx)
                outer = math.atan2(sy, sx)
                for d in (1.5 * math.pi, -1.5 * math.pi):
                    mid = a1 + d / 2
                    if abs(math.atan2(math.sin(mid - outer), math.cos(mid - outer))) < 0.1:
                        break
                pts = [(ccx + r * math.cos(a1 + d * i / 36), ccy + r * math.sin(a1 + d * i / 36)) for i in range(37)]
                parts.append(g(pts))
        hi.append(('glyph', ' '.join(parts)))
    else:  # V
        hi.append(('glyph', g([(-0.6, -0.75), (0, 0.75), (0.6, -0.75)])))
    edge = max(base, key=lambda p: p[0])
    return lines, fill, acc, edge

def svg(paths, color='#000', fill=False):
    out = []
    for p in paths:
        if isinstance(p, tuple):
            kind, d = p
            sw = {'mesh': 0.9, 'chev': 1.4, 'glyph': 1.3}[kind]
            op = ' stroke-opacity=".8"' if kind == 'mesh' else ''
            out.append(f'<path d="{d}" stroke-width="{sw}"{op}/>')
        elif fill:
            out.append(f'<path d="{p}" fill="{color}" stroke="none"/>')
        else:
            out.append(f'<path d="{p}" stroke-width="1.2"/>')
    vb = ' '.join(map(str, VB))
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vb}" fill="none" stroke="{color}" '
            f'stroke-linecap="round" stroke-linejoin="round">\n' + '\n'.join(out) + '\n</svg>\n')

def build(mic_active):
    ml, mf, ma, mic_anchor = microphone(100, 64, mic_active)
    k1 = keycap(0, 0, 28, 'cmd', 64, 216, not mic_active, ground=-104)
    k2 = keycap(66, 0, 28, 'v', 64, 216, not mic_active, ground=-104)
    return (ml, mf, ma), (k1[0] + k2[0], k1[1] + k2[1], k1[2] + k2[2]), mic_anchor, k2[3]

def waveform():
    """A voice waveform: capsule bars on a dashed axis, the loudest three
    in the accent. Front-on (2D): it stands for sound, not an object."""
    head = ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 300 130" fill="none" '
            'stroke="#000" stroke-linecap="round" stroke-linejoin="round">\n')
    heights = [14, 26, 40, 58, 34, 72, 88, 64, 40, 52, 30, 20, 12]
    base = ['<path d="M6 65H294" stroke-width="1" stroke-dasharray="3 3" stroke-opacity=".45"/>',
            '<path d="M150 4V126" stroke-width="1" stroke-dasharray="3 3" stroke-opacity=".45"/>']
    fill, acc = [], []
    for i, h in enumerate(heights):
        x = 30 + i * 20
        rect = f'<rect x="{x - 4}" y="{65 - h / 2}" width="8" height="{h}" rx="4"'
        if 5 <= i <= 7:
            fill.append(rect + ' fill="#000" stroke="none"/>')
            acc.append(rect + ' stroke-width="1.4"/>')
        else:
            base.append(rect + ' stroke-width="1.2"/>')
    for suffix, body in (('', base), ('-fill', fill), ('-accent', acc)):
        open(f'{OUT}/waveform{suffix}.svg', 'w').write(head + '\n'.join(body) + '\n</svg>\n')


if __name__ == '__main__':
    os.makedirs(OUT, exist_ok=True)
    waveform()
    # Files: the accent layer holds what may be highlighted; the app tints it
    # by state, so generate with both parts in the accent layer.
    (ml, mf, ma), _, mic_anchor, _ = build(True)
    _, (kl, kf, ka), _, key_edge = build(False)
    for name, (l, f, a) in (('microphone', (ml, mf, ma)), ('paste-keys', (kl, kf, ka))):
        open(f'{OUT}/{name}.svg', 'w').write(svg(l))
        open(f'{OUT}/{name}-fill.svg', 'w').write(svg(f, fill=True))
        open(f'{OUT}/{name}-accent.svg', 'w').write(svg(a))
    print('mic callout anchor', [round(v) for v in mic_anchor], 'key edge', [round(v) for v in key_edge])
