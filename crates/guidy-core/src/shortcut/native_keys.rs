//! Native key listener shortcut implementation
//!
//! This module provides an alternative to Tauri's global-shortcut plugin
//! using a native key-listener library for more control over keyboard events.
//!
//! ## Architecture
//!
//! The implementation uses a dedicated manager thread that owns the `HotkeyManager`:
//!
//! ```text
//! ┌─────────────────┐     commands      ┌──────────────────────┐
//! │   Main Thread   │ ───────────────▶ │   Manager Thread     │
//! │                 │   (via channel)   │                      │
//! │ - register()    │                   │ - owns HotkeyManager │
//! │ - unregister()  │                   │ - polls for events   │
//! └─────────────────┘                   │ - dispatches actions │
//!                                       └──────────────────────┘
//! ```
//!
//! This design ensures thread-safety since `HotkeyManager` is only accessed
//! from a single thread. Commands (register/unregister) are sent via an mpsc
//! channel and responses are synchronously awaited.
//!
//! ## Single-Key Gestures
//!
//! A transcribe binding that is exactly one modifier skips `HotkeyManager`,
//! which reports only pressed/released and swallows the modifier's own event.
//! The manager thread polls a non-blocking `KeyboardListener` instead, created
//! with the first such binding and dropped with the last, and feeds it through
//! one [`ModifierGesture`] per binding. The coordinator receives the intents.
//!
//! ## Recording Mode
//!
//! For UI key capture, a separate `KeyboardListener` is created on-demand and
//! polled from a dedicated recording thread. Events are emitted to the frontend
//! via Tauri's event system.

use crate::host::AppHandle;
use key_listener::{Hotkey, HotkeyId, HotkeyManager, HotkeyState, KeyboardListener};
use log::{debug, error, info, warn};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::settings::{self, get_settings, ShortcutBinding};
use crate::transcription_coordinator::is_transcribe_binding;
use crate::TranscriptionCoordinator;

use super::gesture::{single_modifier_binding, GestureIntent, ModifierGesture};
use super::handler::handle_shortcut_event;

/// How often a hold checks that the OS still reports its modifier down.
const STUCK_HOLD_POLL: Duration = Duration::from_millis(500);

/// One single-modifier binding handled by the gesture listener.
struct GestureEntry {
    hotkey_string: String,
    modifiers: key_listener::Modifiers,
    gesture: ModifierGesture,
}

/// The gesture side of the manager thread.
struct GestureListener {
    entries: HashMap<String, GestureEntry>,
    /// Present exactly while `entries` is non-empty.
    listener: Option<KeyboardListener>,
    last_stuck_hold_poll: Instant,
}

