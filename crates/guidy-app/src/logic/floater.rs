//! How the recording floater moves. Pure: driven by pointer events and a
//! clock, so the springs and the docking rules are testable.
//!
//! The floater lives on a stage the size of one display (the one with the
//! notch), in points from its top-left corner. It is either:
//! - **free**: a rounded rectangle the user drags anywhere. Dragged close to the
//!   notch, a liquid bridge reaches out to it, and closer still it snaps
//!   in: the blob is absorbed and wings grow out of the notch;
//! - **docked**: wings around the notch. Pulling them stretches a neck of
//!   "jelly" toward the pointer; past [`DETACH`] it breaks, the wings pull
//!   back into the notch and the blob springs to the pointer, wobbling.
//!
//! Every visible quantity is a damped [`Spring`] chasing a target, so the
//! motion stays continuous whatever the pointer does.

use crate::logic::notch::{notch_frame, ScreenInfo};
use crate::logic::recording_widget::{MARGIN, PANEL, TOP_SHARE};

/// The free floater's size (a rounded rectangle), in design units.
pub const BLOB: (f32, f32) = (96.0, 44.0);
/// The blob docks when its center comes this close to the wings' bottom
/// edge (design units).
pub const ATTACH: f32 = 56.0;
/// Pulling the docked wings this far (pointer to wings, design units)
/// breaks them free.
pub const DETACH: f32 = 150.0;
/// A dragged blob closer than this to the wings grows a bridge toward them.
pub const REACH: f32 = 190.0;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Free,
    Docked,
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

