//! Single-key dictation gesture recognition.
//!
//! A pure reducer that turns raw key events for one modifier binding (Fn,
//! Option, Right Ctrl, ...) into gesture intents: hold to speak, double-press
//! to latch. It owns gestures only; the transcription coordinator owns
//! sessions, including what "latched" means.
//!
//! Time is passed in, so tests drive the exact production transitions. The
//! driver calls [`ModifierGesture::tick`] from its poll loop and before each
//! event, then [`ModifierGesture::on_event`].
//!
//! ```text
//! Idle ──bare press──▶ FirstDown ──ARM──▶ Holding ──release──▶ Idle
//!                          │ release before ARM
//!                          ▼
//!                     AwaitSecond ──press in window──▶ SecondDown
//!                          │ window expires               │ release before ARM: DoublePress
//!                          ▼                              │ ARM: Holding
//!                         Idle                            ▼
//!                                                        Idle
//! ```
//!
//! Anything that shows the modifier is part of a chord sends the gesture to
//! `Dirty`, which waits for the binding to come up before listening again.

use key_listener::{Hotkey, Key, KeyEvent, Modifiers};
use std::time::{Duration, Instant};

/// How long the key must stay down before dictation starts. Recording begins
/// here rather than at key-down so Option+C never flashes the overlay or plays
/// a sound.
pub const ARM: Duration = Duration::from_millis(200);
/// How long after a tap's release a second press still counts as a double.
pub const DOUBLE_WINDOW: Duration = Duration::from_millis(300);
/// Holds shorter than this, measured from the press, are discarded.
pub const SHORT_HOLD: Duration = Duration::from_millis(350);

/// What a gesture asks the coordinator to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureIntent {
    /// The key has been down past [`ARM`]: start speaking.
    HoldStart,
    /// The key came up after a real hold: stop and transcribe.
    HoldEnd,
    /// Two quick taps: latch on, or stop a latched session.
    DoublePress,
    /// The hold was accidental or became a chord: discard it.
    Abort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    FirstDown { pressed_at: Instant },
    AwaitSecond { released_at: Instant },
    SecondDown { pressed_at: Instant },
    Holding { pressed_at: Instant },
    Dirty,
}

/// Gesture recognizer for one single-modifier binding.
#[derive(Debug, Clone)]
pub struct ModifierGesture {
    binding: Modifiers,
    state: State,
}

impl ModifierGesture {
    pub fn new(binding: Modifiers) -> Self {
        Self {
            binding,
            state: State::Idle,
        }
    }

    /// Whether dictation is being held. The stuck-hold watchdog runs only then.
    pub fn is_holding(&self) -> bool {
        matches!(self.state, State::Holding { .. })
    }

    /// Advance the time-driven transitions: arming a hold and expiring the
    /// double-press window.
    pub fn tick(&mut self, now: Instant) -> Option<GestureIntent> {
        match self.state {
            State::FirstDown { pressed_at } | State::SecondDown { pressed_at }
                if now.saturating_duration_since(pressed_at) >= ARM =>
            {
                self.state = State::Holding { pressed_at };
                Some(GestureIntent::HoldStart)
            }
            State::AwaitSecond { released_at }
                if now.saturating_duration_since(released_at) >= DOUBLE_WINDOW =>
            {
                self.state = State::Idle;
                None
            }
            _ => None,
        }
    }