impl GestureListener {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            listener: None,
            last_stuck_hold_poll: Instant::now(),
        }
    }

    fn register(
        &mut self,
        binding_id: &str,
        hotkey_string: &str,
        modifiers: key_listener::Modifiers,
    ) -> Result<(), String> {
        if self.entries.contains_key(binding_id) {
            return Err(format!("'{binding_id}' is already registered"));
        }
        self.reject_overlap(hotkey_string, modifiers)?;
        if self.listener.is_none() {
            let listener = KeyboardListener::new()
                .map_err(|e| format!("Failed to create keyboard listener: {}", e))?;
            self.listener = Some(listener);
        }
        self.entries.insert(
            binding_id.to_string(),
            GestureEntry {
                hotkey_string: hotkey_string.to_string(),
                modifiers,
                gesture: ModifierGesture::new(modifiers),
            },
        );
        Ok(())
    }

    /// Two gestures can't share a physical key. Option overlaps Right Option.
    fn reject_overlap(
        &self,
        hotkey_string: &str,
        modifiers: key_listener::Modifiers,
    ) -> Result<(), String> {
        match self
            .entries
            .iter()
            .find(|(_, entry)| entry.modifiers.intersects(modifiers))
        {
            Some((id, entry)) => Err(format!(
                "'{hotkey_string}' overlaps '{}' used by '{id}'",
                entry.hotkey_string
            )),
            None => Ok(()),
        }
    }

    /// Returns whether the binding was a gesture. A hold in progress is
    /// aborted, since its key can no longer end the recording.
    fn unregister(&mut self, app: &AppHandle, binding_id: &str) -> bool {
        let Some(mut entry) = self.entries.remove(binding_id) else {
            return false;
        };
        if let Some(intent) = entry.gesture.reset() {
            send_gesture(app, binding_id, &entry.hotkey_string, intent);
        }
        if self.entries.is_empty() {
            self.listener = None;
        }
        true
    }

    /// Drain listener events and advance timers. Called from the manager
    /// thread's loop, at most about 10 ms apart.
    fn poll(&mut self, app: &AppHandle) {
        let Some(listener) = &self.listener else {
            return;
        };
        let now = Instant::now();
        while let Some(event) = listener.try_recv() {
            for (binding_id, entry) in &mut self.entries {
                if let Some(intent) = entry.gesture.tick(now) {
                    send_gesture(app, binding_id, &entry.hotkey_string, intent);
                }
                // A modifier event may be the second press, so settle first
                // whether a click separated it from the first tap.
                if event.key.is_none() {
                    if let Some(released_at) = entry.gesture.awaiting_second_since() {
                        if click_within(now.saturating_duration_since(released_at)) {
                            entry.gesture.on_click_between_taps();
                        }
                    }
                }
                if let Some(intent) = entry.gesture.on_event(&event, now) {
                    send_gesture(app, binding_id, &entry.hotkey_string, intent);
                }
            }
        }
        for (binding_id, entry) in &mut self.entries {
            if let Some(intent) = entry.gesture.tick(now) {
                send_gesture(app, binding_id, &entry.hotkey_string, intent);
            }
        }
        self.end_stuck_holds(app, now);
    }

    /// handy-keys reconciles modifiers silently after its tap is disabled, so
    /// a release can be lost and a hold would record forever. Runs only while
    /// a hold is live.
    fn end_stuck_holds(&mut self, app: &AppHandle, now: Instant) {
        if !self.entries.values().any(|e| e.gesture.is_holding())
            || now.saturating_duration_since(self.last_stuck_hold_poll) < STUCK_HOLD_POLL
        {
            return;
        }
        self.last_stuck_hold_poll = now;
        for (binding_id, entry) in &mut self.entries {
            if entry.gesture.is_holding() && !modifier_down_per_os(entry.modifiers) {
                if let Some(intent) = entry.gesture.on_missed_release(now) {
                    send_gesture(app, binding_id, &entry.hotkey_string, intent);
                }
            }
        }
    }
}

fn send_gesture(app: &AppHandle, binding_id: &str, hotkey_string: &str, intent: GestureIntent) {
    debug!("gesture: binding={binding_id}, hotkey={hotkey_string}, intent={intent:?}");
    if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
        coordinator.send_gesture(binding_id, hotkey_string, intent);
    } else {
        warn!("TranscriptionCoordinator is not initialized");
    }
}

/// Which modifier group the OS must report for `modifiers` to count as down,
/// as `CGEventFlags` bits. Reads group flags only, so a stuck Right Option hold
/// also waits for Left Option, which still ends at the next listener event.
#[cfg(target_os = "macos")]
fn cg_flag_mask(modifiers: key_listener::Modifiers) -> u64 {
    use key_listener::Modifiers;
    const MASK_SHIFT: u64 = 0x0002_0000;
    const MASK_CONTROL: u64 = 0x0004_0000;
    const MASK_ALTERNATE: u64 = 0x0008_0000;
    const MASK_COMMAND: u64 = 0x0010_0000;
    const MASK_SECONDARY_FN: u64 = 0x0080_0000;

    let mut mask = 0;
    if modifiers.intersects(Modifiers::SHIFT) {
        mask |= MASK_SHIFT;
    }
    if modifiers.intersects(Modifiers::CTRL) {
        mask |= MASK_CONTROL;
    }
    if modifiers.intersects(Modifiers::OPT) {
        mask |= MASK_ALTERNATE;
    }
    if modifiers.intersects(Modifiers::CMD) {
        mask |= MASK_COMMAND;
    }
    if modifiers.intersects(Modifiers::FN) {
        mask |= MASK_SECONDARY_FN;
    }
    mask
}