/// A liquid bridge between the wings and the blob, for drawing. `a` is on
/// the wings' bottom edge, `b` the blob's center; the half-widths are at
/// `a`, at `b` and at the narrowest point between.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Neck {
    pub a: P,
    pub b: P,
    pub half_a: f32,
    pub half_b: f32,
    pub waist: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Floater {
    pub mode: Mode,
    /// Blob center.
    pos: Spring2,
    /// Blob scale per axis (1 at rest): stretch while moving, wobble after.
    squash: Spring2,
    /// Blob size factor: 1 full; toward 0 while it is absorbed by the wings.
    size: Spring,
    /// 0: the wings are inside the notch; 1: out.
    wings: Spring,
    /// 0 collapsed, 1 expanded (the docked panel grows out of the wings).
    pub expand: Spring,
    press: Option<Press>,
    /// The pointer while it pulls at the docked wings.
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
    pub fn new(stage: &Stage, docked: bool) -> Self {
        let home = stage.home();
        let mut f = Self {
            mode: Mode::Free,
            pos: Spring2::new(home, SETTLE.0, SETTLE.1),
            squash: Spring2::new(P::new(1.0, 1.0), 520.0, 0.18),
            size: Spring::new(1.0, 380.0, 0.5),
            wings: Spring::new(0.0, 360.0, 0.38),
            expand: Spring::new(0.0, 300.0, 0.78),
            press: None,
            tether: None,
            home,
            animate: true,
        };
        if docked {
            f.mode = Mode::Docked;
            let c = stage.wings().center();
            f.pos = Spring2::new(c, SNAP.0, SNAP.1);
            f.size = Spring::new(0.0, 380.0, 0.5);
            f.wings = Spring::new(1.0, 360.0, 0.38);
        }
        f
    }

    pub fn is_docked(&self) -> bool {
        self.mode == Mode::Docked
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

    /// Dock (or float to its resting place) without a drag: the setting
    /// changed.
    pub fn set_docked(&mut self, stage: &Stage, docked: bool) {
        if self.press.is_some() || self.is_docked() == docked {
            return;
        }
        if docked {
            self.dock(stage);
        } else {
            self.mode = Mode::Free;
            self.tether = None;
            self.wings.target = 0.0;
            self.size.target = 1.0;
            self.pos.tune(SETTLE.0, SETTLE.1);
            self.pos.set_target(self.home);
        }
    }

    fn dock(&mut self, stage: &Stage) {
        self.mode = Mode::Docked;
        self.wings.target = 1.0;
        self.size.target = 0.0;
        self.pos.tune(SNAP.0, SNAP.1);
        self.pos.set_target(stage.wings().center());
        // The blob is swallowed: a squeeze toward the notch.
        self.squash.x.velocity += 3.0;
        self.squash.y.velocity -= 3.0;
    }

    /// A press on the floater (the blob or the wings) at `p`.
    pub fn press(&mut self, p: P) {
        let grab = match self.mode {
            Mode::Free => self.pos.value() - p,
            // Pulled out of the wings, the blob comes out under the pointer.
            Mode::Docked => P::default(),
        };
        self.press = Some(Press {
            start: p,
            grab,
            moved: false,
        });
    }

    /// The pointer moved to `p` with the button down. Returns the new mode
    /// when it docked or broke free.
    pub fn drag_to(&mut self, stage: &Stage, p: P) -> Option<Mode> {
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
                let target = stage.clamp_blob(p + press.grab);
                self.pos.tune(FOLLOW.0, FOLLOW.1);
                self.pos.set_target(target);
                let anchor = stage.anchor(target);
                if target.dist(anchor) < stage.d(ATTACH) + stage.blob_size().1 / 2.0 {
                    self.dock(stage);
                    self.tether = Some(p);
                    self.pull(stage, p);
                    return Some(Mode::Docked);
                }
                None
            }
            Mode::Docked => {
                self.tether = Some(p);
                let anchor = stage.anchor(p);
                if p.dist(anchor) > stage.d(DETACH) {
                    self.break_free(stage, p);
                    return Some(Mode::Free);
                }
                self.pull(stage, p);
                None
            }
        }
    }

    /// Docked, pulled toward `p`: the blob end of the neck follows the
    /// pointer with resistance and grows as it comes out.
    fn pull(&mut self, stage: &Stage, p: P) {
        let anchor = stage.anchor(p);
        let t = (p.dist(anchor) / stage.d(DETACH)).clamp(0.0, 1.0);
        self.pos.tune(FOLLOW.0, FOLLOW.1);
        self.pos
            .set_target(anchor + (p - anchor) * (0.55 + 0.25 * t));
        self.size.target = 0.35 + 0.45 * t;
    }

    /// The neck snaps: the wings pull back into the notch and the blob
    /// springs to the pointer.
    fn break_free(&mut self, stage: &Stage, p: P) {
        self.mode = Mode::Free;
        self.tether = None;
        self.wings.target = 0.0;
        self.size.target = 1.0;
        let target = stage.clamp_blob(p);
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
    pub fn release(&mut self, stage: &Stage) -> Release {
        let Some(press) = self.press.take() else {
            return Release::None;
        };
        self.tether = None;
        if !press.moved {
            return Release::Click;
        }
        match self.mode {
            Mode::Docked => {
                // Let go of the neck: it springs back into the wings.
                self.size.target = 0.0;
                self.pos.tune(SNAP.0, SNAP.1);
                self.pos.set_target(stage.wings().center());
            }
            Mode::Free => {
                self.home = stage.clamp_blob(self.pos.target());
                self.pos.tune(SETTLE.0, SETTLE.1);
                self.pos.set_target(self.home);
            }
        }
        Release::Dropped
    }

    /// The display changed: keep the blob on it.
    pub fn restage(&mut self, stage: &Stage) {
        self.home = stage.clamp_blob(self.home);
        if self.mode == Mode::Free && self.press.is_none() {
            self.pos.set_target(self.home);
        } else if self.mode == Mode::Docked && self.press.is_none() {
            self.pos.set_target(stage.wings().center());
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
            self.wings.step(h);
            self.expand.step(h);
            left -= h;
        }
    }

    fn settle(&mut self) {
        self.pos.settle();
        self.squash.set_target(P::new(1.0, 1.0));
        self.squash.settle();
        self.size.settle();
        self.wings.settle();
        self.expand.settle();
    }

    /// Something still moves (keep drawing frames).
    pub fn animating(&self) -> bool {
        self.animate
            && (self.dragging()
                || !self.pos.at_rest()
                || !self.squash.at_rest()
                || !self.size.at_rest()
                || !self.wings.at_rest()
                || !self.expand.at_rest())
    }

    // -- what to draw -------------------------------------------------------

    /// The blob's rectangle, if any of it shows.
    pub fn blob(&self, stage: &Stage) -> Option<R> {
        let size = self.size.value.max(0.0);
        if size < 0.04 || (self.mode == Mode::Free && self.expanded()) {
            return None;
        }
        let (w, h) = stage.blob_size();
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

    /// The wings (or the docked panel growing out of them), if out at all.
    pub fn wings(&self, stage: &Stage) -> Option<R> {
        let w = self.wings.value.max(0.0);
        if w < 0.02 {
            return None;
        }
        let rect = R::lerp(stage.housing(), stage.wings(), w);
        // The panel only grows out of docked wings.
        let e = if self.mode == Mode::Docked {
            self.expand.value.clamp(0.0, 1.2)
        } else {
            0.0
        };
        Some(R::lerp(rect, stage.docked_panel(), e))
    }

    /// How much the wings' controls show (out, and not turning into the
    /// panel).
    pub fn wings_content(&self) -> f32 {
        let out = ((self.wings.value - 0.7) / 0.3).clamp(0.0, 1.0);
        let collapsed = if self.mode == Mode::Docked {
            1.0 - (self.expand.value / 0.3).clamp(0.0, 1.0)
        } else {
            1.0
        };
        out * collapsed
    }

    /// How much the docked panel's content shows.
    pub fn docked_panel_content(&self) -> f32 {
        if self.mode != Mode::Docked {
            return 0.0;
        }
        ((self.expand.value - 0.75) / 0.25).clamp(0.0, 1.0)
    }

    /// The free panel, when expanded while free.
    pub fn free_panel(&self, stage: &Stage) -> Option<R> {
        (self.mode == Mode::Free && self.expanded()).then(|| stage.free_panel(self.pos.target()))
    }

    /// The liquid bridge between the wings and the blob, if any.
    pub fn neck(&self, stage: &Stage) -> Option<Neck> {
        let blob = self.blob(stage)?;
        let b = blob.center();
        let r = blob.w.min(blob.h) / 2.0;
        match self.mode {
            Mode::Docked => {
                // Out of the wings (pulled, or springing back in).
                let wings = self.wings(stage)?;
                let a = stage.anchor(b);
                if b.y - r <= wings.bottom() - r * 0.5 {
                    return None;
                }
                let t = (b.dist(a) / stage.d(DETACH)).clamp(0.0, 1.0);
                Some(Neck {
                    a,
                    b,
                    half_a: r * (1.35 - 0.5 * t),
                    half_b: r * 0.9,
                    waist: r * (0.78 - 0.58 * t),
                })
            }
            Mode::Free => {
                // Dragged near: the wings reach out for it.
                if !self.dragging() {
                    return None;
                }
                let a = stage.anchor(b);
                let reach = stage.d(REACH);
                let t = 1.0 - (b.dist(a) - r) / reach;
                if t <= 0.0 {
                    return None;
                }
                let t = t.clamp(0.0, 1.0);
                Some(Neck {
                    a,
                    b,
                    half_a: r * 0.75 * t,
                    half_b: r * 0.55 * t,
                    waist: r * 0.16 * t * t,
                })
            }
        }
    }

    /// Whether the floater takes the pointer at `p` (elsewhere clicks go
    /// through to the apps beneath).
    pub fn hit(&self, stage: &Stage, p: P) -> bool {
        if self.press.is_some() {
            return true;
        }
        let pad = stage.d(4.0);
        if let Some(panel) = self.free_panel(stage) {
            return panel.inflate(pad).contains(p);
        }
        if let Some(w) = self.wings(stage).filter(|_| self.mode == Mode::Docked) {
            return w.inflate(pad).contains(p);
        }
        self.blob(stage).is_some_and(|b| b.inflate(pad).contains(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage() -> Stage {
        Stage::fallback(1.2)
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
    fn drag(f: &mut Floater, s: &Stage, from: P, to: P) -> Vec<Mode> {
        let mut changes = Vec::new();
        f.press(from);
        for i in 1..=40 {
            let p = from + (to - from) * (i as f32 / 40.0);
            changes.extend(f.drag_to(s, p));
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
        let s = stage();
        let mut f = Floater::new(&s, false);
        let home = f.blob_center();
        assert!(home.x > s.width - s.blob_size().0);
        assert!(f.hit(&s, home));
        assert!(
            !f.hit(&s, P::new(100.0, 500.0)),
            "clicks elsewhere go through"
        );
        f.press(home);
        assert_eq!(f.release(&s), Release::Click);
        assert_eq!(f.release(&s), Release::None);
        assert!(f.wings(&s).is_none());
        assert_eq!(f.blob_content(), 1.0);
    }

    #[test]
    fn a_drag_moves_it_and_it_stays_where_dropped() {
        let s = stage();
        let mut f = Floater::new(&s, false);
        let from = f.blob_center();
        let to = P::new(400.0, 600.0);
        assert!(drag(&mut f, &s, from, to).is_empty());
        assert!(f.dragging());
        assert_eq!(f.release(&s), Release::Dropped);
        run(&mut f, 2.0);
        assert!(f.blob_center().dist(to) < 1.0, "{:?}", f.blob_center());
        assert!(!f.animating());
        // Kept on screen, below the menu bar.
        let mut f = Floater::new(&s, false);
        let from = f.blob_center();
        drag(&mut f, &s, from, P::new(-500.0, 5_000.0));
        f.release(&s);
        run(&mut f, 2.0);
        let c = f.blob_center();
        assert!(c.x > 0.0 && c.y < s.height, "{c:?}");
    }

    #[test]
    fn dragged_near_the_notch_it_reaches_out_then_snaps_in() {
        let s = stage();
        let mut f = Floater::new(&s, false);
        let wings = s.wings();
        let from = f.blob_center();
        // Getting close: a bridge, still free.
        let near = P::new(
            wings.center().x,
            wings.bottom() + s.d(ATTACH) + s.blob_size().1 + 40.0,
        );
        assert!(drag(&mut f, &s, from, near).is_empty());
        assert_eq!(f.mode, Mode::Free);
        assert!(f.neck(&s).is_some(), "a bridge reaches for it");
        // Closer: docked while still held.
        let changes = drag(
            &mut f,
            &s,
            near,
            P::new(wings.center().x, wings.bottom() + 10.0),
        );
        assert_eq!(changes, vec![Mode::Docked]);
        f.release(&s);
        run(&mut f, 2.0);
        assert_eq!(f.mode, Mode::Docked);
        assert_eq!(f.wings(&s), Some(wings));
        assert!(f.blob(&s).is_none(), "absorbed");
        assert_eq!(f.wings_content(), 1.0);
        assert!(f.hit(&s, wings.center()));
    }

    #[test]
    fn pulling_the_wings_stretches_a_neck_until_it_breaks() {
        let s = stage();
        let mut f = Floater::new(&s, true);
        let wings = s.wings();
        let grab = wings.center();
        // A short pull: a neck, still docked; let go and it springs back.
        let short = P::new(grab.x, wings.bottom() + s.d(DETACH) * 0.6);
        assert!(drag(&mut f, &s, grab, short).is_empty());
        run(&mut f, 0.3);
        let neck = f.neck(&s).expect("a neck");
        assert!(neck.waist < neck.half_a, "it thins as it stretches");
        assert_eq!(f.release(&s), Release::Dropped);
        run(&mut f, 2.0);
        assert_eq!(f.mode, Mode::Docked);
        assert!(f.blob(&s).is_none());
        // A long pull breaks it: free, under the pointer, wings back in.
        let far = P::new(grab.x - 100.0, wings.bottom() + s.d(DETACH) + 80.0);
        let changes = drag(&mut f, &s, grab, far);
        assert_eq!(changes, vec![Mode::Free]);
        f.release(&s);
        run(&mut f, 2.0);
        assert!(f.blob_center().dist(far) < 1.0, "{:?}", f.blob_center());
        assert!(f.wings(&s).is_none());
        assert_eq!(f.blob_content(), 1.0);
    }

    #[test]
    fn a_click_on_the_wings_expands_the_notch_into_a_panel() {
        let s = stage();
        let mut f = Floater::new(&s, true);
        f.press(s.wings().center());
        assert_eq!(f.release(&s), Release::Click);
        f.set_expanded(true);
        run(&mut f, 2.0);
        let panel = s.docked_panel();
        let w = f.wings(&s).unwrap();
        assert!((w.w - panel.w).abs() < 0.5 && (w.h - panel.h).abs() < 0.5);
        assert_eq!(w.y, 0.0, "attached to the top");
        assert_eq!(f.docked_panel_content(), 1.0);
        assert_eq!(f.wings_content(), 0.0);
        assert!(f.hit(&s, P::new(panel.center().x, panel.bottom() - 5.0)));
        // Expanded, it isn't pulled out.
        f.press(s.wings().center());
        assert!(f.drag_to(&s, P::new(700.0, 700.0)).is_none());
        assert_eq!(f.mode, Mode::Docked);
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
        let s = stage();
        let mut f = Floater::new(&s, false);
        f.set_docked(&s, true);
        run(&mut f, 2.0);
        assert_eq!(f.wings(&s), Some(s.wings()));
        f.set_docked(&s, false);
        run(&mut f, 2.0);
        assert!(f.wings(&s).is_none());
        assert!(f.blob_center().dist(s.home()) < 1.0);
    }

    #[test]
    fn without_animation_everything_jumps() {
        let s = stage();
        let mut f = Floater::new(&s, false);
        f.animate = false;
        f.set_docked(&s, true);
        f.step(0.0);
        assert_eq!(f.wings(&s), Some(s.wings()));
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
