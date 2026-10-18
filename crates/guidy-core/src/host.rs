//! The application host: what the backend needs from its surrounding app.
//!
//! The backend was written against `tauri::AppHandle`. This module is the
//! Tauri-free replacement: a cheap-to-clone handle that carries managed state
//! (a typed service locator), an event bus the UI subscribes to, resolved
//! filesystem paths, the settings store, and a [`Shell`] the UI implements for
//! window and main-thread operations. The method names deliberately mirror the
//! Tauri API that the ported code calls (`state`, `try_state`, `manage`,
//! `emit`, `emit_to`, `listen`, `path`, `run_on_main_thread`, ...), so the
//! ported code stays close to its Tauri original.

use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Reverse-DNS identifier; names the per-user data and log directories.
pub const APP_IDENTIFIER: &str = "dev.guidy.app";
/// User-visible product name.
pub const PRODUCT_NAME: &str = "Guidy";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum Error {
    UnknownPath,
    Io(std::io::Error),
    Json(serde_json::Error),
    Join(String),
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnknownPath => write!(f, "unknown path"),
            Error::Io(e) => write!(f, "{e}"),
            Error::Json(e) => write!(f, "{e}"),
            Error::Join(e) => write!(f, "task failed: {e}"),
            Error::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// A backend → UI event. `name` and the JSON `payload` keep the webview-era
/// event shapes, so consumers decode the same structures.
#[derive(Clone, Debug)]
pub struct Event {
    pub name: String,
    /// `None` broadcasts; `Some(label)` targets one window (`"main"`,
    /// `"recording_overlay"`).
    pub target: Option<String>,
    pub payload: serde_json::Value,
}

impl Event {
    pub fn payload(&self) -> &serde_json::Value {
        &self.payload
    }

    pub fn decode<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        serde_json::from_value(self.payload.clone()).ok()
    }
}

pub type EventId = u64;
type ListenerFn = Arc<dyn Fn(&Event) + Send + Sync>;

struct ListenerEntry {
    id: EventId,
    /// `None` listens to every event.
    name: Option<String>,
    f: ListenerFn,
}

// ---------------------------------------------------------------------------
// Monitors (supplied by the shell; used for overlay placement)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicalPosition<T> {
    pub x: T,
    pub y: T,
}

impl<T> PhysicalPosition<T> {
    pub fn new(x: T, y: T) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicalSize<T> {
    pub width: T,
    pub height: T,
}

impl<T> PhysicalSize<T> {
    pub fn new(width: T, height: T) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkArea {
    pub position: PhysicalPosition<i32>,
    pub size: PhysicalSize<u32>,
}

/// A display, in physical pixels (like `tauri::Monitor`).
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    scale_factor: f64,
    work_area: WorkArea,
    /// The platform display id, when the UI layer knows it.
    id: Option<u64>,
}

impl Monitor {
    pub fn new(
        position: PhysicalPosition<i32>,
        size: PhysicalSize<u32>,
        scale_factor: f64,
        work_area: WorkArea,
    ) -> Self {
        Self {
            position,
            size,
            scale_factor,
            work_area,
            id: None,
        }
    }
    pub fn with_id(mut self, id: u64) -> Self {
        self.id = Some(id);
        self
    }
    pub fn id(&self) -> Option<u64> {
        self.id
    }
    pub fn position(&self) -> &PhysicalPosition<i32> {
        &self.position
    }
    pub fn size(&self) -> &PhysicalSize<u32> {
        &self.size
    }
    pub fn scale_factor(&self) -> f64 {
        self.scale_factor
    }
    pub fn work_area(&self) -> &WorkArea {
        &self.work_area
    }
}

/// Where and how big the overlay window should be, in logical points.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OverlayPlacement {
    /// `"recording" | "streaming" | "transcribing" | "processing"`.
    pub state: String,
    /// Position in global (all-displays) coordinates.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Top or bottom anchoring, for layer-shell style placement.
    pub top: bool,
    /// The display the overlay belongs on and that display's global origin,
    /// when known: windows are placed relative to their display.
    pub display_id: Option<u64>,
    pub display_origin: (f64, f64),
}