#[cfg(target_os = "macos")]
const COMBINED_SESSION_STATE: i32 = 0;

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGEventSourceFlagsState(state_id: i32) -> u64;
    fn CGEventSourceSecondsSinceLastEventType(state_id: i32, event_type: u32) -> f64;
}

#[cfg(target_os = "macos")]
fn modifier_down_per_os(modifiers: key_listener::Modifiers) -> bool {
    // SAFETY: reads session state; takes no pointers and has no preconditions.
    let flags = unsafe { CGEventSourceFlagsState(COMBINED_SESSION_STATE) };
    flags & cg_flag_mask(modifiers) != 0
}

/// Only macOS can lose a release this way; elsewhere trust the listener.
#[cfg(not(target_os = "macos"))]
fn modifier_down_per_os(_modifiers: key_listener::Modifiers) -> bool {
    true
}

/// Whether a left or right click happened within `window`. The listeners never
/// report a click with no modifier held, so the OS is asked directly. Called
/// only between the taps of a possible double-press.
#[cfg(target_os = "macos")]
fn click_within(window: Duration) -> bool {
    const LEFT_MOUSE_DOWN: u32 = 1;
    const RIGHT_MOUSE_DOWN: u32 = 3;
    [LEFT_MOUSE_DOWN, RIGHT_MOUSE_DOWN]
        .into_iter()
        .any(|event_type| {
            // SAFETY: reads session state; takes no pointers and has no preconditions.
            let seconds = unsafe {
                CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION_STATE, event_type)
            };
            seconds < window.as_secs_f64()
        })
}

/// No equivalent query here, so a click between taps can't cancel a double.
#[cfg(not(target_os = "macos"))]
fn click_within(_window: Duration) -> bool {
    false
}

/// Commands that can be sent to the hotkey manager thread
enum ManagerCommand {
    Register {
        binding_id: String,
        hotkey_string: String,
        response: Sender<Result<(), String>>,
    },
    Unregister {
        binding_id: String,
        response: Sender<Result<(), String>>,
    },
    Shutdown,
}

/// State for the native-keys shortcut manager
pub struct NativeKeysState {
    /// Channel to send commands to the manager thread (wrapped in Mutex for Sync)
    command_sender: Mutex<Sender<ManagerCommand>>,
    /// Handle to the manager thread (wrapped in Mutex for Sync, allows proper join on drop)
    thread_handle: Mutex<Option<JoinHandle<()>>>,
    /// Recording listener for UI key capture (only active during recording)
    recording_listener: Mutex<Option<KeyboardListener>>,
    /// Flag indicating if we're in recording mode
    is_recording: AtomicBool,
    /// The binding ID being recorded (if any)
    recording_binding_id: Mutex<Option<String>>,
    /// Flag to stop recording loop
    recording_running: Arc<AtomicBool>,
}

/// Key event sent to frontend during recording mode
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct FrontendKeyEvent {
    /// Currently pressed modifier keys
    pub modifiers: Vec<String>,
    /// The key that was pressed (if any)
    pub key: Option<String>,
    /// Whether this is a key down event
    pub is_key_down: bool,
    /// The full hotkey string (e.g., "option+space")
    pub hotkey_string: String,
}

impl NativeKeysState {
    /// Create a new NativeKeysState
    pub fn new(app: AppHandle) -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ManagerCommand>();

        // Start the manager thread
        let app_clone = app.clone();
        let thread_handle = thread::spawn(move || {
            Self::manager_thread(cmd_rx, app_clone);
        });

