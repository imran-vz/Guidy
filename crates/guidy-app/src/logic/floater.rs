//! How the recording floater moves. Pure: driven by pointer events and a
//! clock, so the springs and the docking rules are testable.
//!
//! The floater lives on the desktop: every display, in points from the
//! primary display's top-left corner ([`Desk`]). It is either:
//! - **free**: a rounded rectangle the user drags anywhere, from one
//!   display to another. Dragged close to a notch or to a corner of a
//!   display, a liquid bridge reaches out to it, and closer still it snaps
//!   in: the blob is absorbed, and wings grow out of the notch or a leaf
//!   out of the corner;
//! - **docked** ([`Target`]): wings around a notch, or a leaf in a corner.
//!   Pulling them stretches a neck of "jelly" toward the pointer; past
//!   [`DETACH`] it breaks, the wings or the leaf pull back in and the blob
//!   springs to the pointer, wobbling.
//!
//! Every visible quantity is a damped [`Spring`] chasing a target, so the
//! motion stays continuous whatever the pointer does.

use crate::logic::notch::{notch_frame, ScreenInfo};
use crate::logic::recording_widget::{MARGIN, PANEL, TOP_SHARE};

/// The free floater's size (a pill), in design units.
pub const BLOB: (f32, f32) = (134.0, 36.0);
/// The hint beside the stopped floater during a call ("Take meeting
/// notes?"), in design units.
pub const HINT: (f32, f32) = (296.0, 52.0);
/// The leaf tucked into a corner, in design units.
pub const LEAF: (f32, f32) = (84.0, 38.0);
/// The blob docks when its center comes this close to the wings' bottom
/// edge (design units).
pub const ATTACH: f32 = 56.0;
/// The blob tucks into a corner when its center comes this close to it
/// (design units).
pub const CORNER: f32 = 84.0;
/// Pulling the docked wings or leaf this far (pointer to them, design
/// units) breaks them free.
pub const DETACH: f32 = 150.0;
/// A dragged blob closer than this to the wings grows a bridge toward them.
pub const REACH: f32 = 190.0;
/// The same for a corner (measured to the corner itself).
pub const CORNER_REACH: f32 = 170.0;
/// Pointer travel before a press counts as a drag, not a click (points).
pub const DRAG_SLOP: f32 = 4.0;
/// Longest step integrated at once; a stall doesn't make things jump.
const MAX_DT: f32 = 1.0 / 30.0;
/// Integration substep.
const SUBSTEP: f32 = 1.0 / 240.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct P {
    pub x: f32,
    pub y: f32,
}

impl P {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub fn len(self) -> f32 {
        self.x.hypot(self.y)
    }
    pub fn dist(self, o: P) -> f32 {
        (self - o).len()
    }
    /// Unit vector (zero stays zero).
    pub fn unit(self) -> P {
        let l = self.len();
        if l < 1e-4 {
            P::default()
        } else {
            self * (1.0 / l)
        }
    }
    /// Perpendicular (rotated a quarter turn).
    pub fn perp(self) -> P {
        P::new(-self.y, self.x)
    }
}

impl std::ops::Add for P {
    type Output = P;
    fn add(self, o: P) -> P {
        P::new(self.x + o.x, self.y + o.y)
    }
}
impl std::ops::Sub for P {
    type Output = P;
    fn sub(self, o: P) -> P {
        P::new(self.x - o.x, self.y - o.y)
    }
}
impl std::ops::Mul<f32> for P {
    type Output = P;
    fn mul(self, k: f32) -> P {
        P::new(self.x * k, self.y * k)
    }
}

/// A rectangle by its top-left corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct R {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl R {
    pub fn centered(c: P, w: f32, h: f32) -> Self {
        Self {
            x: c.x - w / 2.0,
            y: c.y - h / 2.0,
            w,
            h,
        }
    }
    pub fn center(&self) -> P {
        P::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn contains(&self, p: P) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.bottom()
    }
    pub fn inflate(&self, d: f32) -> R {
        R {
            x: self.x - d,
            y: self.y - d,
            w: self.w + 2.0 * d,
            h: self.h + 2.0 * d,
        }
    }
    pub fn lerp(a: R, b: R, t: f32) -> R {
        R {
            x: lerp(a.x, b.x, t),
            y: lerp(a.y, b.y, t),
            w: lerp(a.w, b.w, t).max(0.0),
            h: lerp(a.h, b.h, t).max(0.0),
        }
    }
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// A damped spring: `value` chases `target`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
    stiffness: f32,
    damping: f32,
}

impl Spring {
    /// `zeta` < 1 overshoots (elastic); 1 settles without.
    pub fn new(value: f32, stiffness: f32, zeta: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
            stiffness,
            damping: 2.0 * zeta * stiffness.sqrt(),
        }
    }

    fn tune(&mut self, stiffness: f32, zeta: f32) {
        self.stiffness = stiffness;
        self.damping = 2.0 * zeta * stiffness.sqrt();
    }

    fn step(&mut self, dt: f32) {
        let force = -self.stiffness * (self.value - self.target) - self.damping * self.velocity;
        self.velocity += force * dt;
        self.value += self.velocity * dt;
    }

    pub fn at_rest(&self) -> bool {
        (self.value - self.target).abs() < 0.002 && self.velocity.abs() < 0.02
    }

    fn settle(&mut self) {
        self.value = self.target;
        self.velocity = 0.0;
    }
}

/// Two springs, one per axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring2 {
    pub x: Spring,
    pub y: Spring,
}

impl Spring2 {
    fn new(p: P, stiffness: f32, zeta: f32) -> Self {
        Self {
            x: Spring::new(p.x, stiffness, zeta),
            y: Spring::new(p.y, stiffness, zeta),
        }
    }
    pub fn value(&self) -> P {
        P::new(self.x.value, self.y.value)
    }
    fn velocity(&self) -> P {
        P::new(self.x.velocity, self.y.velocity)
    }
    fn target(&self) -> P {
        P::new(self.x.target, self.y.target)
    }
    fn set_target(&mut self, p: P) {
        self.x.target = p.x;
        self.y.target = p.y;
    }
    fn set_velocity(&mut self, v: P) {
        self.x.velocity = v.x;
        self.y.velocity = v.y;
    }
    fn tune(&mut self, stiffness: f32, zeta: f32) {
        self.x.tune(stiffness, zeta);
        self.y.tune(stiffness, zeta);
    }
    fn step(&mut self, dt: f32) {
        self.x.step(dt);
        self.y.step(dt);
    }
    fn at_rest(&self) -> bool {
        self.x.at_rest() && self.y.at_rest()
    }
    fn settle(&mut self) {
        self.x.settle();
        self.y.settle();
    }
}