// ---------------------------------------------------------------------------
// Shell: implemented by the UI layer
// ---------------------------------------------------------------------------

/// Window-system operations the backend triggers. The GPUI app implements
/// this; headless runs and tests use [`NullShell`].
pub trait Shell: Send + Sync {
    /// Run `f` on the UI/main thread. Must run inline when already on it.
    fn run_on_main_thread(&self, f: Box<dyn FnOnce() + Send>);
    fn show_main_window(&self);
    fn hide_main_window(&self) {}
    fn is_main_window_visible(&self) -> bool {
        false
    }
    /// Apply an explicit theme (None = follow the system).
    fn set_theme(&self, _theme: Option<Theme>) {}
    /// The OS appearance (used to pick tray icon variants).
    fn system_theme(&self) -> Option<Theme> {
        None
    }
    fn monitors(&self) -> Vec<Monitor> {
        Vec::new()
    }
    fn primary_monitor(&self) -> Option<Monitor> {
        self.monitors().into_iter().next()
    }
    fn show_overlay(&self, _placement: OverlayPlacement) {}
    fn move_overlay(&self, _placement: OverlayPlacement) {}
    fn hide_overlay(&self) {}
    /// Current overlay size in logical points, if it exists.
    fn overlay_size(&self) -> Option<(f64, f64)> {
        None
    }
    /// macOS: Regular (dock icon) vs Accessory (menu-bar only).
    fn set_activation_policy_accessory(&self, _accessory: bool) {}
    fn exit(&self, code: i32) {
        std::process::exit(code);
    }
}

/// Shell used headless and in tests: runs main-thread work inline.
pub struct NullShell;

impl Shell for NullShell {
    fn run_on_main_thread(&self, f: Box<dyn FnOnce() + Send>) {
        f();
    }
    fn show_main_window(&self) {}
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseDirectory {
    Resource,
    AppData,
}

pub mod path {
    pub use super::BaseDirectory;
}

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub app_data_dir: PathBuf,
    pub app_log_dir: PathBuf,
    /// Directory containing the bundled `resources/` tree.
    pub resource_dir: PathBuf,
    pub home_dir: PathBuf,
}

impl AppPaths {
    /// Platform default locations for [`APP_IDENTIFIER`], matching what
    /// Tauri's path resolver produces.
    pub fn platform_default() -> Self {
        let data = dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(APP_IDENTIFIER);
        #[cfg(target_os = "macos")]
        let logs = dirs::home_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Library/Logs")
            .join(APP_IDENTIFIER);
        #[cfg(not(target_os = "macos"))]
        let logs = dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(APP_IDENTIFIER)
            .join("logs");
        Self {
            app_data_dir: data,
            app_log_dir: logs,
            resource_dir: default_resource_dir(),
            home_dir: dirs::home_dir().unwrap_or_else(std::env::temp_dir),
        }
    }

    /// Everything under one directory; used by tests.
    pub fn rooted_at(root: &Path) -> Self {
        Self {
            app_data_dir: root.join("data"),
            app_log_dir: root.join("logs"),
            resource_dir: default_resource_dir(),
            home_dir: root.join("home"),
        }
    }
}

/// Locate the directory that contains `resources/`:
/// `GUIDY_RESOURCE_DIR` → macOS bundle `Contents/Resources` → next to the
/// executable → Linux `../lib/guidy` → the workspace `assets/` (dev builds).
pub fn default_resource_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("GUIDY_RESOURCE_DIR") {
        return PathBuf::from(dir);
    }
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join("../Resources"));
            candidates.push(exe_dir.to_path_buf());
            candidates.push(exe_dir.join("../lib/guidy"));
        }
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
    for c in &candidates {
        if c.join("resources").is_dir() {
            return c.canonicalize().unwrap_or_else(|_| c.clone());
        }
    }
    candidates.pop().unwrap()
}

pub struct PathResolver<'a> {
    paths: &'a AppPaths,
}