        Ok(Self {
            command_sender: Mutex::new(cmd_tx),
            thread_handle: Mutex::new(Some(thread_handle)),
            recording_listener: Mutex::new(None),
            is_recording: AtomicBool::new(false),
            recording_binding_id: Mutex::new(None),
            recording_running: Arc::new(AtomicBool::new(false)),
        })
    }

    /// The main manager thread - owns the HotkeyManager and processes commands
    fn manager_thread(cmd_rx: Receiver<ManagerCommand>, app: AppHandle) {
        info!("native-keys manager thread started");

        // Create the HotkeyManager in this thread
        let manager = match HotkeyManager::new_with_blocking() {
            Ok(m) => m,
            Err(e) => {
                error!("Failed to create HotkeyManager: {}", e);
                return;
            }
        };

        // Maps binding IDs to HotkeyIds and hotkey strings
        let mut binding_to_hotkey: HashMap<String, HotkeyId> = HashMap::new();
        let mut hotkey_to_binding: HashMap<HotkeyId, (String, String)> = HashMap::new(); // (binding_id, hotkey_string)
        let mut gestures = GestureListener::new();

        loop {
            gestures.poll(&app);

            // Check for hotkey events (non-blocking)
            while let Some(event) = manager.try_recv() {
                if let Some((binding_id, hotkey_string)) = hotkey_to_binding.get(&event.id) {
                    debug!(
                        "native-keys event: binding={}, hotkey={}, state={:?}",
                        binding_id, hotkey_string, event.state
                    );
                    let is_pressed = event.state == HotkeyState::Pressed;
                    handle_shortcut_event(&app, binding_id, hotkey_string, is_pressed);
                }
            }

            // Check for commands (non-blocking with timeout)
            match cmd_rx.recv_timeout(std::time::Duration::from_millis(10)) {
                Ok(cmd) => match cmd {
                    ManagerCommand::Register {
                        binding_id,
                        hotkey_string,
                        response,
                    } => {
                        let result = Self::do_register(
                            &manager,
                            &mut binding_to_hotkey,
                            &mut hotkey_to_binding,
                            &mut gestures,
                            &binding_id,
                            &hotkey_string,
                        );
                        let _ = response.send(result);
                    }
                    ManagerCommand::Unregister {
                        binding_id,
                        response,
                    } => {
                        let result = Self::do_unregister(
                            &app,
                            &manager,
                            &mut binding_to_hotkey,
                            &mut hotkey_to_binding,
                            &mut gestures,
                            &binding_id,
                        );
                        let _ = response.send(result);
                    }
                    ManagerCommand::Shutdown => {
                        info!("native-keys manager thread shutting down");
                        break;
                    }
                },
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // No command, continue
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    info!("Command channel disconnected, shutting down");
                    break;
                }
            }
        }

        info!("native-keys manager thread stopped");
    }

    /// Register a hotkey
    fn do_register(
        manager: &HotkeyManager,
        binding_to_hotkey: &mut HashMap<String, HotkeyId>,
        hotkey_to_binding: &mut HashMap<HotkeyId, (String, String)>,
        gestures: &mut GestureListener,
        binding_id: &str,
        hotkey_string: &str,
    ) -> Result<(), String> {
        let hotkey: Hotkey = hotkey_string
            .parse()
            .map_err(|e| format!("Failed to parse hotkey '{}': {}", hotkey_string, e))?;

        let single_modifier = single_modifier_binding(hotkey_string);
        if is_transcribe_binding(binding_id) {
            if let Some(modifiers) = single_modifier {
                // Same "already registered" failure the manager gives below,
                // which resume_all_shortcuts relies on being harmless.
                if binding_to_hotkey.contains_key(binding_id) {
                    return Err(format!("'{binding_id}' is already registered"));
                }
                gestures.register(binding_id, hotkey_string, modifiers)?;
                debug!("Registered gesture shortcut: {binding_id} -> {hotkey_string}");
                return Ok(());
            }
            // Also registered as a gesture: same id, different binding kind.
            if gestures.entries.contains_key(binding_id) {
                return Err(format!("'{binding_id}' is already registered"));
            }
        } else if let Some(modifiers) = single_modifier {
            // The blocking manager would swallow the gesture's modifier.
            gestures.reject_overlap(hotkey_string, modifiers)?;
        }

        let id = manager
            .register(hotkey)
            .map_err(|e| format!("Failed to register hotkey: {}", e))?;

        binding_to_hotkey.insert(binding_id.to_string(), id);
        hotkey_to_binding.insert(id, (binding_id.to_string(), hotkey_string.to_string()));

        debug!(
            "Registered native-keys shortcut: {} -> {:?}",
            binding_id, hotkey
        );
        Ok(())
    }

    /// Unregister a hotkey
    fn do_unregister(
        app: &AppHandle,
        manager: &HotkeyManager,
        binding_to_hotkey: &mut HashMap<String, HotkeyId>,
        hotkey_to_binding: &mut HashMap<HotkeyId, (String, String)>,
        gestures: &mut GestureListener,
        binding_id: &str,
    ) -> Result<(), String> {
        if gestures.unregister(app, binding_id) {
            debug!("Unregistered gesture shortcut: {}", binding_id);
            return Ok(());
        }
        if let Some(id) = binding_to_hotkey.remove(binding_id) {
            manager
                .unregister(id)
                .map_err(|e| format!("Failed to unregister hotkey: {}", e))?;
            hotkey_to_binding.remove(&id);
            debug!("Unregistered native-keys shortcut: {}", binding_id);
        }
        Ok(())
    }

    /// Register a shortcut binding
    pub fn register(&self, binding: &ShortcutBinding) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.command_sender
            .lock()
            .map_err(|_| "Failed to lock command_sender")?
            .send(ManagerCommand::Register {
                binding_id: binding.id.clone(),
                hotkey_string: binding.current_binding.clone(),
                response: tx,
            })
            .map_err(|_| "Failed to send register command")?;

        rx.recv()
            .map_err(|_| "Failed to receive register response")?
    }

    /// Unregister a shortcut binding
    pub fn unregister(&self, binding: &ShortcutBinding) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.command_sender
            .lock()
            .map_err(|_| "Failed to lock command_sender")?
            .send(ManagerCommand::Unregister {
                binding_id: binding.id.clone(),
                response: tx,
            })
            .map_err(|_| "Failed to send unregister command")?;

        rx.recv()
            .map_err(|_| "Failed to receive unregister response")?
    }

    /// Start recording mode for a specific binding
    pub fn start_recording(&self, app: &AppHandle, binding_id: String) -> Result<(), String> {
        if self.is_recording.load(Ordering::SeqCst) {
            return Err("Already recording".into());
        }

        // Create a new keyboard listener for recording
        let listener = KeyboardListener::new()
            .map_err(|e| format!("Failed to create keyboard listener: {}", e))?;

        {
            let mut recording = self
                .recording_listener
                .lock()
                .map_err(|_| "Failed to lock recording_listener")?;
            *recording = Some(listener);
        }
        {
            let mut binding = self
                .recording_binding_id
                .lock()
                .map_err(|_| "Failed to lock recording_binding_id")?;
            *binding = Some(binding_id);
        }

        self.is_recording.store(true, Ordering::SeqCst);
        self.recording_running.store(true, Ordering::SeqCst);

        // Start a thread to emit key events to the frontend
        let app_clone = app.clone();
        let recording_running = Arc::clone(&self.recording_running);
        thread::spawn(move || {
            Self::recording_loop(app_clone, recording_running);
        });

        debug!("Started native-keys recording mode");
        Ok(())
    }

    /// Recording loop - emits key events to frontend during recording
    fn recording_loop(app: AppHandle, running: Arc<AtomicBool>) {
        while running.load(Ordering::SeqCst) {
            let event = {
                let state = match app.try_state::<NativeKeysState>() {
                    Some(s) => s,
                    None => break,
                };
                let listener = state.recording_listener.lock().ok();
                listener.as_ref().and_then(|l| l.as_ref()?.try_recv())
            };

            if let Some(key_event) = event {
                // Convert to frontend-friendly format
                let frontend_event = FrontendKeyEvent {
                    modifiers: modifiers_to_strings(key_event.modifiers),
                    key: key_event.key.map(|k| k.to_string().to_lowercase()),
                    is_key_down: key_event.is_key_down,
                    hotkey_string: key_event
                        .as_hotkey()
                        .map(|h| h.to_handy_string())
                        .unwrap_or_default(),
                };

                // Emit to frontend
                if let Err(e) = app.emit("native-keys-event", &frontend_event) {
                    error!("Failed to emit key event: {}", e);
                }
            } else {
                thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        debug!("Recording loop ended");
    }

    /// Stop recording mode
    pub fn stop_recording(&self) -> Result<(), String> {
        self.is_recording.store(false, Ordering::SeqCst);
        self.recording_running.store(false, Ordering::SeqCst);

        {
            let mut recording = self
                .recording_listener
                .lock()
                .map_err(|_| "Failed to lock recording_listener")?;
            *recording = None;
        }
        {
            let mut binding = self
                .recording_binding_id
                .lock()
                .map_err(|_| "Failed to lock recording_binding_id")?;
            *binding = None;
        }

        debug!("Stopped native-keys recording mode");
        Ok(())
    }
}

impl Drop for NativeKeysState {
    fn drop(&mut self) {
        // Signal recording to stop
        self.recording_running.store(false, Ordering::SeqCst);
        self.is_recording.store(false, Ordering::SeqCst);

        // Send shutdown command
        if let Ok(sender) = self.command_sender.lock() {
            let _ = sender.send(ManagerCommand::Shutdown);
        }

        // Wait for the manager thread to finish
        if let Ok(mut handle) = self.thread_handle.lock() {
            if let Some(h) = handle.take() {
                let _ = h.join();
            }
        }
    }
}

/// Convert the key listener's modifiers to a list of strings
fn modifiers_to_strings(modifiers: key_listener::Modifiers) -> Vec<String> {
    let mut result = Vec::new();

    if modifiers.contains(key_listener::Modifiers::CTRL) {
        result.push("ctrl".to_string());
    }
    if modifiers.contains(key_listener::Modifiers::OPT) {
        #[cfg(target_os = "macos")]
        result.push("option".to_string());
        #[cfg(not(target_os = "macos"))]
        result.push("alt".to_string());
    }
    if modifiers.contains(key_listener::Modifiers::SHIFT) {
        result.push("shift".to_string());
    }
    if modifiers.contains(key_listener::Modifiers::CMD) {
        #[cfg(target_os = "macos")]
        result.push("command".to_string());
        #[cfg(not(target_os = "macos"))]
        result.push("super".to_string());
    }
    if modifiers.contains(key_listener::Modifiers::FN) {
        result.push("fn".to_string());
    }

    result
}

/// Validate a shortcut string for the NativeKeys implementation.
/// NativeKeys is more permissive: allows modifier-only combos and the fn key.
pub fn validate_shortcut(raw: &str) -> Result<(), String> {
    if raw.trim().is_empty() {
        return Err("Shortcut cannot be empty".into());
    }
    // NativeKeys accepts modifier-only, key-only, and modifier+key combos
    // Just verify the string is parseable
    let hotkey = raw
        .parse::<Hotkey>()
        .map_err(|e| format!("Invalid shortcut for NativeKeys: {}", e))?;
    // handy-keys only reports Fn on macOS, so off it this would never fire.
    if !cfg!(target_os = "macos")
        && hotkey.key.is_none()
        && hotkey.modifiers == key_listener::Modifiers::FN
    {
        return Err("The Fn key can only be used on macOS".into());
    }
    Ok(())
}

/// Initialize native-keys shortcuts
pub fn init_shortcuts(app: &AppHandle) -> Result<(), String> {
    let state = NativeKeysState::new(app.clone())?;

    let default_bindings = settings::get_default_settings().bindings;
    let user_settings = settings::load_or_create_app_settings(app);

    // Register all bindings except cancel (which is dynamic)
    for (id, default_binding) in default_bindings {
        if id == "cancel" {
            continue;
        }
        // Skip post-processing shortcut when the feature is disabled
        if id == "transcribe_with_post_process" && !user_settings.post_process_enabled {
            continue;
        }

        let binding = user_settings
            .bindings
            .get(&id)
            .cloned()
            .unwrap_or(default_binding);

        if let Err(e) = state.register(&binding) {
            error!(
                "Failed to register native-keys shortcut {} during init: {}",
                id, e
            );
        }
    }

    app.manage(state);
    info!("native-keys shortcuts initialized");
    Ok(())
}

/// Register a shortcut
pub fn register_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    let state = app
        .try_state::<NativeKeysState>()
        .ok_or("NativeKeysState not initialized")?;
    state.register(&binding)
}