/// The display the floater lives on, in stage points (top-left origin).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stage {
    pub width: f32,
    pub height: f32,
    /// Menu bar / notch height.
    pub top_inset: f32,
    /// The camera housing's horizontal extent; `None` without a notch.
    pub notch: Option<(f32, f32)>,
    /// Interface scale (design units to points).
    pub scale: f32,
}

impl Stage {
    pub fn from_screen(s: &ScreenInfo, scale: f32) -> Self {
        Self {
            width: s.width as f32,
            height: s.height as f32,
            top_inset: s.top_inset as f32,
            notch: s.notch.map(|(a, b)| (a as f32, b as f32)),
            scale,
        }
    }

    /// A 14" MacBook Pro display, for tests and before a screen is known.
    pub fn fallback(scale: f32) -> Self {
        Self {
            width: 1512.0,
            height: 982.0,
            top_inset: 38.0,
            notch: Some((662.0, 850.0)),
            scale,
        }
    }

    fn screen(&self) -> ScreenInfo {
        ScreenInfo {
            x: 0.0,
            y: 0.0,
            width: self.width as f64,
            height: self.height as f64,
            notch: self.notch.map(|(a, b)| (a as f64, b as f64)),
            top_inset: self.top_inset as f64,
        }
    }

    pub fn d(&self, design: f32) -> f32 {
        design * self.scale
    }

    /// The docked wings: both sides of the notch, flush with the top.
    pub fn wings(&self) -> R {
        let f = notch_frame(&self.screen());
        R {
            x: f.x as f32,
            y: 0.0,
            w: f.width as f32,
            h: f.height as f32,
        }
    }

    /// What the wings pull back into: the camera housing (nothing to see),
    /// or a zero-width sliver mid menu bar on a display without one.
    pub fn housing(&self) -> R {
        let w = self.wings();
        match self.notch {
            Some((a, b)) if b > a => R {
                x: a,
                y: 0.0,
                w: b - a,
                h: w.h,
            },
            _ => R {
                x: w.center().x,
                y: 0.0,
                w: 0.0,
                h: w.h,
            },
        }
    }

    /// Where the wings meet a neck pulled toward `toward`: their bottom
    /// edge, under the pointer as far as the wings reach.
    pub fn anchor(&self, toward: P) -> P {
        let w = self.wings();
        let inset = self.d(24.0).min(w.w / 2.0);
        P::new(
            toward.x.clamp(w.x + inset, w.right() - inset),
            w.bottom() - 1.0,
        )
    }

    /// Width and height of the free floater.
    pub fn blob_size(&self) -> (f32, f32) {
        (self.d(BLOB.0), self.d(BLOB.1))
    }

    /// Where a free blob rests by default: at the right edge, clear of the
    /// top-right corner where notifications land.
    pub fn home(&self) -> P {
        let (w, h) = self.blob_size();
        let visible = self.height - self.top_inset;
        P::new(
            self.width - self.d(MARGIN as f32) - w / 2.0,
            self.top_inset + (visible * TOP_SHARE as f32).round() + h / 2.0,
        )
    }

    /// Keep a blob centered at `c` on screen and below the menu bar.
    pub fn clamp_blob(&self, c: P) -> P {
        let (w, h) = self.blob_size();
        let (hw, hh) = (w / 2.0 + self.d(4.0), h / 2.0 + self.d(4.0));
        P::new(
            c.x.clamp(hw, (self.width - hw).max(hw)),
            c.y.clamp(self.top_inset + hh, (self.height - hh).max(hh)),
        )
    }

    /// The panel a docked floater expands into: the notch grown down.
    pub fn docked_panel(&self) -> R {
        let wings = self.wings();
        let w = self.d(PANEL.0 as f32).max(wings.w + self.d(24.0));
        let h = self.d(PANEL.1 as f32) + wings.h;
        R::centered(P::new(wings.center().x, h / 2.0), w, h)
    }

    /// The panel a free floater expands into: from the blob's corner
    /// facing the middle of the screen, kept on screen.
    pub fn free_panel(&self, blob: P) -> R {
        let (w, h) = (self.d(PANEL.0 as f32), self.d(PANEL.1 as f32));
        let (bw, bh) = self.blob_size();
        let margin = self.d(MARGIN as f32);
        let x = if blob.x >= self.width / 2.0 {
            blob.x + bw / 2.0 - w
        } else {
            blob.x - bw / 2.0
        };
        let y = if blob.y <= self.height / 2.0 {
            blob.y - bh / 2.0
        } else {
            blob.y + bh / 2.0 - h
        };
        R {
            x: x.clamp(margin, (self.width - w - margin).max(margin)),
            y: y.clamp(
                self.top_inset + margin,
                (self.height - h - margin).max(self.top_inset + margin),
            ),
            w,
            h,
        }
    }
}

/// A corner of a display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ];

    pub fn is_left(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::BottomLeft)
    }

    pub fn is_top(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::TopRight)
    }
}

impl Stage {
    pub fn leaf_size(&self) -> (f32, f32) {
        (self.d(LEAF.0), self.d(LEAF.1))
    }

    /// The point of a corner. The top ones are under the menu bar.
    pub fn corner_point(&self, c: Corner) -> P {
        P::new(
            if c.is_left() { 0.0 } else { self.width },
            if c.is_top() {
                self.top_inset
            } else {
                self.height
            },
        )
    }

    /// The leaf tucked into corner `c`, flush with both edges.
    pub fn leaf(&self, c: Corner) -> R {
        let (w, h) = self.leaf_size();
        let p = self.corner_point(c);
        R {
            x: if c.is_left() { p.x } else { p.x - w },
            y: if c.is_top() { p.y } else { p.y - h },
            w,
            h,
        }
    }
}

/// Where the floater docks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Wings around display `n`'s notch (or mid menu bar without one).
    Notch(usize),
    /// A leaf in a corner of display `n`.
    Corner(usize, Corner),
}

impl Target {
    pub fn screen(self) -> usize {
        match self {
            Target::Notch(i) | Target::Corner(i, _) => i,
        }
    }
}

/// Every display, placed on the desktop: `origin` is a display's top-left
/// corner in points from the primary display's (y down).
#[derive(Clone, Debug, PartialEq)]
pub struct Desk {
    pub screens: Vec<(P, Stage)>,
}