impl PathResolver<'_> {
    pub fn app_data_dir(&self) -> Result<PathBuf> {
        Ok(self.paths.app_data_dir.clone())
    }
    pub fn app_log_dir(&self) -> Result<PathBuf> {
        Ok(self.paths.app_log_dir.clone())
    }
    pub fn resource_dir(&self) -> Result<PathBuf> {
        Ok(self.paths.resource_dir.clone())
    }
    pub fn home_dir(&self) -> Result<PathBuf> {
        Ok(self.paths.home_dir.clone())
    }
    pub fn resolve(&self, path: impl AsRef<Path>, base: BaseDirectory) -> Result<PathBuf> {
        let base = match base {
            BaseDirectory::Resource => &self.paths.resource_dir,
            BaseDirectory::AppData => &self.paths.app_data_dir,
        };
        Ok(base.join(path))
    }
}

// ---------------------------------------------------------------------------
// JSON key-value store (replacement for tauri-plugin-store)
// ---------------------------------------------------------------------------

/// A JSON object persisted to one file. Every `set` writes through
/// atomically (temp file + rename), like the store plugin's autosave.
pub struct Store {
    path: PathBuf,
    data: Mutex<serde_json::Map<String, serde_json::Value>>,
}

impl Store {
    fn open(path: PathBuf) -> Self {
        let data = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|v| match v {
                serde_json::Value::Object(map) => Some(map),
                _ => None,
            })
            .unwrap_or_default();
        Self {
            path,
            data: Mutex::new(data),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.data.lock().get(key).cloned()
    }

    pub fn set(&self, key: impl Into<String>, value: serde_json::Value) {
        let mut data = self.data.lock();
        data.insert(key.into(), value);
        if let Err(e) = write_json_atomic(&self.path, &serde_json::Value::Object(data.clone())) {
            log::error!("Failed to persist store {}: {}", self.path.display(), e);
        }
    }
}

fn write_json_atomic(path: &Path, value: &serde_json::Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, path)
}

// ---------------------------------------------------------------------------
// Managed state
// ---------------------------------------------------------------------------

/// Borrowed managed state, like `tauri::State`.
pub struct State<'a, T: Send + Sync + 'static> {
    value: Arc<T>,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<T: Send + Sync + 'static> State<'_, T> {
    pub fn inner(&self) -> &T {
        &self.value
    }
}

impl<T: Send + Sync + 'static> std::ops::Deref for State<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T: Send + Sync + 'static> Clone for State<'_, T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            _marker: std::marker::PhantomData,
        }
    }
}

// ---------------------------------------------------------------------------
// AppHandle
// ---------------------------------------------------------------------------

struct Inner {
    state: RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
    listeners: RwLock<Vec<ListenerEntry>>,
    next_listener: AtomicU64,
    paths: AppPaths,
    stores: Mutex<HashMap<PathBuf, Arc<Store>>>,
    shell: RwLock<Arc<dyn Shell>>,
}

#[derive(Clone)]
pub struct AppHandle {
    inner: Arc<Inner>,
}

impl fmt::Debug for AppHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppHandle").finish_non_exhaustive()
    }
}