    /// Feed one key event from the listener.
    pub fn on_event(&mut self, event: &KeyEvent, now: Instant) -> Option<GestureIntent> {
        let held = self.is_held(event.modifiers);
        match self.state {
            State::Idle => {
                if self.is_press_edge(event, held) {
                    self.state = if self.binding.matches(event.modifiers) {
                        State::FirstDown { pressed_at: now }
                    } else {
                        State::Dirty
                    };
                } else if held && self.is_extra_modifier(event) {
                    self.state = State::Dirty;
                }
                None
            }
            State::FirstDown { pressed_at } => {
                if self.is_chord_signal(event) {
                    self.state = State::Dirty;
                } else if !held {
                    self.state = if now.saturating_duration_since(pressed_at) < ARM {
                        State::AwaitSecond { released_at: now }
                    } else {
                        // Only reachable if the driver missed a tick. Nothing
                        // started, so there is nothing to end.
                        State::Idle
                    };
                }
                None
            }
            State::AwaitSecond { released_at } => {
                if self.is_chord_signal(event) {
                    self.state = State::Dirty;
                } else if self.is_press_edge(event, held) {
                    self.state = if !self.binding.matches(event.modifiers) {
                        State::Dirty
                    } else if now.saturating_duration_since(released_at) < DOUBLE_WINDOW {
                        State::SecondDown { pressed_at: now }
                    } else {
                        State::FirstDown { pressed_at: now }
                    };
                }
                None
            }
            State::SecondDown { pressed_at } => {
                if self.is_chord_signal(event) {
                    self.state = State::Dirty;
                    None
                } else if !held {
                    self.state = State::Idle;
                    (now.saturating_duration_since(pressed_at) < ARM)
                        .then_some(GestureIntent::DoublePress)
                } else {
                    None
                }
            }
            State::Holding { pressed_at } => {
                // Release first: a key-down that already shows the binding up
                // means the release was missed, not that a chord began.
                if !held {
                    Some(self.finish_hold(pressed_at, now))
                } else if self.is_chord_key_while_holding(event) {
                    self.state = State::Dirty;
                    Some(GestureIntent::Abort)
                } else {
                    None
                }
            }
            State::Dirty => {
                if !held {
                    self.state = State::Idle;
                }
                None
            }
        }
    }

    /// The OS reports the binding up though no release event arrived. Only a
    /// hold can be stuck this way; the other states end on their own.
    pub fn on_missed_release(&mut self, now: Instant) -> Option<GestureIntent> {
        match self.state {
            State::Holding { pressed_at } => Some(self.finish_hold(pressed_at, now)),
            _ => None,
        }
    }

    /// When the first tap of a possible double-press was released, if one is
    /// waiting. The listeners never report a click with no modifier held, so
    /// the driver uses this to ask the OS whether one happened between taps.
    pub fn awaiting_second_since(&self) -> Option<Instant> {
        match self.state {
            State::AwaitSecond { released_at } => Some(released_at),
            _ => None,
        }
    }

    /// The driver learned of a click after the first tap. The binding is up,
    /// so this skips `Dirty`: the press that follows starts a new gesture.
    pub fn on_click_between_taps(&mut self) {
        if matches!(self.state, State::AwaitSecond { .. }) {
            self.state = State::Idle;
        }
    }

    /// The binding is going away (unregister or suspend). A hold in progress
    /// must not leave a recording running with no key to end it.
    pub fn reset(&mut self) -> Option<GestureIntent> {
        let was_holding = self.is_holding();
        self.state = State::Idle;
        was_holding.then_some(GestureIntent::Abort)
    }

    fn finish_hold(&mut self, pressed_at: Instant, now: Instant) -> GestureIntent {
        self.state = State::Idle;
        if now.saturating_duration_since(pressed_at) >= SHORT_HOLD {
            GestureIntent::HoldEnd
        } else {
            GestureIntent::Abort
        }
    }

    /// Whether the binding's modifier is down. Level-based: a release is
    /// recognized from any event, which also covers Fn, whose state only the
    /// event flags report reliably.
    fn is_held(&self, modifiers: Modifiers) -> bool {
        modifiers.intersects(self.binding)
    }