fn shift(r: R, o: P) -> R {
    R {
        x: r.x + o.x,
        y: r.y + o.y,
        ..r
    }
}

impl Desk {
    /// One display at the origin.
    pub fn single(stage: Stage) -> Self {
        Self {
            screens: vec![(P::default(), stage)],
        }
    }

    /// The displays from `NSScreen` (Cocoa coordinates, primary first).
    pub fn from_screens(screens: &[ScreenInfo], scale: f32) -> Self {
        let Some(primary) = screens.first() else {
            return Self::single(Stage::fallback(scale));
        };
        let top = primary.y + primary.height;
        Self {
            screens: screens
                .iter()
                .map(|s| {
                    let origin = P::new(s.x as f32, (top - (s.y + s.height)) as f32);
                    (origin, Stage::from_screen(s, scale))
                })
                .collect(),
        }
    }

    /// Cocoa coordinates (origin bottom-left of the primary display) to
    /// the desktop's.
    pub fn from_cocoa(&self, x: f64, y: f64) -> P {
        let top = self.screens.first().map_or(0.0, |(o, s)| o.y + s.height);
        P::new(x as f32, top - y as f32)
    }

    pub fn stage(&self, i: usize) -> &Stage {
        &self.screens[i.min(self.screens.len() - 1)].1
    }

    pub fn origin(&self, i: usize) -> P {
        self.screens[i.min(self.screens.len() - 1)].0
    }

    pub fn d(&self, design: f32) -> f32 {
        self.stage(0).d(design)
    }

    /// The display with a notch, else the primary one.
    pub fn main(&self) -> usize {
        self.screens
            .iter()
            .position(|(_, s)| s.notch.is_some())
            .unwrap_or(0)
    }

    pub fn screen_rect(&self, i: usize) -> R {
        let s = self.stage(i);
        let o = self.origin(i);
        R {
            x: o.x,
            y: o.y,
            w: s.width,
            h: s.height,
        }
    }

    /// The display `p` is on, else the closest one.
    pub fn screen_at(&self, p: P) -> usize {
        let dist = |r: R| {
            let dx = (r.x - p.x).max(0.0).max(p.x - r.right());
            let dy = (r.y - p.y).max(0.0).max(p.y - r.bottom());
            dx.hypot(dy)
        };
        (0..self.screens.len())
            .min_by(|a, b| dist(self.screen_rect(*a)).total_cmp(&dist(self.screen_rect(*b))))
            .unwrap_or(0)
    }

    pub fn blob_size(&self) -> (f32, f32) {
        self.stage(0).blob_size()
    }

    /// Where a free blob rests by default: on the main display.
    pub fn home(&self) -> P {
        let m = self.main();
        self.origin(m) + self.stage(m).home()
    }

    /// Keep a blob centered at `c` on the display it is (nearest) on.
    pub fn clamp_blob(&self, c: P) -> P {
        let i = self.screen_at(c);
        let o = self.origin(i);
        self.stage(i).clamp_blob(c - o) + o
    }

    /// The docked shape at rest.
    pub fn shape(&self, t: Target) -> R {
        match t {
            Target::Notch(i) => shift(self.stage(i).wings(), self.origin(i)),
            Target::Corner(i, c) => shift(self.stage(i).leaf(c), self.origin(i)),
        }
    }

    /// What the shape pulls back into: the camera housing, or the corner.
    pub fn housing(&self, t: Target) -> R {
        match t {
            Target::Notch(i) => shift(self.stage(i).housing(), self.origin(i)),
            Target::Corner(i, c) => {
                let p = self.stage(i).corner_point(c) + self.origin(i);
                R {
                    x: p.x,
                    y: p.y,
                    w: 0.0,
                    h: 0.0,
                }
            }
        }
    }

    /// Where a neck pulled toward `toward` leaves the docked shape, and the
    /// direction out of the shape there.
    pub fn anchor(&self, t: Target, toward: P) -> (P, P) {
        match t {
            Target::Notch(i) => {
                let o = self.origin(i);
                (self.stage(i).anchor(toward - o) + o, P::new(0.0, 1.0))
            }
            Target::Corner(_, c) => {
                let leaf = self.shape(t);
                let inset = self.d(12.0).min(leaf.w / 2.0).min(leaf.h / 2.0);
                // The leaf's inner edges face away from the corner.
                let sx = if c.is_left() { 1.0 } else { -1.0 };
                let sy = if c.is_top() { 1.0 } else { -1.0 };
                let d = toward - leaf.center();
                if (d.x * sx) / leaf.w > (d.y * sy) / leaf.h {
                    let x = if sx > 0.0 {
                        leaf.right() - 1.0
                    } else {
                        leaf.x + 1.0
                    };
                    let y = toward.y.clamp(leaf.y + inset, leaf.bottom() - inset);
                    (P::new(x, y), P::new(sx, 0.0))
                } else {
                    let y = if sy > 0.0 {
                        leaf.bottom() - 1.0
                    } else {
                        leaf.y + 1.0
                    };
                    let x = toward.x.clamp(leaf.x + inset, leaf.right() - inset);
                    (P::new(x, y), P::new(0.0, sy))
                }
            }
        }
    }

    /// The panel a docked floater opens: the notch grown down, or beside
    /// the leaf.
    pub fn panel(&self, t: Target) -> R {
        match t {
            Target::Notch(i) => shift(self.stage(i).docked_panel(), self.origin(i)),
            Target::Corner(..) => self.free_panel(self.shape(t).center()),
        }
    }

    /// The panel a free floater at `blob` opens, on its display.
    pub fn free_panel(&self, blob: P) -> R {
        let i = self.screen_at(blob);
        let o = self.origin(i);
        shift(self.stage(i).free_panel(blob - o), o)
    }

    /// Where a blob dragged to `c` docks, if it is close enough to a notch
    /// or to a corner of its display.
    fn snap(&self, c: P) -> Option<Target> {
        let half_h = self.blob_size().1 / 2.0;
        for i in 0..self.screens.len() {
            let t = Target::Notch(i);
            if c.dist(self.anchor(t, c).0) < self.d(ATTACH) + half_h {
                return Some(t);
            }
        }
        let i = self.screen_at(c);
        let o = self.origin(i);
        Corner::ALL
            .into_iter()
            .find(|k| c.dist(self.stage(i).corner_point(*k) + o) < self.d(CORNER))
            .map(|k| Target::Corner(i, k))
    }