impl AppHandle {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: RwLock::new(HashMap::new()),
                listeners: RwLock::new(Vec::new()),
                next_listener: AtomicU64::new(1),
                paths,
                stores: Mutex::new(HashMap::new()),
                shell: RwLock::new(Arc::new(NullShell)),
            }),
        }
    }

    // -- state -------------------------------------------------------------

    /// Register managed state. Like Tauri, the first registration of a type
    /// wins; returns `false` if the type was already managed.
    pub fn manage<T: Send + Sync + 'static>(&self, value: T) -> bool {
        let mut state = self.inner.state.write();
        let id = TypeId::of::<T>();
        if state.contains_key(&id) {
            return false;
        }
        state.insert(id, Arc::new(value));
        true
    }

    pub fn try_state<T: Send + Sync + 'static>(&self) -> Option<State<'_, T>> {
        let state = self.inner.state.read();
        let any = state.get(&TypeId::of::<T>())?.clone();
        drop(state);
        let value = any.downcast::<T>().ok()?;
        Some(State {
            value,
            _marker: std::marker::PhantomData,
        })
    }

    /// Panics when `T` is not managed, like `tauri::Manager::state`.
    pub fn state<T: Send + Sync + 'static>(&self) -> State<'_, T> {
        self.try_state::<T>().unwrap_or_else(|| {
            panic!(
                "state() called before manage() for {}",
                std::any::type_name::<T>()
            )
        })
    }

    // -- events ------------------------------------------------------------

    pub fn emit<S: Serialize>(&self, event: &str, payload: S) -> Result<()> {
        self.dispatch(Event {
            name: event.to_string(),
            target: None,
            payload: serde_json::to_value(payload)?,
        });
        Ok(())
    }

    pub fn emit_to<S: Serialize>(&self, target: &str, event: &str, payload: S) -> Result<()> {
        self.dispatch(Event {
            name: event.to_string(),
            target: Some(target.to_string()),
            payload: serde_json::to_value(payload)?,
        });
        Ok(())
    }

    fn dispatch(&self, event: Event) {
        // Snapshot so listeners may (un)register without deadlocking.
        let listeners: Vec<ListenerFn> = self
            .inner
            .listeners
            .read()
            .iter()
            .filter(|l| l.name.as_deref().is_none_or(|n| n == event.name))
            .map(|l| l.f.clone())
            .collect();
        for f in listeners {
            f(&event);
        }
    }

    /// Listen to one event by name.
    pub fn listen<F>(&self, event: &str, f: F) -> EventId
    where
        F: Fn(Event) + Send + Sync + 'static,
    {
        self.add_listener(
            Some(event.to_string()),
            Arc::new(move |e: &Event| f(e.clone())),
        )
    }

    /// Listen to every event (the UI's bridge uses this).
    pub fn listen_any<F>(&self, f: F) -> EventId
    where
        F: Fn(&Event) + Send + Sync + 'static,
    {
        self.add_listener(None, Arc::new(f))
    }

    fn add_listener(&self, name: Option<String>, f: ListenerFn) -> EventId {
        let id = self.inner.next_listener.fetch_add(1, Ordering::Relaxed);
        self.inner
            .listeners
            .write()
            .push(ListenerEntry { id, name, f });
        id
    }

    pub fn unlisten(&self, id: EventId) {
        self.inner.listeners.write().retain(|l| l.id != id);
    }

    // -- paths & store -----------------------------------------------------

    pub fn path(&self) -> PathResolver<'_> {
        PathResolver {
            paths: &self.inner.paths,
        }
    }

    pub fn paths(&self) -> &AppPaths {
        &self.inner.paths
    }

    /// Open (or reuse) a JSON store. Relative paths resolve under the app
    /// data dir, absolute paths are used as-is (portable mode).
    pub fn store(&self, path: impl AsRef<Path>) -> Result<Arc<Store>> {
        let path = path.as_ref();
        let full = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.inner.paths.app_data_dir.join(path)
        };
        let mut stores = self.inner.stores.lock();
        Ok(stores
            .entry(full.clone())
            .or_insert_with(|| Arc::new(Store::open(full)))
            .clone())
    }

    // -- shell -------------------------------------------------------------

    pub fn set_shell(&self, shell: Arc<dyn Shell>) {
        *self.inner.shell.write() = shell;
    }

    pub fn shell(&self) -> Arc<dyn Shell> {
        self.inner.shell.read().clone()
    }

    pub fn run_on_main_thread<F: FnOnce() + Send + 'static>(&self, f: F) -> Result<()> {
        self.shell().run_on_main_thread(Box::new(f));
        Ok(())
    }

    pub fn available_monitors(&self) -> Result<Vec<Monitor>> {
        Ok(self.shell().monitors())
    }

    pub fn primary_monitor(&self) -> Result<Option<Monitor>> {
        Ok(self.shell().primary_monitor())
    }

    pub fn exit(&self, code: i32) {
        self.shell().exit(code);
    }

    // -- plugin replacements -----------------------------------------------

    pub fn clipboard(&self) -> clipboard::Clipboard {
        clipboard::Clipboard
    }

    pub fn opener(&self) -> opener::Opener {
        opener::Opener
    }

    pub fn package_info(&self) -> PackageInfo {
        PackageInfo {
            name: PRODUCT_NAME.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// async runtime (replacement for tauri::async_runtime)
// ---------------------------------------------------------------------------

pub mod async_runtime {
    use super::Error;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::OnceLock;
    use std::task::{Context, Poll};

    fn runtime() -> &'static tokio::runtime::Runtime {
        static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
        RT.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("guidy-async")
                .build()
                .expect("failed to build tokio runtime")
        })
    }

    pub fn handle() -> tokio::runtime::Handle {
        runtime().handle().clone()
    }

    pub struct JoinHandle<T>(tokio::task::JoinHandle<T>);

    impl<T> JoinHandle<T> {
        pub fn abort(&self) {
            self.0.abort();
        }
    }

    impl<T> Future for JoinHandle<T> {
        type Output = Result<T, Error>;
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            Pin::new(&mut self.0)
                .poll(cx)
                .map(|r| r.map_err(|e| Error::Join(e.to_string())))
        }
    }

    pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        JoinHandle(runtime().spawn(future))
    }

    pub fn spawn_blocking<F, R>(f: F) -> JoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        JoinHandle(runtime().spawn_blocking(f))
    }

    pub fn block_on<F: Future>(future: F) -> F::Output {
        runtime().block_on(future)
    }
}