    /// A press of the bound modifier itself. Only key-less events qualify,
    /// because arrow and F-keys carry a false Fn flag. Fn ignores `is_key_down`:
    /// handy-keys derives it by toggling a bit that a lost event inverts, while
    /// the flags in `modifiers` stay correct.
    fn is_press_edge(&self, event: &KeyEvent, held: bool) -> bool {
        event.key.is_none()
            && held
            && (event.is_key_down || self.binding == Modifiers::FN)
            && event
                .changed_modifier
                .is_some_and(|changed| changed.intersects(self.binding))
    }

    /// A different modifier going down. Fn is read from the flags for the same
    /// reason as in [`Self::is_press_edge`].
    fn is_extra_modifier(&self, event: &KeyEvent) -> bool {
        event.key.is_none()
            && event.changed_modifier.is_some_and(|changed| {
                let going_down = if changed == Modifiers::FN {
                    event.modifiers.contains(Modifiers::FN)
                } else {
                    event.is_key_down
                };
                going_down && !changed.intersects(self.binding)
            })
    }

    /// Before a hold starts, a key, click or extra modifier means the press
    /// belongs to a chord.
    fn is_chord_signal(&self, event: &KeyEvent) -> bool {
        self.is_extra_modifier(event) || self.is_key_down(event)
    }

    /// During a hold only a real key is a chord: extra modifiers and clicks
    /// are ignored, so the user can keep working while speaking.
    fn is_chord_key_while_holding(&self, event: &KeyEvent) -> bool {
        self.is_key_down(event) && !event.key.is_some_and(is_mouse_button)
    }

    fn is_key_down(&self, event: &KeyEvent) -> bool {
        event.is_key_down
            && event.key.is_some()
            && !(self.binding == Modifiers::FN && event.key == Some(Key::Dictation))
    }
}

fn is_mouse_button(key: Key) -> bool {
    matches!(
        key,
        Key::MouseLeft | Key::MouseRight | Key::MouseMiddle | Key::MouseX1 | Key::MouseX2
    )
}

/// The modifier a binding string reduces to, if it is exactly one physical
/// modifier: `fn`, `option`, `option_right`, `ctrl_left` and so on. Chords and
/// bare keys return `None`.
pub fn single_modifier_binding(binding: &str) -> Option<Modifiers> {
    let hotkey: Hotkey = binding.parse().ok()?;
    if hotkey.key.is_some() {
        return None;
    }
    const SINGLE: [Modifiers; 13] = [
        Modifiers::FN,
        Modifiers::OPT,
        Modifiers::OPT_LEFT,
        Modifiers::OPT_RIGHT,
        Modifiers::CTRL,
        Modifiers::CTRL_LEFT,
        Modifiers::CTRL_RIGHT,
        Modifiers::SHIFT,
        Modifiers::SHIFT_LEFT,
        Modifiers::SHIFT_RIGHT,
        Modifiers::CMD,
        Modifiers::CMD_LEFT,
        Modifiers::CMD_RIGHT,
    ];
    SINGLE
        .contains(&hotkey.modifiers)
        .then_some(hotkey.modifiers)
}