/// Unregister a shortcut
pub fn unregister_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    let state = app
        .try_state::<NativeKeysState>()
        .ok_or("NativeKeysState not initialized")?;
    state.unregister(&binding)
}

/// Start key recording mode
pub fn start_native_keys_recording(app: AppHandle, binding_id: String) -> Result<(), String> {
    let settings = get_settings(&app);
    if settings.keyboard_implementation != settings::KeyboardImplementation::NativeKeys {
        return Err("native-keys is not the active keyboard implementation".into());
    }

    // While Secure Input is active the tap receives no KeyDown/KeyUp, so the
    // recorder would silently capture just the modifier and overwrite the
    // binding with it (issue #1578). Refuse instead; the frontend maps this
    // marker to a localized explanation, and the noted impact makes the
    // warning banner appear with the full story.
    if crate::secure_input::is_enabled_now() {
        crate::secure_input::note_recorder_blocked(&app);
        return Err("secure-input-active".into());
    }

    let state = app
        .try_state::<NativeKeysState>()
        .ok_or("NativeKeysState not initialized")?;

    // Suspend every registered shortcut so a combo that overlaps an existing
    // binding can't fire it (or have its keys swallowed) mid-capture.
    super::suspend_all_shortcuts(&app);

    let result = state.start_recording(&app, binding_id);
    if result.is_err() {
        super::resume_all_shortcuts(&app);
    }
    result
}