    /// The docking place a dragged blob at `c` is nearest, and how near
    /// (1 touching, 0 at the edge of its reach), for the bridge toward it.
    fn reaching(&self, c: P) -> Option<(Target, f32)> {
        let mut best: Option<(Target, f32)> = None;
        let mut consider = |t: Target, near: f32| {
            if near > 0.0 && best.is_none_or(|(_, b)| near > b) {
                best = Some((t, near.min(1.0)));
            }
        };
        let half_h = self.blob_size().1 / 2.0;
        for i in 0..self.screens.len() {
            let t = Target::Notch(i);
            let d = c.dist(self.anchor(t, c).0) - half_h;
            consider(t, 1.0 - d / self.d(REACH));
        }
        let i = self.screen_at(c);
        let o = self.origin(i);
        for k in Corner::ALL {
            let d = c.dist(self.stage(i).corner_point(k) + o);
            consider(Target::Corner(i, k), 1.0 - d / self.d(CORNER_REACH));
        }
        best
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Free,
    Docked,
}

/// The floater docked somewhere, or broke free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockChange {
    Docked(Target),
    Free,
}

/// A press in progress.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Press {
    start: P,
    /// Blob center minus pointer at the press, so it doesn't jump.
    grab: P,
    moved: bool,
}

/// What a release did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Release {
    /// It didn't move: a click.
    Click,
    Dropped,
    /// No press was in progress.
    None,
}

/// A liquid bridge between a docked shape and the blob, for drawing. `a` is
/// on the shape's edge (`normal` points out of it there), `b` the blob's
/// center; the half-widths are at `a`, at `b` and at the narrowest point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Neck {
    pub a: P,
    pub normal: P,
    pub b: P,
    pub half_a: f32,
    pub half_b: f32,
    pub waist: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Floater {
    pub mode: Mode,
    /// Where it is docked; while free, where its shape last was (it
    /// retracts there).
    pub target: Target,
    /// Blob center.
    pos: Spring2,
    /// Blob scale per axis (1 at rest): stretch while moving, wobble after.
    squash: Spring2,
    /// Blob size factor: 1 full; toward 0 while it is absorbed.
    size: Spring,
    /// 0: the docked shape is pulled in; 1: out.
    presence: Spring,
    /// 0 collapsed, 1 expanded (the notch grows into the panel).
    pub expand: Spring,
    press: Option<Press>,
    /// The pointer while it pulls at the docked shape.
    tether: Option<P>,
    /// Where the free blob rests.
    home: P,
    /// Animate (false: everything jumps to its target; reduced motion).
    pub animate: bool,
}

/// Spring tunings: (stiffness, damping ratio).
const FOLLOW: (f32, f32) = (900.0, 0.72);
const SNAP: (f32, f32) = (320.0, 0.42);
const SETTLE: (f32, f32) = (260.0, 0.6);

impl Floater {
    /// Free at its home, or docked at the main display's notch.
    pub fn new(desk: &Desk, docked: bool) -> Self {
        let home = desk.home();
        let target = Target::Notch(desk.main());
        let mut f = Self {
            mode: Mode::Free,
            target,
            pos: Spring2::new(home, SETTLE.0, SETTLE.1),
            squash: Spring2::new(P::new(1.0, 1.0), 520.0, 0.18),
            size: Spring::new(1.0, 380.0, 0.5),
            presence: Spring::new(0.0, 360.0, 0.38),
            expand: Spring::new(0.0, 300.0, 0.78),
            press: None,
            tether: None,
            home,
            animate: true,
        };
        if docked {
            f.mode = Mode::Docked;
            f.pos = Spring2::new(desk.shape(target).center(), SNAP.0, SNAP.1);
            f.size = Spring::new(0.0, 380.0, 0.5);
            f.presence = Spring::new(1.0, 360.0, 0.38);
        }
        f
    }

    pub fn is_docked(&self) -> bool {
        self.mode == Mode::Docked
    }

    /// Docked at a notch (not in a corner).
    pub fn at_notch(&self) -> bool {
        self.is_docked() && matches!(self.target, Target::Notch(_))
    }

    pub fn dragging(&self) -> bool {
        self.press.is_some_and(|p| p.moved)
    }

    pub fn pressed(&self) -> bool {
        self.press.is_some()
    }

    pub fn blob_center(&self) -> P {
        self.pos.value()
    }

    /// Where the free blob rests (it is moved by drags).
    pub fn home(&self) -> P {
        self.home
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        self.expand.target = if expanded { 1.0 } else { 0.0 };
    }

    pub fn expanded(&self) -> bool {
        self.expand.target > 0.5
    }

    /// The display the floater is on (where its panel opens).
    pub fn screen(&self, desk: &Desk) -> usize {
        match self.mode {
            Mode::Docked => self.target.screen(),
            Mode::Free => desk.screen_at(self.pos.value()),
        }
    }

    /// Dock at the notch (or float to its resting place) without a drag:
    /// the setting changed.
    pub fn set_docked(&mut self, desk: &Desk, docked: bool) {
        if self.press.is_some() || self.at_notch() == docked {
            return;
        }
        if docked {
            self.dock(desk, Target::Notch(desk.main()));
        } else {
            self.mode = Mode::Free;
            self.tether = None;
            self.presence.target = 0.0;
            self.size.target = 1.0;
            self.pos.tune(SETTLE.0, SETTLE.1);
            self.pos.set_target(self.home);
        }
    }

    fn dock(&mut self, desk: &Desk, t: Target) {
        self.mode = Mode::Docked;
        if self.target != t {
            // A shape elsewhere: this one starts pulled in.
            self.presence.value = 0.0;
            self.presence.velocity = 0.0;
        }
        self.target = t;
        self.presence.target = 1.0;
        self.size.target = 0.0;
        self.pos.tune(SNAP.0, SNAP.1);
        self.pos.set_target(desk.shape(t).center());
        // The blob is swallowed: a squeeze toward the shape.
        self.squash.x.velocity += 3.0;
        self.squash.y.velocity -= 3.0;
    }

    /// A press on the floater (the blob or the docked shape) at `p`.
    pub fn press(&mut self, p: P) {
        let grab = match self.mode {
            Mode::Free => self.pos.value() - p,
            // Pulled out of the shape, the blob comes out under the pointer.
            Mode::Docked => P::default(),
        };
        self.press = Some(Press {
            start: p,
            grab,
            moved: false,
        });
    }