/// Whether a binding string is a single modifier, and so uses the gesture.
pub fn is_single_modifier_binding(binding: &str) -> bool {
    single_modifier_binding(binding).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// The bound modifier going down or up, as a flagsChanged event reports it.
    fn edge(changed: Modifiers, modifiers: Modifiers, is_key_down: bool) -> KeyEvent {
        KeyEvent {
            modifiers,
            key: None,
            is_key_down,
            changed_modifier: Some(changed),
        }
    }

    fn down(bit: Modifiers) -> KeyEvent {
        edge(bit, bit, true)
    }

    fn up(bit: Modifiers) -> KeyEvent {
        edge(bit, Modifiers::empty(), false)
    }

    fn key_down(key: Key, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            modifiers,
            key: Some(key),
            is_key_down: true,
            changed_modifier: None,
        }
    }

    fn key_up(key: Key, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            modifiers,
            key: Some(key),
            is_key_down: false,
            changed_modifier: None,
        }
    }

    const OPT: Modifiers = Modifiers::OPT_RIGHT;

    fn option() -> ModifierGesture {
        ModifierGesture::new(OPT)
    }

    #[test]
    fn tap_alone_emits_nothing() {
        let t0 = Instant::now();
        let mut g = option();
        assert_eq!(g.on_event(&down(OPT), t0), None);
        assert_eq!(g.tick(t0 + ms(50)), None);
        assert_eq!(g.on_event(&up(OPT), t0 + ms(80)), None);
        assert_eq!(
            g.state,
            State::AwaitSecond {
                released_at: t0 + ms(80)
            }
        );
        assert_eq!(g.tick(t0 + ms(80) + DOUBLE_WINDOW), None);
        assert_eq!(g.state, State::Idle);
    }

    #[test]
    fn hold_emits_start_after_arm_then_end() {
        let t0 = Instant::now();
        let mut g = option();
        g.on_event(&down(OPT), t0);
        assert_eq!(g.tick(t0 + ARM - ms(1)), None);
        assert_eq!(g.tick(t0 + ARM), Some(GestureIntent::HoldStart));
        assert!(g.is_holding());
        assert_eq!(g.tick(t0 + ms(900)), None);
        assert_eq!(
            g.on_event(&up(OPT), t0 + SHORT_HOLD),
            Some(GestureIntent::HoldEnd)
        );
        assert_eq!(g.state, State::Idle);
    }

    #[test]
    fn hold_under_short_hold_aborts() {
        let t0 = Instant::now();
        let mut g = option();
        g.on_event(&down(OPT), t0);
        assert_eq!(g.tick(t0 + ARM), Some(GestureIntent::HoldStart));
        assert_eq!(
            g.on_event(&up(OPT), t0 + SHORT_HOLD - ms(1)),
            Some(GestureIntent::Abort)
        );
        assert_eq!(g.state, State::Idle);
    }

    #[test]
    fn double_press_within_window_emits_once() {
        let t0 = Instant::now();
        let mut g = option();
        assert_eq!(g.on_event(&down(OPT), t0), None);
        assert_eq!(g.on_event(&up(OPT), t0 + ms(60)), None);
        assert_eq!(g.on_event(&down(OPT), t0 + ms(150)), None);
        assert_eq!(
            g.on_event(&up(OPT), t0 + ms(210)),
            Some(GestureIntent::DoublePress)
        );
        assert_eq!(g.state, State::Idle);
        assert_eq!(g.tick(t0 + ms(2000)), None);
    }

    #[test]
    fn press_after_window_is_a_new_gesture() {
        let t0 = Instant::now();
        let mut g = option();
        g.on_event(&down(OPT), t0);
        g.on_event(&up(OPT), t0 + ms(50));
        // The press lands past the window before any tick expired it.
        g.on_event(&down(OPT), t0 + ms(50) + DOUBLE_WINDOW);
        assert_eq!(
            g.state,
            State::FirstDown {
                pressed_at: t0 + ms(50) + DOUBLE_WINDOW
            }
        );
        assert_eq!(
            g.on_event(&up(OPT), t0 + ms(50) + DOUBLE_WINDOW + ms(50)),
            None
        );
    }

    #[test]
    fn second_press_held_is_a_hold() {
        let t0 = Instant::now();
        let mut g = option();
        g.on_event(&down(OPT), t0);
        g.on_event(&up(OPT), t0 + ms(50));
        let second = t0 + ms(150);
        g.on_event(&down(OPT), second);
        assert_eq!(g.tick(second + ARM), Some(GestureIntent::HoldStart));
        // SHORT_HOLD counts from the second press, not the first.
        assert_eq!(
            g.on_event(&up(OPT), second + SHORT_HOLD - ms(1)),
            Some(GestureIntent::Abort)
        );
    }

    #[test]
    fn option_c_in_first_down_goes_dirty_until_release() {
        let t0 = Instant::now();
        let mut g = option();
        g.on_event(&down(OPT), t0);
        assert_eq!(g.on_event(&key_down(Key::C, OPT), t0 + ms(50)), None);
        assert_eq!(g.state, State::Dirty);
        // Still held well past ARM: nothing ever starts.
        assert_eq!(g.tick(t0 + ms(400)), None);
        assert_eq!(g.on_event(&key_up(Key::C, OPT), t0 + ms(450)), None);
        assert_eq!(g.state, State::Dirty);
        assert_eq!(g.on_event(&up(OPT), t0 + ms(500)), None);
        assert_eq!(g.state, State::Idle);
    }

    #[test]
    fn option_c_after_arm_aborts_the_hold() {
        let t0 = Instant::now();
        let mut g = option();
        g.on_event(&down(OPT), t0);
        assert_eq!(g.tick(t0 + ms(210)), Some(GestureIntent::HoldStart));
        assert_eq!(
            g.on_event(&key_down(Key::C, OPT), t0 + ms(400)),
            Some(GestureIntent::Abort)
        );
        assert_eq!(g.on_event(&up(OPT), t0 + ms(500)), None);
        assert_eq!(g.state, State::Idle);
    }

    #[test]
    fn key_while_holding_aborts() {
        let t0 = Instant::now();
        let mut g = option();
        g.on_event(&down(OPT), t0);
        g.tick(t0 + ARM);
        assert_eq!(
            g.on_event(&key_down(Key::Space, OPT), t0 + ms(800)),
            Some(GestureIntent::Abort)
        );
        assert_eq!(g.state, State::Dirty);
    }

    #[test]
    fn extra_modifier_while_holding_is_ignored() {
        let t0 = Instant::now();
        let mut g = option();
        g.on_event(&down(OPT), t0);
        g.tick(t0 + ARM);
        let both = OPT | Modifiers::SHIFT_LEFT;
        assert_eq!(
            g.on_event(&edge(Modifiers::SHIFT_LEFT, both, true), t0 + ms(400)),
            None
        );
        assert_eq!(
            g.on_event(&key_down(Key::MouseLeft, both), t0 + ms(450)),
            None
        );
        assert!(g.is_holding());
        assert_eq!(
            g.on_event(&edge(OPT, Modifiers::SHIFT_LEFT, false), t0 + ms(800)),
            Some(GestureIntent::HoldEnd)
        );
    }

    #[test]
    fn key_during_await_second_blocks_double() {
        let t0 = Instant::now();
        let mut g = option();
        g.on_event(&down(OPT), t0);
        g.on_event(&up(OPT), t0 + ms(50));
        g.on_event(&key_down(Key::C, Modifiers::empty()), t0 + ms(100));
        assert_eq!(g.state, State::Dirty);
        g.on_event(&key_up(Key::C, Modifiers::empty()), t0 + ms(120));
        assert_eq!(g.state, State::Idle);
        g.on_event(&down(OPT), t0 + ms(150));
        assert_eq!(g.on_event(&up(OPT), t0 + ms(200)), None);
    }

    #[test]
    fn click_and_extra_modifier_before_arm_go_dirty() {
        let t0 = Instant::now();

        let mut g = option();
        g.on_event(&down(OPT), t0);
        g.on_event(&key_down(Key::MouseLeft, OPT), t0 + ms(50));
        assert_eq!(g.state, State::Dirty);

        let mut g = option();
        g.on_event(&down(OPT), t0);
        g.on_event(
            &edge(Modifiers::SHIFT_LEFT, OPT | Modifiers::SHIFT_LEFT, true),
            t0 + ms(50),
        );
        assert_eq!(g.state, State::Dirty);

        let mut g = option();
        g.on_event(&down(OPT), t0);
        g.on_event(&up(OPT), t0 + ms(50));
        g.on_event(&down(OPT), t0 + ms(100));
        g.on_event(&key_down(Key::C, OPT), t0 + ms(120));
        assert_eq!(g.state, State::Dirty);
        assert_eq!(g.on_event(&up(OPT), t0 + ms(150)), None);
        assert_eq!(g.state, State::Idle);
    }

    #[test]
    fn binding_pressed_with_another_modifier_held_goes_dirty() {
        let t0 = Instant::now();
        let mut g = option();
        let both = OPT | Modifiers::SHIFT_LEFT;
        g.on_event(
            &edge(Modifiers::SHIFT_LEFT, Modifiers::SHIFT_LEFT, true),
            t0,
        );
        assert_eq!(g.state, State::Idle);
        g.on_event(&edge(OPT, both, true), t0 + ms(20));
        assert_eq!(g.state, State::Dirty);
        assert_eq!(g.tick(t0 + ms(500)), None);
        g.on_event(&edge(OPT, Modifiers::SHIFT_LEFT, false), t0 + ms(600));
        assert_eq!(g.state, State::Idle);
    }

    #[test]
    fn arrow_fn_flag_never_presses_fn_binding() {
        let t0 = Instant::now();
        let mut g = ModifierGesture::new(Modifiers::FN);
        g.on_event(&key_down(Key::LeftArrow, Modifiers::FN), t0);
        assert_eq!(g.state, State::Idle);
        g.on_event(&key_up(Key::LeftArrow, Modifiers::FN), t0 + ms(30));
        assert_eq!(g.state, State::Idle);
        assert_eq!(g.tick(t0 + ms(500)), None);
    }

    #[test]
    fn dictation_key_is_not_a_chord_for_fn() {
        let t0 = Instant::now();
        let mut g = ModifierGesture::new(Modifiers::FN);
        g.on_event(&down(Modifiers::FN), t0);
        g.on_event(&key_down(Key::Dictation, Modifiers::FN), t0 + ms(30));
        assert_eq!(g.state, State::FirstDown { pressed_at: t0 });
        assert_eq!(g.tick(t0 + ARM), Some(GestureIntent::HoldStart));
        assert_eq!(
            g.on_event(&key_down(Key::Dictation, Modifiers::FN), t0 + ms(300)),
            None
        );
        assert!(g.is_holding());

        // For any other binding it is a key like the rest.
        let mut g = option();
        g.on_event(&down(OPT), t0);
        g.on_event(&key_down(Key::Dictation, OPT), t0 + ms(30));
        assert_eq!(g.state, State::Dirty);
    }

    #[test]
    fn fn_release_is_level_based_and_press_survives_inverted_edge() {
        let t0 = Instant::now();
        let mut g = ModifierGesture::new(Modifiers::FN);
        // A lost event inverted handy-keys' toggle: the press reads as key-up.
        g.on_event(&edge(Modifiers::FN, Modifiers::FN, false), t0);
        assert_eq!(g.state, State::FirstDown { pressed_at: t0 });
        assert_eq!(g.tick(t0 + ARM), Some(GestureIntent::HoldStart));
        // And the release reads as key-down; the flags still say Fn is up.
        assert_eq!(
            g.on_event(&edge(Modifiers::FN, Modifiers::empty(), true), t0 + ms(500)),
            Some(GestureIntent::HoldEnd)
        );
    }

    #[test]
    fn right_binding_ignores_left_side() {
        let t0 = Instant::now();
        let mut g = ModifierGesture::new(Modifiers::OPT_RIGHT);
        g.on_event(&down(Modifiers::OPT_LEFT), t0);
        assert_eq!(g.state, State::Idle);
        assert_eq!(g.tick(t0 + ms(500)), None);
        g.on_event(&up(Modifiers::OPT_LEFT), t0 + ms(600));
        assert_eq!(g.state, State::Idle);
    }

    #[test]
    fn hold_release_is_not_a_first_tap() {
        let t0 = Instant::now();
        let mut g = option();
        g.on_event(&down(OPT), t0);
        g.tick(t0 + ARM);
        assert_eq!(
            g.on_event(&up(OPT), t0 + ms(500)),
            Some(GestureIntent::HoldEnd)
        );
        // A quick tap right after is only a first tap, not a double.
        g.on_event(&down(OPT), t0 + ms(550));
        assert_eq!(g.on_event(&up(OPT), t0 + ms(600)), None);
        assert_eq!(
            g.state,
            State::AwaitSecond {
                released_at: t0 + ms(600)
            }
        );
    }

    #[test]
    fn missed_release_via_watchdog_ends_hold() {
        let t0 = Instant::now();
        let mut g = option();
        g.on_event(&down(OPT), t0);
        assert_eq!(g.on_missed_release(t0 + ms(100)), None);
        g.tick(t0 + ARM);
        assert_eq!(
            g.on_missed_release(t0 + ms(800)),
            Some(GestureIntent::HoldEnd)
        );
        assert_eq!(g.state, State::Idle);
        assert_eq!(g.on_missed_release(t0 + ms(900)), None);
    }

    #[test]
    fn inverted_fn_edge_is_still_an_extra_modifier() {
        let t0 = Instant::now();
        let mut g = option();
        g.on_event(&down(OPT), t0);
        // Fn pressed, but a lost event inverted handy-keys' toggle.
        let both = OPT | Modifiers::FN;
        g.on_event(&edge(Modifiers::FN, both, false), t0 + ms(50));
        assert_eq!(g.state, State::Dirty);
        assert_eq!(g.tick(t0 + ms(400)), None);

        // The mirror image: Fn released, but reported as a key-down.
        let mut g = option();
        g.on_event(&down(OPT), t0);
        g.on_event(&edge(Modifiers::FN, OPT, true), t0 + ms(50));
        assert_eq!(g.state, State::FirstDown { pressed_at: t0 });
    }

    #[test]
    fn click_between_taps_blocks_double_but_not_a_new_hold() {
        let t0 = Instant::now();
        let mut g = option();
        assert_eq!(g.awaiting_second_since(), None);
        g.on_event(&down(OPT), t0);
        g.on_click_between_taps();
        assert_eq!(g.state, State::FirstDown { pressed_at: t0 });
        g.on_event(&up(OPT), t0 + ms(50));
        assert_eq!(g.awaiting_second_since(), Some(t0 + ms(50)));
        g.on_click_between_taps();
        assert_eq!(g.state, State::Idle);

        g.on_event(&down(OPT), t0 + ms(100));
        assert_eq!(g.tick(t0 + ms(100) + ARM), Some(GestureIntent::HoldStart));
    }

    #[test]
    fn reset_aborts_only_a_hold() {
        let t0 = Instant::now();
        let mut g = option();
        g.on_event(&down(OPT), t0);
        assert_eq!(g.reset(), None);
        assert_eq!(g.state, State::Idle);
        g.on_event(&down(OPT), t0 + ms(10));
        g.tick(t0 + ms(10) + ARM);
        assert_eq!(g.reset(), Some(GestureIntent::Abort));
        assert_eq!(g.state, State::Idle);
    }

    #[test]
    fn single_modifier_bindings_are_recognized() {
        for binding in [
            "fn",
            "option",
            "option_left",
            "option_right",
            "alt_right",
            "ctrl",
            "ctrl_right",
            "shift_left",
            "command",
            "command_right",
        ] {
            assert!(is_single_modifier_binding(binding), "{binding}");
        }
        for binding in [
            "option+shift",
            "option+space",
            "f13",
            "fn+option",
            "ctrl_left+shift_right",
            "not_a_key",
            "",
        ] {
            assert!(!is_single_modifier_binding(binding), "{binding}");
        }
        assert_eq!(
            single_modifier_binding("option_right"),
            Some(Modifiers::OPT_RIGHT)
        );
    }
}