/// Stop key recording mode
pub fn stop_native_keys_recording(app: AppHandle) -> Result<(), String> {
    let settings = get_settings(&app);
    if settings.keyboard_implementation != settings::KeyboardImplementation::NativeKeys {
        return Err("native-keys is not the active keyboard implementation".into());
    }

    let state = app
        .try_state::<NativeKeysState>()
        .ok_or("NativeKeysState not initialized")?;

    // Restore shortcuts from settings regardless of how recording ended.
    // A commit has already registered the new binding via change_binding;
    // re-registering it here fails cleanly and is ignored.
    let result = state.stop_recording();
    super::resume_all_shortcuts(&app);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use key_listener::Modifiers;

    fn entry(hotkey_string: &str, modifiers: Modifiers) -> GestureEntry {
        GestureEntry {
            hotkey_string: hotkey_string.to_string(),
            modifiers,
            gesture: ModifierGesture::new(modifiers),
        }
    }

    #[test]
    fn gesture_overlap_is_rejected_by_physical_key() {
        let mut gestures = GestureListener::new();
        gestures.entries.insert(
            "transcribe".to_string(),
            entry("option_right", Modifiers::OPT_RIGHT),
        );
        assert!(gestures
            .reject_overlap("option_right", Modifiers::OPT_RIGHT)
            .is_err());
        assert!(gestures.reject_overlap("option", Modifiers::OPT).is_err());
        assert!(gestures
            .reject_overlap("option_left", Modifiers::OPT_LEFT)
            .is_ok());
        assert!(gestures.reject_overlap("fn", Modifiers::FN).is_ok());
    }

    #[test]
    fn registering_a_gesture_id_twice_fails_cleanly() {
        let mut gestures = GestureListener::new();
        gestures.entries.insert(
            "transcribe".to_string(),
            entry("option_right", Modifiers::OPT_RIGHT),
        );
        assert!(gestures
            .register("transcribe", "option_left", Modifiers::OPT_LEFT)
            .is_err());
        assert!(gestures.listener.is_none());
    }

    #[test]
    fn lone_fn_validates_only_on_macos() {
        assert_eq!(validate_shortcut("fn").is_ok(), cfg!(target_os = "macos"));
        assert!(validate_shortcut("option_right").is_ok());
        assert!(validate_shortcut("ctrl+space").is_ok());
        assert!(validate_shortcut("").is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn stuck_hold_mask_covers_each_modifier_group() {
        assert_eq!(cg_flag_mask(Modifiers::FN), 0x0080_0000);
        assert_eq!(cg_flag_mask(Modifiers::OPT_RIGHT), 0x0008_0000);
        assert_eq!(cg_flag_mask(Modifiers::OPT), 0x0008_0000);
        assert_eq!(cg_flag_mask(Modifiers::CTRL_LEFT), 0x0004_0000);
        assert_eq!(cg_flag_mask(Modifiers::SHIFT_RIGHT), 0x0002_0000);
        assert_eq!(cg_flag_mask(Modifiers::CMD), 0x0010_0000);
    }
}