    /// The pointer moved to `p` with the button down. Returns the change
    /// when it docked or broke free.
    pub fn drag_to(&mut self, desk: &Desk, p: P) -> Option<DockChange> {
        let mut press = self.press?;
        if !press.moved {
            if p.dist(press.start) < DRAG_SLOP {
                return None;
            }
            // Not while expanded: the panel isn't dragged around.
            if self.expanded() {
                return None;
            }
            press.moved = true;
            self.press = Some(press);
        }
        match self.mode {
            Mode::Free => {
                let target = desk.clamp_blob(p + press.grab);
                self.pos.tune(FOLLOW.0, FOLLOW.1);
                self.pos.set_target(target);
                let t = desk.snap(target)?;
                self.dock(desk, t);
                self.tether = Some(p);
                self.pull(desk, p);
                Some(DockChange::Docked(t))
            }
            Mode::Docked => {
                self.tether = Some(p);
                let (anchor, _) = desk.anchor(self.target, p);
                if p.dist(anchor) > desk.d(DETACH) {
                    self.break_free(desk, p);
                    return Some(DockChange::Free);
                }
                self.pull(desk, p);
                None
            }
        }
    }

    /// Docked, pulled toward `p`: the blob end of the neck follows the
    /// pointer with resistance and grows as it comes out.
    fn pull(&mut self, desk: &Desk, p: P) {
        let (anchor, _) = desk.anchor(self.target, p);
        let t = (p.dist(anchor) / desk.d(DETACH)).clamp(0.0, 1.0);
        self.pos.tune(FOLLOW.0, FOLLOW.1);
        self.pos
            .set_target(anchor + (p - anchor) * (0.55 + 0.25 * t));
        self.size.target = 0.35 + 0.45 * t;
    }

    /// The neck snaps: the shape pulls back in and the blob springs to the
    /// pointer.
    fn break_free(&mut self, desk: &Desk, p: P) {
        self.mode = Mode::Free;
        self.tether = None;
        self.presence.target = 0.0;
        self.size.target = 1.0;
        let target = desk.clamp_blob(p);
        let from = self.pos.value();
        self.pos.tune(FOLLOW.0, FOLLOW.1);
        self.pos.set_target(target);
        // Released tension: a fling toward the pointer and a wobble.
        self.pos
            .set_velocity(self.pos.velocity() + (target - from) * 12.0);
        let dir = (target - from).unit();
        self.squash.x.velocity += 6.0 * dir.y.abs() - 4.0 * dir.x.abs();
        self.squash.y.velocity += 6.0 * dir.x.abs() - 4.0 * dir.y.abs();
        if let Some(press) = &mut self.press {
            press.grab = P::default();
        }
    }

    /// The button went up.
    pub fn release(&mut self, desk: &Desk) -> Release {
        let Some(press) = self.press.take() else {
            return Release::None;
        };
        self.tether = None;
        if !press.moved {
            return Release::Click;
        }
        match self.mode {
            Mode::Docked => {
                // Let go of the neck: it springs back in.
                self.size.target = 0.0;
                self.pos.tune(SNAP.0, SNAP.1);
                self.pos.set_target(desk.shape(self.target).center());
            }
            Mode::Free => {
                self.home = desk.clamp_blob(self.pos.target());
                self.pos.tune(SETTLE.0, SETTLE.1);
                self.pos.set_target(self.home);
            }
        }
        Release::Dropped
    }

    /// The displays changed: keep the floater on them.
    pub fn restage(&mut self, desk: &Desk) {
        if self.target.screen() >= desk.screens.len() {
            self.target = Target::Notch(desk.main());
        }
        self.home = desk.clamp_blob(self.home);
        if self.press.is_some() {
            return;
        }
        match self.mode {
            Mode::Free => self.pos.set_target(self.home),
            Mode::Docked => self.pos.set_target(desk.shape(self.target).center()),
        }
    }