// ---------------------------------------------------------------------------
// image (clipboard images)
// ---------------------------------------------------------------------------

pub mod image {
    use std::borrow::Cow;

    /// RGBA8 image, like `tauri::image::Image`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Image<'a> {
        rgba: Cow<'a, [u8]>,
        width: u32,
        height: u32,
    }

    impl<'a> Image<'a> {
        pub fn new(rgba: &'a [u8], width: u32, height: u32) -> Self {
            Self {
                rgba: Cow::Borrowed(rgba),
                width,
                height,
            }
        }
        pub fn new_owned(rgba: Vec<u8>, width: u32, height: u32) -> Image<'static> {
            Image {
                rgba: Cow::Owned(rgba),
                width,
                height,
            }
        }
        pub fn rgba(&self) -> &[u8] {
            &self.rgba
        }
        pub fn width(&self) -> u32 {
            self.width
        }
        pub fn height(&self) -> u32 {
            self.height
        }
        pub fn to_owned(self) -> Image<'static> {
            Image {
                rgba: Cow::Owned(self.rgba.into_owned()),
                width: self.width,
                height: self.height,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// clipboard (replacement for tauri-plugin-clipboard-manager)
// ---------------------------------------------------------------------------

pub mod clipboard {
    use super::image::Image;
    use std::borrow::Cow;

    pub struct Clipboard;

    fn board() -> Result<arboard::Clipboard, String> {
        arboard::Clipboard::new().map_err(|e| e.to_string())
    }

    impl Clipboard {
        pub fn read_text(&self) -> Result<String, String> {
            board()?.get_text().map_err(|e| e.to_string())
        }
        pub fn write_text(&self, text: impl Into<String>) -> Result<(), String> {
            board()?.set_text(text.into()).map_err(|e| e.to_string())
        }
        pub fn read_image(&self) -> Result<Image<'static>, String> {
            let img = board()?.get_image().map_err(|e| e.to_string())?;
            Ok(Image::new_owned(
                img.bytes.into_owned(),
                img.width as u32,
                img.height as u32,
            ))
        }
        pub fn write_image(&self, image: &Image<'_>) -> Result<(), String> {
            board()?
                .set_image(arboard::ImageData {
                    width: image.width() as usize,
                    height: image.height() as usize,
                    bytes: Cow::Owned(image.rgba().to_vec()),
                })
                .map_err(|e| e.to_string())
        }
        pub fn clear(&self) -> Result<(), String> {
            board()?.clear().map_err(|e| e.to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// opener (replacement for tauri-plugin-opener)
// ---------------------------------------------------------------------------

pub mod opener {
    pub struct Opener;

    impl Opener {
        pub fn open_path(
            &self,
            path: impl AsRef<std::ffi::OsStr>,
            _with: Option<String>,
        ) -> Result<(), String> {
            open::that_detached(path).map_err(|e| e.to_string())
        }
        pub fn open_url(&self, url: impl AsRef<str>, _with: Option<String>) -> Result<(), String> {
            open::that_detached(url.as_ref()).map_err(|e| e.to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

#[cfg(any(test, feature = "test-support"))]
pub mod testing {
    use super::*;

    /// An app handle rooted in a temp dir, plus a recorder of every event.
    pub struct TestApp {
        pub app: AppHandle,
        pub events: Arc<Mutex<Vec<Event>>>,
        _dir: tempfile_dir::TempDir,
    }

    mod tempfile_dir {
        /// Minimal temp dir so the helper works without dev-dependencies
        /// in dependent crates.
        pub struct TempDir(pub std::path::PathBuf);
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    impl TestApp {
        pub fn new() -> Self {
            static N: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "guidy-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let app = AppHandle::new(AppPaths::rooted_at(&dir));
            // Always present in a running app (secure_input::init at startup).
            crate::secure_input::init_state_only(&app);
            let events = Arc::new(Mutex::new(Vec::new()));
            let sink = events.clone();
            app.listen_any(move |e| sink.lock().push(e.clone()));
            Self {
                app,
                events,
                _dir: tempfile_dir::TempDir(dir),
            }
        }

        pub fn event_names(&self) -> Vec<String> {
            self.events.lock().iter().map(|e| e.name.clone()).collect()
        }

        pub fn take_events(&self) -> Vec<Event> {
            std::mem::take(&mut *self.events.lock())
        }
    }

    impl Default for TestApp {
        fn default() -> Self {
            Self::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Typed events (replacement for tauri_specta::Event)
// ---------------------------------------------------------------------------

/// An event type with a fixed wire name (tauri-specta used the kebab-case
/// type name, e.g. `HistoryUpdatePayload` → `history-update-payload`).
pub trait TypedEvent: Serialize + Clone {
    const NAME: &'static str;

    fn emit(&self, app: &AppHandle) -> Result<()> {
        app.emit(Self::NAME, self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::testing::TestApp;
    use super::*;

    #[test]
    fn manage_is_first_write_wins_and_state_round_trips() {
        let t = TestApp::new();
        assert!(t.app.manage(Arc::new(5u32)));
        assert!(!t.app.manage(Arc::new(6u32)));
        assert_eq!(**t.app.state::<Arc<u32>>(), 5);
        assert!(t.app.try_state::<String>().is_none());
    }

    #[test]
    fn emit_reaches_named_and_global_listeners() {
        let t = TestApp::new();
        let hits = Arc::new(Mutex::new(0));
        let h = hits.clone();
        let id = t.app.listen("model-deleted", move |e| {
            assert_eq!(e.payload, serde_json::json!("m1"));
            *h.lock() += 1;
        });
        t.app.emit("model-deleted", "m1").unwrap();
        t.app.emit("other", ()).unwrap();
        t.app.unlisten(id);
        t.app.emit("model-deleted", "m2").unwrap();
        assert_eq!(*hits.lock(), 1);
        assert_eq!(
            t.event_names(),
            vec!["model-deleted", "other", "model-deleted"]
        );
    }

    #[test]
    fn emit_to_records_target() {
        let t = TestApp::new();
        t.app
            .emit_to("recording_overlay", "mic-level", [0.5f32])
            .unwrap();
        let ev = t.take_events();
        assert_eq!(ev[0].target.as_deref(), Some("recording_overlay"));
    }

    #[test]
    fn store_persists_and_reloads() {
        let t = TestApp::new();
        let store = t.app.store("settings_store.json").unwrap();
        store.set("settings", serde_json::json!({"a": 1}));
        let path = store.path().to_path_buf();
        let reopened = Store::open(path);
        assert_eq!(reopened.get("settings"), Some(serde_json::json!({"a": 1})));
    }

    #[test]
    fn resolve_uses_base_directories() {
        let t = TestApp::new();
        let p = t
            .app
            .path()
            .resolve("custom_start.wav", BaseDirectory::AppData)
            .unwrap();
        assert!(p.ends_with("data/custom_start.wav"));
        let r = t
            .app
            .path()
            .resolve("resources/pop_start.wav", BaseDirectory::Resource)
            .unwrap();
        assert!(
            r.exists(),
            "bundled resources resolve in dev: {}",
            r.display()
        );
    }

    #[test]
    fn async_runtime_join_handle_returns_value() {
        let v = async_runtime::block_on(async_runtime::spawn_blocking(|| 7)).unwrap();
        assert_eq!(v, 7);
    }
}