    /// Advance the animation by `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        if !self.animate {
            self.settle();
            return;
        }
        // The blob stretches along its motion (velocity in points/s).
        let v = self.pos.velocity();
        let stretch = |a: f32, b: f32| {
            let along = (a.abs() / 2600.0).min(1.0);
            let across = (b.abs() / 2600.0).min(1.0);
            1.0 + 0.28 * along - 0.16 * across
        };
        if self.press.is_some_and(|p| p.moved) && self.mode == Mode::Free {
            self.squash
                .set_target(P::new(stretch(v.x, v.y), stretch(v.y, v.x)));
        } else {
            self.squash.set_target(P::new(1.0, 1.0));
        }
        let mut left = dt.clamp(0.0, MAX_DT);
        while left > 0.0 {
            let h = left.min(SUBSTEP);
            self.pos.step(h);
            self.squash.step(h);
            self.size.step(h);
            self.presence.step(h);
            self.expand.step(h);
            left -= h;
        }
    }

    fn settle(&mut self) {
        self.pos.settle();
        self.squash.set_target(P::new(1.0, 1.0));
        self.squash.settle();
        self.size.settle();
        self.presence.settle();
        self.expand.settle();
    }

    /// Something still moves (keep drawing frames).
    pub fn animating(&self) -> bool {
        self.animate
            && (self.dragging()
                || !self.pos.at_rest()
                || !self.squash.at_rest()
                || !self.size.at_rest()
                || !self.presence.at_rest()
                || !self.expand.at_rest())
    }

    // -- what to draw -------------------------------------------------------

    /// The blob's rectangle, if any of it shows.
    pub fn blob(&self, desk: &Desk) -> Option<R> {
        let size = self.size.value.max(0.0);
        if size < 0.04 || (self.mode == Mode::Free && self.expanded()) {
            return None;
        }
        let (w, h) = desk.blob_size();
        let sq = self.squash.value();
        Some(R::centered(
            self.pos.value(),
            w * size * sq.x.max(0.2),
            h * size * sq.y.max(0.2),
        ))
    }

    /// How much the blob's own controls show (only a whole, resting blob).
    pub fn blob_content(&self) -> f32 {
        if self.mode == Mode::Docked {
            return 0.0;
        }
        ((self.size.value - 0.75) / 0.25).clamp(0.0, 1.0)
    }

    /// The docked shape (wings or leaf; at a notch, the panel growing out
    /// of the wings), if out at all.
    pub fn shape(&self, desk: &Desk) -> Option<R> {
        let w = self.presence.value.max(0.0);
        if w < 0.02 {
            return None;
        }
        let rect = R::lerp(desk.housing(self.target), desk.shape(self.target), w);
        // The panel only grows out of docked wings.
        let e = if self.at_notch() {
            self.expand.value.clamp(0.0, 1.2)
        } else {
            0.0
        };
        Some(R::lerp(rect, desk.panel(self.target), e))
    }

    /// How much the docked shape's controls show (out, and not turning
    /// into the panel).
    pub fn shape_content(&self) -> f32 {
        let out = ((self.presence.value - 0.7) / 0.3).clamp(0.0, 1.0);
        let collapsed = if self.at_notch() {
            1.0 - (self.expand.value / 0.3).clamp(0.0, 1.0)
        } else {
            1.0
        };
        out * collapsed
    }

    /// How much the panel grown out of the notch shows its content.
    pub fn docked_panel_content(&self) -> f32 {
        if !self.at_notch() {
            return 0.0;
        }
        ((self.expand.value - 0.75) / 0.25).clamp(0.0, 1.0)
    }

    /// The separate panel: opened from the free blob or from a leaf.
    pub fn free_panel(&self, desk: &Desk) -> Option<R> {
        if !self.expanded() {
            return None;
        }
        match (self.mode, self.target) {
            (Mode::Free, _) => Some(desk.free_panel(self.pos.target())),
            (Mode::Docked, Target::Corner(..)) => Some(desk.panel(self.target)),
            (Mode::Docked, Target::Notch(_)) => None,
        }
    }

    /// The hint beside the floater: under the wings, else beside the leaf or
    /// the blob toward the middle of its display, kept on screen and below
    /// the menu bar. None while it is open, held or on its way in.
    pub fn hint(&self, desk: &Desk) -> Option<R> {
        if self.expanded() || self.press.is_some() {
            return None;
        }
        let (anchor, below) = match self.mode {
            Mode::Docked => (self.shape(desk)?, self.at_notch()),
            Mode::Free => (self.blob(desk)?, false),
        };
        let i = self.screen(desk);
        let screen = desk.screen_rect(i);
        let (w, h) = (desk.d(HINT.0), desk.d(HINT.1));
        let gap = desk.d(8.0);
        let (x, y) = if below {
            (anchor.center().x - w / 2.0, anchor.bottom() + gap)
        } else if anchor.center().x >= screen.center().x {
            (anchor.x - gap - w, anchor.center().y - h / 2.0)
        } else {
            (anchor.right() + gap, anchor.center().y - h / 2.0)
        };
        let margin = desk.d(MARGIN as f32);
        let top = screen.y + desk.stage(i).top_inset + margin;
        Some(R {
            x: x.clamp(
                screen.x + margin,
                (screen.right() - w - margin).max(screen.x + margin),
            ),
            y: y.clamp(top, (screen.bottom() - h - margin).max(top)),
            w,
            h,
        })
    }

    /// The liquid bridge between the docked shape and the blob, if any.
    pub fn neck(&self, desk: &Desk) -> Option<Neck> {
        let blob = self.blob(desk)?;
        let b = blob.center();
        let r = blob.w.min(blob.h) / 2.0;
        match self.mode {
            Mode::Docked => {
                // Out of the shape (pulled, or springing back in).
                self.shape(desk)?;
                let (a, normal) = desk.anchor(self.target, b);
                let out = (b.x - a.x) * normal.x + (b.y - a.y) * normal.y;
                if out <= r * 0.5 {
                    return None;
                }
                let t = (b.dist(a) / desk.d(DETACH)).clamp(0.0, 1.0);
                Some(Neck {
                    a,
                    normal,
                    b,
                    half_a: r * (1.35 - 0.5 * t),
                    half_b: r * 0.9,
                    waist: r * (0.78 - 0.58 * t),
                })
            }
            Mode::Free => {
                // Dragged near: the notch or the corner reaches out for it.
                if !self.dragging() {
                    return None;
                }
                let (t, near) = desk.reaching(b)?;
                let (a, normal) = match t {
                    Target::Notch(_) => desk.anchor(t, b),
                    // From the corner itself (no leaf yet).
                    Target::Corner(i, c) => {
                        let p = desk.stage(i).corner_point(c) + desk.origin(i);
                        (p, (b - p).unit())
                    }
                };
                Some(Neck {
                    a,
                    normal,
                    b,
                    half_a: r * 0.75 * near,
                    half_b: r * 0.55 * near,
                    waist: r * 0.16 * near * near,
                })
            }
        }
    }

    /// Whether the floater takes the pointer at `p` (elsewhere clicks go
    /// through to the apps beneath).
    pub fn hit(&self, desk: &Desk, p: P) -> bool {
        if self.press.is_some() {
            return true;
        }
        let pad = desk.d(4.0);
        if self
            .free_panel(desk)
            .is_some_and(|panel| panel.inflate(pad).contains(p))
        {
            return true;
        }
        if self.mode == Mode::Docked {
            if self.at_notch() && self.expanded() {
                return self.shape(desk).is_some_and(|s| s.inflate(pad).contains(p));
            }
            if self.shape(desk).is_some_and(|s| s.inflate(pad).contains(p)) {
                return true;
            }
            return false;
        }
        if self.free_panel(desk).is_some() {
            return false;
        }
        self.blob(desk).is_some_and(|b| b.inflate(pad).contains(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage() -> Stage {
        Stage::fallback(1.2)
    }

    fn desk() -> Desk {
        Desk::single(stage())
    }

    /// The laptop (with the notch) and a 1920x1080 monitor to its right,
    /// its top 100 points higher.
    fn two_displays() -> Desk {
        let laptop = ScreenInfo {
            x: 0.0,
            y: 0.0,
            width: 1512.0,
            height: 982.0,
            notch: Some((662.0, 850.0)),
            top_inset: 38.0,
        };
        let monitor = ScreenInfo {
            x: 1512.0,
            y: 982.0 + 100.0 - 1080.0,
            width: 1920.0,
            height: 1080.0,
            notch: None,
            top_inset: 25.0,
        };
        Desk::from_screens(&[laptop, monitor], 1.2)
    }

    /// Run the springs for `secs`.
    fn run(f: &mut Floater, secs: f32) {
        let mut t = 0.0;
        while t < secs {
            f.step(1.0 / 60.0);
            t += 1.0 / 60.0;
        }
    }

    /// Drag from `from` to `to` in small steps.
    fn drag(f: &mut Floater, d: &Desk, from: P, to: P) -> Vec<DockChange> {
        let mut changes = Vec::new();
        f.press(from);
        for i in 1..=40 {
            let p = from + (to - from) * (i as f32 / 40.0);
            changes.extend(f.drag_to(d, p));
            f.step(1.0 / 60.0);
        }
        changes
    }

    #[test]
    fn springs_overshoot_and_settle() {
        let mut s = Spring::new(0.0, SNAP.0, SNAP.1);
        s.target = 1.0;
        let mut peak: f32 = 0.0;
        for _ in 0..240 {
            s.step(1.0 / 120.0);
            peak = peak.max(s.value);
        }
        assert!(peak > 1.05, "elastic: overshoots ({peak})");
        assert!(s.at_rest(), "{s:?}");
    }

    #[test]
    fn a_free_blob_rests_at_the_right_edge_and_takes_clicks() {
        let d = desk();
        let mut f = Floater::new(&d, false);
        let home = f.blob_center();
        assert!(home.x > d.stage(0).width - d.blob_size().0);
        assert!(f.hit(&d, home));
        assert!(
            !f.hit(&d, P::new(100.0, 500.0)),
            "clicks elsewhere go through"
        );
        f.press(home);
        assert_eq!(f.release(&d), Release::Click);
        assert_eq!(f.release(&d), Release::None);
        assert!(f.shape(&d).is_none());
        assert_eq!(f.blob_content(), 1.0);
    }

    #[test]
    fn the_hint_sits_beside_the_floater_toward_the_middle() {
        let d = desk();
        let s = d.stage(0);
        let mut f = Floater::new(&d, false);
        run(&mut f, 1.0);
        let blob = f.blob(&d).unwrap();
        let hint = f.hint(&d).unwrap();
        assert!(hint.right() < blob.x, "left of the blob at the right edge");
        assert!((hint.center().y - blob.center().y).abs() < 0.01);
        // Held, it hides.
        f.press(blob.center());
        assert!(f.hint(&d).is_none());
        f.release(&d);
        // At the notch: under the wings.
        let mut f = Floater::new(&d, true);
        run(&mut f, 1.0);
        let wings = f.shape(&d).unwrap();
        let hint = f.hint(&d).unwrap();
        assert!(hint.y > wings.bottom());
        assert!((hint.center().x - wings.center().x).abs() < 0.01);
        // In the top-left corner: right of the leaf, below the menu bar.
        let corner = s.corner_point(Corner::TopLeft);
        let mut f = Floater::new(&d, false);
        let from = f.blob_center();
        drag(&mut f, &d, from, corner + P::new(10.0, 10.0));
        f.release(&d);
        run(&mut f, 2.0);
        let leaf = f.shape(&d).unwrap();
        let hint = f.hint(&d).unwrap();
        assert!(hint.x > leaf.right());
        assert!(hint.y >= s.top_inset);
    }

    #[test]
    fn a_drag_moves_it_and_it_stays_where_dropped() {
        let d = desk();
        let mut f = Floater::new(&d, false);
        let from = f.blob_center();
        let to = P::new(400.0, 600.0);
        assert!(drag(&mut f, &d, from, to).is_empty());
        assert!(f.dragging());
        assert_eq!(f.release(&d), Release::Dropped);
        run(&mut f, 2.0);
        assert!(f.blob_center().dist(to) < 1.0, "{:?}", f.blob_center());
        assert!(!f.animating());
    }

    #[test]
    fn dragged_near_the_notch_it_reaches_out_then_snaps_in() {
        let d = desk();
        let mut f = Floater::new(&d, false);
        let wings = d.shape(Target::Notch(0));
        let from = f.blob_center();
        // Getting close: a bridge, still free.
        let near = P::new(
            wings.center().x,
            wings.bottom() + d.d(ATTACH) + d.blob_size().1 + 40.0,
        );
        assert!(drag(&mut f, &d, from, near).is_empty());
        assert_eq!(f.mode, Mode::Free);
        assert!(f.neck(&d).is_some(), "a bridge reaches for it");
        // Closer: docked while still held.
        let changes = drag(
            &mut f,
            &d,
            near,
            P::new(wings.center().x, wings.bottom() + 10.0),
        );
        assert_eq!(changes, vec![DockChange::Docked(Target::Notch(0))]);
        f.release(&d);
        run(&mut f, 2.0);
        assert!(f.at_notch());
        assert_eq!(f.shape(&d), Some(wings));
        assert!(f.blob(&d).is_none(), "absorbed");
        assert_eq!(f.shape_content(), 1.0);
        assert!(f.hit(&d, wings.center()));
    }

    #[test]
    fn pulling_the_wings_stretches_a_neck_until_it_breaks() {
        let d = desk();
        let mut f = Floater::new(&d, true);
        let wings = d.shape(Target::Notch(0));
        let grab = wings.center();
        let short = P::new(grab.x, wings.bottom() + d.d(DETACH) * 0.6);
        assert!(drag(&mut f, &d, grab, short).is_empty());
        run(&mut f, 0.3);
        let neck = f.neck(&d).expect("a neck");
        assert!(neck.waist < neck.half_a, "it thins as it stretches");
        assert_eq!(f.release(&d), Release::Dropped);
        run(&mut f, 2.0);
        assert!(f.at_notch());
        assert!(f.blob(&d).is_none());
        // A long pull breaks it: free, under the pointer, wings back in.
        let far = P::new(grab.x - 100.0, wings.bottom() + d.d(DETACH) + 80.0);
        let changes = drag(&mut f, &d, grab, far);
        assert_eq!(changes, vec![DockChange::Free]);
        f.release(&d);
        run(&mut f, 2.0);
        assert!(f.blob_center().dist(far) < 1.0, "{:?}", f.blob_center());
        assert!(f.shape(&d).is_none());
        assert_eq!(f.blob_content(), 1.0);
    }

    #[test]
    fn dragged_into_a_corner_it_tucks_in_as_a_leaf_and_pulls_back_out() {
        let d = desk();
        let s = d.stage(0);
        let mut f = Floater::new(&d, false);
        let from = f.blob_center();
        let corner = s.corner_point(Corner::BottomRight);
        // Near it: a bridge from the corner.
        let near = corner - P::new(d.d(CORNER) + 30.0, d.d(CORNER) + 30.0) * 0.75;
        assert!(drag(&mut f, &d, from, near).is_empty());
        assert!(f.neck(&d).is_some());
        // Caught up with the pointer before it is grabbed again.
        run(&mut f, 1.0);
        // In it: a leaf, flush with both edges.
        let t = Target::Corner(0, Corner::BottomRight);
        let changes = drag(&mut f, &d, near, corner - P::new(20.0, 20.0));
        assert_eq!(changes, vec![DockChange::Docked(t)]);
        f.release(&d);
        run(&mut f, 2.0);
        let leaf = f.shape(&d).unwrap();
        let want = s.leaf(Corner::BottomRight);
        assert!(
            (leaf.x - want.x).abs() < 0.01
                && (leaf.y - want.y).abs() < 0.01
                && (leaf.w - want.w).abs() < 0.01
                && (leaf.h - want.h).abs() < 0.01,
            "{leaf:?}"
        );
        assert!((leaf.right() - s.width).abs() < 0.01);
        assert!((leaf.bottom() - s.height).abs() < 0.01);
        assert!(f.blob(&d).is_none());
        assert_eq!(f.shape_content(), 1.0);
        // A click opens the panel beside it; the leaf stays.
        f.press(leaf.center());
        assert_eq!(f.release(&d), Release::Click);
        f.set_expanded(true);
        run(&mut f, 1.0);
        let panel = f.free_panel(&d).unwrap();
        assert!(panel.right() <= s.width && panel.bottom() <= s.height);
        assert!(f.shape(&d).is_some());
        f.set_expanded(false);
        run(&mut f, 1.0);
        // Pulled out toward the middle, past the break.
        let out = leaf.center() - P::new(d.d(DETACH) + 120.0, 100.0);
        assert_eq!(drag(&mut f, &d, leaf.center(), out), vec![DockChange::Free]);
        f.release(&d);
        run(&mut f, 2.0);
        assert!(f.blob_center().dist(out) < 1.0);
        assert!(f.shape(&d).is_none());
    }

    #[test]
    fn the_top_corners_are_under_the_menu_bar() {
        let s = stage();
        let leaf = s.leaf(Corner::TopLeft);
        assert_eq!((leaf.x, leaf.y), (0.0, s.top_inset));
        let d = desk();
        let (a, n) = d.anchor(
            Target::Corner(0, Corner::TopLeft),
            P::new(400.0, s.top_inset + 10.0),
        );
        assert_eq!(n, P::new(1.0, 0.0), "pulled sideways: from its inner side");
        assert!(a.x > leaf.x);
    }

    #[test]
    fn it_moves_to_another_display_and_docks_there() {
        let d = two_displays();
        assert_eq!(d.main(), 0);
        // The monitor's top-left is at (1512, -100) on the desktop.
        assert_eq!(d.origin(1), P::new(1512.0, -100.0));
        assert_eq!(d.from_cocoa(1512.0, 982.0 + 100.0), P::new(1512.0, -100.0));
        let mut f = Floater::new(&d, false);
        let from = f.blob_center();
        let on_monitor = P::new(2400.0, 500.0);
        assert!(drag(&mut f, &d, from, on_monitor).is_empty());
        f.release(&d);
        run(&mut f, 2.0);
        assert!(f.blob_center().dist(on_monitor) < 1.0);
        assert_eq!(f.screen(&d), 1);
        // Its panel opens on the monitor.
        f.set_expanded(true);
        let panel = f.free_panel(&d).unwrap();
        assert!(d.screen_rect(1).contains(panel.center()));
        f.set_expanded(false);
        // The monitor has no notch: its menu bar island takes it.
        let island = d.shape(Target::Notch(1));
        assert!((island.center().x - (1512.0 + 960.0)).abs() < 1.0);
        assert_eq!(island.y, -100.0);
        let changes = drag(
            &mut f,
            &d,
            on_monitor,
            P::new(island.center().x, island.bottom() + 8.0),
        );
        assert_eq!(changes, vec![DockChange::Docked(Target::Notch(1))]);
        // Dropped between displays, it stays on the nearer one.
        let mut f = Floater::new(&d, false);
        let from = f.blob_center();
        drag(&mut f, &d, from, P::new(1400.0, 1100.0));
        f.release(&d);
        run(&mut f, 2.0);
        assert!(d.screen_rect(0).contains(f.blob_center()));
    }

    #[test]
    fn a_click_on_the_wings_expands_the_notch_into_a_panel() {
        let d = desk();
        let mut f = Floater::new(&d, true);
        let wings = d.shape(Target::Notch(0));
        f.press(wings.center());
        assert_eq!(f.release(&d), Release::Click);
        f.set_expanded(true);
        run(&mut f, 2.0);
        let panel = d.panel(Target::Notch(0));
        let w = f.shape(&d).unwrap();
        assert!((w.w - panel.w).abs() < 0.5 && (w.h - panel.h).abs() < 0.5);
        assert_eq!(w.y, 0.0, "attached to the top");
        assert_eq!(f.docked_panel_content(), 1.0);
        assert_eq!(f.shape_content(), 0.0);
        assert!(f.free_panel(&d).is_none());
        assert!(f.hit(&d, P::new(panel.center().x, panel.bottom() - 5.0)));
        // Expanded, it isn't pulled out.
        f.press(wings.center());
        assert!(f.drag_to(&d, P::new(700.0, 700.0)).is_none());
        assert!(f.at_notch());
    }

    #[test]
    fn the_free_panel_opens_toward_the_middle_and_stays_on_screen() {
        let s = stage();
        let right = s.free_panel(s.home());
        assert!(right.right() <= s.width && right.x >= 0.0);
        let left = s.free_panel(P::new(40.0, s.height - 40.0));
        assert!(left.x >= 0.0 && left.bottom() <= s.height);
        assert!(left.y >= s.top_inset);
    }

    #[test]
    fn the_setting_docks_and_undocks_it() {
        let d = desk();
        let mut f = Floater::new(&d, false);
        f.set_docked(&d, true);
        run(&mut f, 2.0);
        assert_eq!(f.shape(&d), Some(d.shape(Target::Notch(0))));
        f.set_docked(&d, false);
        run(&mut f, 2.0);
        assert!(f.shape(&d).is_none());
        assert!(f.blob_center().dist(d.home()) < 1.0);
    }

    #[test]
    fn without_animation_everything_jumps() {
        let d = desk();
        let mut f = Floater::new(&d, false);
        f.animate = false;
        f.set_docked(&d, true);
        f.step(0.0);
        assert_eq!(f.shape(&d), Some(d.shape(Target::Notch(0))));
        assert!(!f.animating());
    }

    #[test]
    fn a_display_without_a_notch_docks_mid_menu_bar() {
        let s = Stage {
            notch: None,
            top_inset: 24.0,
            ..stage()
        };
        let w = s.wings();
        assert!((w.center().x - s.width / 2.0).abs() < 1.0);
        assert_eq!(s.housing().w, 0.0);
    }
}
