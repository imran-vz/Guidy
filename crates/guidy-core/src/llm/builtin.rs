//! The built-in llama.cpp runtime: supervises the `guidy-llm` helper process
//! (`crates/guidy-llm`) and exposes it as an [`LlmProvider`] with id
//! [`BUILTIN_LLM_PROVIDER_ID`].
//!
//! - The helper is found next to the running executable (in the app bundle:
//!   `Contents/MacOS/guidy-llm`). Debug and test builds also accept
//!   `$GUIDY_LLM_BIN`; release builds ignore it, so the environment can't make
//!   the app run (and send transcripts to) another binary.
//! - It is spawned lazily on the first request, loads the model on demand,
//!   and is shut down after [`IDLE_TIMEOUT`] without requests (freeing the
//!   model and its Metal buffers).
//! - Cancelling a request kills the process; the next request respawns it.
//!   So does a timeout (start, load, ping).
//! - Deleting the loaded model stops the process, at the latest when the
//!   request using it finishes ([`BuiltinLlm::forget_model`]).
//! - One request at a time; concurrent callers queue on a mutex.
//!
//! Protocol: newline-delimited JSON, mirrored from
//! `crates/guidy-llm/src/protocol.rs` (keep them in sync).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::local_models::{self, CatalogEntry, LocalModels};
use super::{
    extract_json, json_via_prompt, strip_think, validate_json, ChatMessage, ChatOptions,
    LlmProvider, CANCELLED,
};
use crate::host::AppHandle;
use crate::meetings::settings::BUILTIN_LLM_PROVIDER_ID;

/// Shut the helper down after this long without a request.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Env var that overrides the helper binary path (debug and test builds only).
pub const BIN_ENV: &str = "GUIDY_LLM_BIN";

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const LOAD_TIMEOUT: Duration = Duration::from_secs(300);
const POLL: Duration = Duration::from_millis(100);

// ----- protocol (mirror of crates/guidy-llm/src/protocol.rs) ---------------

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WireMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Load {
        id: u64,
        path: String,
        n_ctx: u32,
        #[serde(default)]
        n_gpu_layers: Option<u32>,
    },
    Chat {
        id: u64,
        messages: Vec<WireMessage>,
        max_tokens: u32,
        temperature: f32,
        #[serde(default)]
        json_schema: Option<Value>,
        #[serde(default)]
        stream: bool,
    },
    Cancel {
        #[serde(default)]
        id: Option<u64>,
    },
    Unload {
        id: u64,
    },
    Ping {
        id: u64,
    },
    Shutdown,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    Cancelled,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Ready {
        version: String,
    },
    Loaded {
        id: u64,
        n_ctx: u32,
        n_ctx_train: u32,
    },
    Token {
        id: u64,
        text: String,
    },
    Done {
        id: u64,
        text: String,
        finish_reason: FinishReason,
        prompt_tokens: u32,
        completion_tokens: u32,
    },
    Unloaded {
        id: u64,
    },
    Pong {
        id: u64,
    },
    Error {
        id: Option<u64>,
        message: String,
    },
}

/// Encode one frame (a single line, newline-terminated).
pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    Ok(line)
}

/// Decode one line; `None` for blank lines.
pub fn decode_frame<T: serde::de::DeserializeOwned>(line: &str) -> Option<Result<T>> {
    let line = line.trim();
    (!line.is_empty()).then(|| serde_json::from_str(line).map_err(Into::into))
}

// ----- binary discovery ------------------------------------------------------

fn bin_name() -> &'static str {
    if cfg!(windows) {
        "guidy-llm.exe"
    } else {
        "guidy-llm"
    }
}

/// Whether [`BIN_ENV`] is honoured in this build.
const BIN_ENV_ALLOWED: bool = cfg!(any(test, debug_assertions));

/// Where the helper may be, in order, for an executable at `exe`: next to it
/// and, for test binaries in `target/<profile>/deps`, one level up
/// (`search_parent`, tests only).
fn binary_candidates(exe: &Path, search_parent: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = exe.parent() {
        out.push(dir.join(bin_name()));
        if search_parent {
            if let Some(up) = dir.parent() {
                out.push(up.join(bin_name()));
            }
        }
    }
    out
}

/// Where the helper is: `current_exe().parent()/guidy-llm`. Debug and test
/// builds first honour `$GUIDY_LLM_BIN`; tests also look one level up.
pub fn find_binary() -> Result<PathBuf> {
    if BIN_ENV_ALLOWED {
        if let Some(p) = std::env::var_os(BIN_ENV) {
            let p = PathBuf::from(p);
            if p.is_file() {
                return Ok(p);
            }
            bail!("{BIN_ENV} points to a missing file: {}", p.display());
        }
    } else if std::env::var_os(BIN_ENV).is_some() {
        log::warn!("Ignoring {BIN_ENV} in a release build");
    }
    let exe = std::env::current_exe().context("current executable path")?;
    binary_candidates(&exe, cfg!(test))
        .into_iter()
        .find(|c| c.is_file())
        .ok_or_else(|| {
            anyhow!(
                "the built-in model runtime ({}) is missing next to {}",
                bin_name(),
                exe.display()
            )
        })
}

// ----- supervisor ----------------------------------------------------------

/// The helper didn't answer within the deadline. Fatal: the worker is killed
/// (find it with [`is_timeout_error`]).
#[derive(Debug)]
pub struct WorkerTimeout;

impl std::fmt::Display for WorkerTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the built-in model runtime did not answer in time")
    }
}

impl std::error::Error for WorkerTimeout {}

/// The helper couldn't turn the JSON schema into a grammar (or the grammar
/// was rejected). Not fatal: `chat_json` retries without a grammar.
#[derive(Debug)]
pub struct GrammarError(pub(crate) String);

impl std::fmt::Display for GrammarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "built-in model: {}", self.0)
    }
}

impl std::error::Error for GrammarError {}

/// Whether the helper's error message is about the JSON schema grammar (see
/// `build_sampler` in crates/guidy-llm/src/main.rs).
fn is_grammar_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("unsupported json schema") || lower.contains("invalid grammar")
}

/// Whether `e` (or anything it wraps) is a [`WorkerTimeout`].
pub fn is_timeout_error(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.is::<WorkerTimeout>())
}

/// A running helper. Dropping it kills and reaps the process, so no error
/// path (e.g. a failed start) leaves it running or as a zombie.
struct Worker {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Response>,
    /// `(model path, n_ctx)` currently loaded.
    loaded: Option<(PathBuf, u32)>,
    next_id: u64,
}

impl Worker {
    /// Start the helper and wait up to `startup_timeout` for `Ready`.
    fn spawn(binary: &Path, startup_timeout: Duration) -> Result<Self> {
        let mut child = Command::new(binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("start {}", binary.display()))?;
        let stdin = child.stdin.take().context("worker stdin")?;
        let stdout = child.stdout.take().context("worker stdout")?;
        let stderr = child.stderr.take().context("worker stderr")?;
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("guidy-llm-out".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    match decode_frame::<Response>(&line) {
                        Some(Ok(r)) => {
                            if tx.send(r).is_err() {
                                break;
                            }
                        }
                        Some(Err(e)) => log::warn!("guidy-llm: bad frame: {e}"),
                        None => {}
                    }
                }
            })?;
        std::thread::Builder::new()
            .name("guidy-llm-err".into())
            .spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    log::debug!("guidy-llm: {line}");
                }
            })?;
        // From here on, any early return drops `worker`, which kills it.
        let mut worker = Self {
            child,
            stdin,
            rx,
            loaded: None,
            next_id: 1,
        };
        match worker.recv(startup_timeout, None)? {
            Response::Ready { version } => {
                log::info!("Started guidy-llm {version}");
                Ok(worker)
            }
            Response::Error { message, .. } => {
                bail!("the built-in model runtime failed to start: {message}")
            }
            other => bail!("unexpected first message from guidy-llm: {other:?}"),
        }
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn send(&mut self, request: &Request) -> Result<()> {
        let frame = encode_frame(request)?;
        self.stdin
            .write_all(&frame)
            .and_then(|_| self.stdin.flush())
            .context("the built-in model runtime stopped")
    }

    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Next response, waiting up to `timeout` and watching `cancel`.
    fn recv(&mut self, timeout: Duration, cancel: Option<&AtomicBool>) -> Result<Response> {
        let start = Instant::now();
        loop {
            if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
                bail!(CANCELLED);
            }
            match self.rx.recv_timeout(POLL) {
                Ok(r) => return Ok(r),
                Err(RecvTimeoutError::Disconnected) => {
                    bail!("the built-in model runtime exited unexpectedly")
                }
                Err(RecvTimeoutError::Timeout) => {
                    if start.elapsed() >= timeout {
                        return Err(WorkerTimeout.into());
                    }
                }
            }
        }
    }

    /// Wait for the response to request `id` (skipping stale ones).
    fn wait_for(
        &mut self,
        id: u64,
        timeout: Duration,
        cancel: Option<&AtomicBool>,
    ) -> Result<Response> {
        loop {
            let r = self.recv(timeout, cancel)?;
            let rid = match &r {
                Response::Loaded { id, .. }
                | Response::Done { id, .. }
                | Response::Unloaded { id }
                | Response::Pong { id } => Some(*id),
                Response::Error { id, .. } => *id,
                Response::Token { .. } | Response::Ready { .. } => continue,
            };
            if rid == Some(id) {
                return Ok(r);
            }
            if let Response::Error { id: None, message } = &r {
                log::warn!("guidy-llm: {message}");
            }
        }
    }

    /// Ask the helper to exit; kill it if it hasn't within 2 s.
    fn shutdown(mut self) {
        let _ = self.send(&Request::Shutdown);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if !self.alive() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // Dropping `self` kills it.
    }

    /// Kill and reap the process (idempotent; also done on drop).
    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Parameters of one built-in chat request.
pub struct BuiltinChat<'a> {
    pub model_path: &'a Path,
    pub n_ctx: u32,
    pub messages: &'a [ChatMessage],
    pub max_tokens: u32,
    pub temperature: f32,
    pub json_schema: Option<&'a Value>,
}

/// How long a queued request waits for the worker before re-checking its
/// cancel flag.
const LOCK_POLL: Duration = Duration::from_millis(50);

/// How long [`BuiltinLlm::ping`] waits for a worker another request holds.
const PING_LOCK_WAIT: Duration = Duration::from_millis(500);

/// What [`BuiltinLlm::ping`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PingStatus {
    /// The helper answered.
    Ready,
    /// A request is using the helper, so it's running; not probed.
    Busy,
}

/// The process-wide `guidy-llm` supervisor.
pub struct BuiltinLlm {
    worker: Mutex<Option<Worker>>,
    last_used: Mutex<Instant>,
    idle_timeout: Duration,
    reaper_started: AtomicBool,
    binary: Mutex<Option<PathBuf>>,
    /// How long the helper may take to start, and to answer a ping.
    startup_timeout: Duration,
    /// Model files deleted while a request held the worker (see
    /// [`Self::forget_model`]); consumed by the next lock holder.
    forgotten: Mutex<Vec<PathBuf>>,
}

impl BuiltinLlm {
    pub fn new(idle_timeout: Duration) -> Self {
        Self {
            worker: Mutex::new(None),
            last_used: Mutex::new(Instant::now()),
            idle_timeout,
            reaper_started: AtomicBool::new(false),
            binary: Mutex::new(None),
            startup_timeout: STARTUP_TIMEOUT,
            forgotten: Mutex::new(Vec::new()),
        }
    }

    pub fn global() -> &'static BuiltinLlm {
        static GLOBAL: OnceLock<BuiltinLlm> = OnceLock::new();
        let llm = GLOBAL.get_or_init(|| BuiltinLlm::new(IDLE_TIMEOUT));
        llm.start_reaper();
        llm
    }

    /// Use this helper binary instead of [`find_binary`] (tests).
    pub fn set_binary(&self, path: PathBuf) {
        *self.binary.lock() = Some(path);
    }

    fn binary(&self) -> Result<PathBuf> {
        match self.binary.lock().clone() {
            Some(p) => Ok(p),
            None => find_binary(),
        }
    }

    fn start_reaper(&'static self) {
        if self.reaper_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = std::thread::Builder::new()
            .name("guidy-llm-idle".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_secs(15));
                self.reap_if_idle();
            });
    }

    /// Shut the worker down when it's idle past the timeout (and not busy).
    pub fn reap_if_idle(&self) -> bool {
        let Some(mut guard) = self.worker.try_lock() else {
            return false;
        };
        if guard.is_some() && self.last_used.lock().elapsed() >= self.idle_timeout {
            log::info!("Stopping the idle built-in model runtime");
            if let Some(w) = guard.take() {
                w.shutdown();
            }
            return true;
        }
        false
    }

    pub fn is_running(&self) -> bool {
        self.worker
            .try_lock()
            .map(|mut g| g.as_mut().is_some_and(|w| w.alive()))
            .unwrap_or(true)
    }

    /// Stop the worker now (e.g. at app exit).
    pub fn shutdown(&self) {
        if let Some(w) = self.worker.lock().take() {
            w.shutdown();
        }
    }

    /// The model file is going away: stop the worker if it has it loaded.
    ///
    /// Doesn't wait for a running request: the path is recorded and whoever
    /// holds the worker next (the running request as it finishes, or the
    /// next request before it starts) stops the worker if that model is
    /// still loaded, so it's neither kept in memory nor reused.
    pub fn forget_model(&self, path: &Path) {
        // Record before trying the lock, so a holder releasing it right now
        // still sees the path.
        self.forgotten.lock().push(path.to_path_buf());
        if let Some(mut guard) = self.worker.try_lock() {
            self.stop_if_forgotten(&mut guard);
        }
    }

    /// Stop the worker if its loaded model was forgotten, and clear the
    /// forgotten list. Call with the worker lock held.
    fn stop_if_forgotten(&self, slot: &mut Option<Worker>) {
        let forgotten = std::mem::take(&mut *self.forgotten.lock());
        let stale = slot.as_ref().is_some_and(|w| {
            w.loaded
                .as_ref()
                .is_some_and(|(p, _)| forgotten.contains(p))
        });
        if stale {
            log::info!("Stopping the built-in model runtime: its model was deleted");
            if let Some(w) = slot.take() {
                w.shutdown();
            }
        }
    }

    /// End of a request, with the worker lock still held: kill the worker if
    /// the request failed in a way that leaves it unusable (cancelled, timed
    /// out, or exited), then apply pending [`Self::forget_model`] calls.
    fn finish_request(&self, slot: &mut Option<Worker>, error: Option<&anyhow::Error>) {
        if let Some(e) = error {
            let fatal = super::is_cancelled_error(e)
                || is_timeout_error(e)
                || slot.as_mut().is_some_and(|w| !w.alive());
            if fatal {
                if let Some(mut w) = slot.take() {
                    w.kill();
                }
            }
        }
        self.stop_if_forgotten(slot);
    }

    fn ensure_worker<'a>(&self, slot: &'a mut Option<Worker>) -> Result<&'a mut Worker> {
        if slot.as_mut().is_some_and(|w| !w.alive()) {
            log::warn!("The built-in model runtime exited; restarting it");
            *slot = None;
        }
        if slot.is_none() {
            *slot = Some(Worker::spawn(&self.binary()?, self.startup_timeout)?);
        }
        Ok(slot.as_mut().expect("worker just ensured"))
    }

    /// Start the helper (if needed) and check it answers. A helper that
    /// doesn't answer in time is killed.
    ///
    /// Doesn't queue behind a running request (a chat holds the worker for
    /// the whole generation, possibly minutes): if the worker stays busy for
    /// [`PING_LOCK_WAIT`], it's running, and [`PingStatus::Busy`] says so.
    pub fn ping(&self) -> Result<PingStatus> {
        let Some(mut guard) = self.worker.try_lock_for(PING_LOCK_WAIT) else {
            return Ok(PingStatus::Busy);
        };
        self.stop_if_forgotten(&mut guard);
        let result = self.ping_locked(&mut guard);
        self.finish_request(&mut guard, result.as_ref().err());
        result.map(|()| PingStatus::Ready)
    }

    fn ping_locked(&self, slot: &mut Option<Worker>) -> Result<()> {
        let worker = self.ensure_worker(slot)?;
        let id = worker.id();
        worker.send(&Request::Ping { id })?;
        match worker.wait_for(id, self.startup_timeout, None)? {
            Response::Pong { .. } => Ok(()),
            other => bail!("unexpected reply to ping: {other:?}"),
        }
    }

    /// Run one chat. `cancel` kills the helper and returns [`CANCELLED`].
    ///
    /// Requests are serialized: there is one helper process, and a chat holds
    /// it for the whole generation. A request waiting for its turn still
    /// honors `cancel` (it returns [`CANCELLED`] without touching the helper
    /// or the running request).
    pub fn chat(&self, req: &BuiltinChat<'_>, cancel: Option<&AtomicBool>) -> Result<String> {
        let mut guard = self.lock_worker(cancel)?;
        *self.last_used.lock() = Instant::now();
        // A model deleted while we queued must not be reused.
        self.stop_if_forgotten(&mut guard);
        let result = self.chat_locked(&mut guard, req, cancel);
        *self.last_used.lock() = Instant::now();
        self.finish_request(&mut guard, result.as_ref().err());
        result
    }

    /// Wait for the worker lock, giving up with [`CANCELLED`] as soon as
    /// `cancel` is set (checked between short timed waits and once more
    /// after acquiring).
    fn lock_worker(
        &self,
        cancel: Option<&AtomicBool>,
    ) -> Result<parking_lot::MutexGuard<'_, Option<Worker>>> {
        let cancelled = || cancel.is_some_and(|c| c.load(Ordering::SeqCst));
        loop {
            if cancelled() {
                bail!(CANCELLED);
            }
            if let Some(guard) = self.worker.try_lock_for(LOCK_POLL) {
                if cancelled() {
                    bail!(CANCELLED);
                }
                return Ok(guard);
            }
        }
    }

    fn chat_locked(
        &self,
        slot: &mut Option<Worker>,
        req: &BuiltinChat<'_>,
        cancel: Option<&AtomicBool>,
    ) -> Result<String> {
        let worker = self.ensure_worker(slot)?;
        let want = (req.model_path.to_path_buf(), req.n_ctx);
        if worker.loaded.as_ref() != Some(&want) {
            worker.loaded = None;
            let id = worker.id();
            worker.send(&Request::Load {
                id,
                path: req.model_path.to_string_lossy().into_owned(),
                n_ctx: req.n_ctx,
                n_gpu_layers: None,
            })?;
            match worker.wait_for(id, LOAD_TIMEOUT, cancel)? {
                Response::Loaded { n_ctx, .. } => {
                    log::info!(
                        "Loaded {} (n_ctx {n_ctx})",
                        req.model_path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    );
                    worker.loaded = Some(want);
                }
                Response::Error { message, .. } => bail!("could not load the model: {message}"),
                other => bail!("unexpected reply to load: {other:?}"),
            }
        }
        let id = worker.id();
        worker.send(&Request::Chat {
            id,
            messages: req
                .messages
                .iter()
                .map(|m| WireMessage {
                    role: m.role.as_str().to_string(),
                    content: m.content.clone(),
                })
                .collect(),
            max_tokens: req.max_tokens,
            temperature: req.temperature,
            json_schema: req.json_schema.cloned(),
            stream: false,
        })?;
        // Generation has no deadline (long transcripts on slow Macs); cancel
        // is the way out.
        match worker.wait_for(id, Duration::MAX, cancel)? {
            Response::Done {
                text,
                finish_reason,
                ..
            } => match finish_reason {
                FinishReason::Stop => Ok(text),
                FinishReason::Length => {
                    bail!("the reply was cut off at {} tokens", req.max_tokens)
                }
                FinishReason::Cancelled => bail!(CANCELLED),
            },
            Response::Error { message, .. }
                if req.json_schema.is_some() && is_grammar_message(&message) =>
            {
                Err(GrammarError(message).into())
            }
            Response::Error { message, .. } => Err(anyhow!("built-in model: {message}")),
            other => bail!("unexpected reply to chat: {other:?}"),
        }
    }
}

// ----- provider ------------------------------------------------------------

/// Context the worker loads a model with: the catalog value on Macs with
/// 16 GB+ memory, 8k otherwise (the KV cache is ~150 KB per token).
pub fn effective_n_ctx(entry: &CatalogEntry) -> u32 {
    match local_models::system_ram_gb() {
        Some(gb) if gb < 16 => entry.n_ctx.min(8_192),
        _ => entry.n_ctx,
    }
}

pub struct BuiltinProvider {
    entry: &'static CatalogEntry,
    path: PathBuf,
    n_ctx: u32,
    llm: &'static BuiltinLlm,
}

impl BuiltinProvider {
    /// `model_id`: a catalog id; empty = the default (or any downloaded) one.
    pub fn new(app: &AppHandle, model_id: &str) -> Result<Self> {
        let models = LocalModels::new(app)?;
        Self::from_models(&models, model_id, BuiltinLlm::global())
    }

    pub fn from_models(
        models: &LocalModels,
        model_id: &str,
        llm: &'static BuiltinLlm,
    ) -> Result<Self> {
        let entry = if model_id.trim().is_empty() {
            models.first_available().ok_or_else(|| {
                anyhow!("No built-in model is downloaded. Download one in Settings, or pick another provider.")
            })?
        } else {
            local_models::catalog_entry(model_id.trim())
                .ok_or_else(|| anyhow!("unknown built-in model '{model_id}'"))?
        };
        let path = models.model_path(entry.id).map_err(|_| {
            anyhow!(
                "{} is not downloaded. Download it in Settings, or pick another model.",
                entry.name
            )
        })?;
        Ok(Self {
            entry,
            path,
            n_ctx: effective_n_ctx(entry),
            llm,
        })
    }

    pub fn model_label(&self) -> &'static str {
        self.entry.name
    }

    fn run(
        &self,
        messages: &[ChatMessage],
        schema: Option<&Value>,
        opts: &ChatOptions,
    ) -> Result<String> {
        self.llm.chat(
            &BuiltinChat {
                model_path: &self.path,
                n_ctx: self.n_ctx,
                messages,
                max_tokens: opts.max_tokens.unwrap_or(2048),
                temperature: opts.temperature.unwrap_or(0.2),
                json_schema: schema,
            },
            opts.cancel.as_deref(),
        )
    }
}

impl LlmProvider for BuiltinProvider {
    fn name(&self) -> String {
        format!("{BUILTIN_LLM_PROVIDER_ID}/{}", self.entry.id)
    }

    fn context_tokens(&self) -> usize {
        self.n_ctx as usize
    }

    fn chat(&self, messages: &[ChatMessage], opts: &ChatOptions) -> Result<String> {
        self.run(messages, None, opts)
            .map(|t| strip_think(&t).to_string())
    }

    /// GBNF grammar from the schema (llama.cpp's `json_schema_to_grammar`),
    /// so the reply is valid JSON by construction; prompt-and-retry only if
    /// the grammar couldn't be built or the reply is invalid anyway. Other
    /// failures (cut off, cancelled, timed out, the helper died) are returned:
    /// a retry would hit them again.
    fn chat_json(
        &self,
        messages: &[ChatMessage],
        json_schema: &Value,
        opts: &ChatOptions,
    ) -> Result<Value> {
        // The model still needs to know what the fields mean.
        let msgs = super::with_json_instruction(messages, json_schema);
        let error = match self.run(&msgs, Some(json_schema), opts) {
            Ok(reply) => {
                match extract_json(&reply).and_then(|v| validate_json(&v, json_schema).map(|_| v)) {
                    Ok(v) => return Ok(v),
                    Err(e) => e,
                }
            }
            Err(e) if e.chain().any(|c| c.is::<GrammarError>()) => e,
            Err(e) => return Err(e),
        };
        log::warn!(
            "{}: grammar-constrained JSON failed ({error}); retrying",
            self.name()
        );
        json_via_prompt(self, messages, json_schema, opts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_match_the_worker_wire_format() {
        // Same JSON as crates/guidy-llm/src/protocol.rs tests.
        let json = serde_json::to_value(Request::Cancel { id: None }).unwrap();
        assert_eq!(json, serde_json::json!({"op": "cancel", "id": null}));
        let chat: Request = serde_json::from_str(
            r#"{"op":"chat","id":3,"messages":[],"max_tokens":5,"temperature":0}"#,
        )
        .unwrap();
        assert!(matches!(
            chat,
            Request::Chat {
                json_schema: None,
                stream: false,
                ..
            }
        ));
        let done: Response = decode_frame(
            r#"{"type":"done","id":3,"text":"hi","finish_reason":"stop","prompt_tokens":4,"completion_tokens":1}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            done,
            Response::Done {
                id: 3,
                text: "hi".into(),
                finish_reason: FinishReason::Stop,
                prompt_tokens: 4,
                completion_tokens: 1
            }
        );
        assert!(decode_frame::<Response>("   ").is_none());
    }

    #[test]
    fn encoded_frames_are_single_lines() {
        let frame = encode_frame(&Request::Chat {
            id: 1,
            messages: vec![WireMessage {
                role: "user".into(),
                content: "a\nb\n".into(),
            }],
            max_tokens: 1,
            temperature: 0.0,
            json_schema: None,
            stream: false,
        })
        .unwrap();
        assert_eq!(frame.iter().filter(|&&b| b == b'\n').count(), 1);
        assert_eq!(*frame.last().unwrap(), b'\n');
        let back: Request = decode_frame(std::str::from_utf8(&frame).unwrap())
            .unwrap()
            .unwrap();
        assert!(matches!(back, Request::Chat { id: 1, .. }));
    }

    #[test]
    fn a_queued_chat_can_be_cancelled_while_waiting_for_the_worker() {
        let llm = std::sync::Arc::new(BuiltinLlm::new(Duration::from_secs(60)));
        // Another request holds the worker (a long generation).
        let held = llm.worker.lock();
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let waiter = {
            let (llm, cancel) = (llm.clone(), cancel.clone());
            std::thread::spawn(move || {
                let req = BuiltinChat {
                    model_path: Path::new("/nonexistent.gguf"),
                    n_ctx: 512,
                    messages: &[],
                    max_tokens: 1,
                    temperature: 0.0,
                    json_schema: None,
                };
                let _ = tx.send(llm.chat(&req, Some(&cancel)));
            })
        };
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
        cancel.store(true, Ordering::SeqCst);
        let err = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("cancel must end the wait for the lock")
            .unwrap_err();
        assert!(super::super::is_cancelled_error(&err), "{err:#}");
        waiter.join().unwrap();
        drop(held);
        // Already cancelled: refused right away even when the lock is free.
        assert!(llm.lock_worker(Some(&AtomicBool::new(true))).is_err());
        assert!(llm.lock_worker(None).is_ok());
    }

    #[test]
    fn missing_model_is_a_helpful_error() {
        let dir = tempfile::tempdir().unwrap();
        let models = LocalModels::at(dir.path());
        let llm: &'static BuiltinLlm = Box::leak(Box::new(BuiltinLlm::new(IDLE_TIMEOUT)));
        let err = BuiltinProvider::from_models(&models, "", llm)
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("No built-in model is downloaded"), "{err}");
        let err = BuiltinProvider::from_models(&models, "qwen3-8b-q4_k_m", llm)
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("not downloaded"), "{err}");
    }

    #[test]
    fn helper_is_looked_up_next_to_the_executable_only() {
        let exe = Path::new("/Applications/Guidy.app/Contents/MacOS/guidy");
        assert_eq!(
            binary_candidates(exe, false),
            vec![PathBuf::from("/Applications/Guidy.app/Contents/MacOS").join(bin_name())]
        );
        // Tests (target/debug/deps/<test>) also look one level up.
        let test_exe = Path::new("/r/target/debug/deps/guidy_core-1234");
        assert_eq!(
            binary_candidates(test_exe, true),
            vec![
                PathBuf::from("/r/target/debug/deps").join(bin_name()),
                PathBuf::from("/r/target/debug").join(bin_name()),
            ]
        );
        const { assert!(BIN_ENV_ALLOWED) };
    }

    /// The helper binary built by `cargo build -p guidy-llm`.
    fn worker_binary() -> Option<PathBuf> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/debug")
            .join(bin_name());
        p.is_file().then_some(p)
    }

    #[test]
    fn supervisor_talks_to_the_real_worker_without_a_model() {
        // Needs `cargo build -p guidy-llm`; skipped when the binary is absent.
        let Some(bin) = worker_binary() else {
            eprintln!("guidy-llm not built; skipping");
            return;
        };
        let llm: &'static BuiltinLlm = Box::leak(Box::new(BuiltinLlm::new(Duration::ZERO)));
        llm.set_binary(bin);
        llm.ping().unwrap();
        let err = llm
            .chat(
                &BuiltinChat {
                    model_path: Path::new("/nonexistent/model.gguf"),
                    n_ctx: 2048,
                    messages: &[ChatMessage::user("hi")],
                    max_tokens: 4,
                    temperature: 0.0,
                    json_schema: None,
                },
                None,
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("model file not found"), "{err}");
        // Idle timeout zero: the reaper stops it.
        assert!(llm.reap_if_idle());
        assert!(!llm.reap_if_idle());
    }

    #[test]
    fn timeouts_are_recognized_through_context() {
        let e = anyhow::Error::from(WorkerTimeout).context("ping");
        assert!(is_timeout_error(&e));
        assert!(e.to_string() == "ping");
        let other = anyhow!("the built-in model runtime did not answer in time");
        assert!(!is_timeout_error(&other), "decided by type, not text");
    }

    /// A fake `guidy-llm` (shell script) that records its pid in `pid` and,
    /// unless `body` replaces it, speaks enough of the protocol: `load`
    /// succeeds, `chat` answers "ok" after `chat_delay` seconds, `ping`
    /// runs `on_ping` (shell; empty = answer pong).
    #[cfg(unix)]
    struct FakeHelper {
        _dir: tempfile::TempDir,
        bin: PathBuf,
        pid_file: PathBuf,
    }

    #[cfg(unix)]
    impl FakeHelper {
        fn script(body: &str) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let dir = tempfile::tempdir().unwrap();
            let bin = dir.path().join("fake-guidy-llm");
            let pid_file = dir.path().join("pid");
            let script = format!("#!/bin/sh\necho $$ > '{}'\n{body}\n", pid_file.display());
            std::fs::write(&bin, script).unwrap();
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
            Self {
                _dir: dir,
                bin,
                pid_file,
            }
        }

        fn protocol(chat_delay: &str, on_ping: &str) -> Self {
            Self::with_chat(
                &format!(
                    r#"sleep {chat_delay}; echo "{{\"type\":\"done\",\"id\":$id,\"text\":\"ok\",\"finish_reason\":\"stop\",\"prompt_tokens\":1,\"completion_tokens\":1}}""#
                ),
                on_ping,
            )
        }

        /// Like [`Self::protocol`], with `on_chat` (shell, `$line` is the
        /// request, `$id` its id) answering `chat`.
        fn with_chat(on_chat: &str, on_ping: &str) -> Self {
            let pong = r#"echo "{\"type\":\"pong\",\"id\":$id}""#;
            let on_ping = if on_ping.is_empty() { pong } else { on_ping };
            Self::script(
                &r#"echo '{"type":"ready","version":"fake"}'
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/^{"op":"[a-z]*","id":\([0-9]*\).*/\1/p')
  case "$line" in
    '{"op":"load"'*) echo "{\"type\":\"loaded\",\"id\":$id,\"n_ctx\":512,\"n_ctx_train\":512}" ;;
    '{"op":"chat"'*) ON_CHAT ;;
    '{"op":"ping"'*) ON_PING ;;
    '{"op":"shutdown"'*) exit 0 ;;
  esac
done"#
                    .replace("ON_CHAT", on_chat)
                    .replace("ON_PING", on_ping),
            )
        }

        /// Pid of the most recently started instance.
        fn pid(&self) -> i32 {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Ok(s) = std::fs::read_to_string(&self.pid_file) {
                    if let Ok(pid) = s.trim().parse() {
                        return pid;
                    }
                }
                assert!(Instant::now() < deadline, "fake helper never started");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    /// Whether `pid` exists, zombies included (so `false` means killed
    /// *and* reaped).
    #[cfg(unix)]
    fn process_exists(pid: i32) -> bool {
        // SAFETY: signal 0 only checks for existence.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[cfg(unix)]
    fn fake_llm(helper: &FakeHelper, startup_timeout: Duration) -> BuiltinLlm {
        let llm = BuiltinLlm {
            startup_timeout,
            ..BuiltinLlm::new(Duration::from_secs(60))
        };
        llm.set_binary(helper.bin.clone());
        llm
    }

    #[cfg(unix)]
    fn fake_chat(model: &Path) -> BuiltinChat<'_> {
        BuiltinChat {
            model_path: model,
            n_ctx: 512,
            messages: &[],
            max_tokens: 1,
            temperature: 0.0,
            json_schema: None,
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_helper_that_never_gets_ready_is_killed_and_reaped() {
        let helper = FakeHelper::script("exec sleep 60");
        let err = Worker::spawn(&helper.bin, Duration::from_secs(2))
            .err()
            .expect("no Ready: spawn fails");
        assert!(is_timeout_error(&err), "{err:#}");
        assert!(!process_exists(helper.pid()));

        // Exits before Ready (disconnected): reaped too.
        let helper = FakeHelper::script("exit 3");
        assert!(Worker::spawn(&helper.bin, Duration::from_secs(5)).is_err());
        assert!(!process_exists(helper.pid()));
    }

    #[cfg(unix)]
    #[test]
    fn dropping_a_worker_kills_it() {
        let helper = FakeHelper::protocol("0", "");
        let mut worker = Worker::spawn(&helper.bin, Duration::from_secs(5)).unwrap();
        let pid = helper.pid();
        assert!(worker.alive());
        worker.kill();
        assert!(!process_exists(pid));
        drop(worker); // idempotent after an explicit kill
        let worker = Worker::spawn(&helper.bin, Duration::from_secs(5)).unwrap();
        let pid = helper.pid();
        drop(worker);
        assert!(!process_exists(pid));
    }

    #[cfg(unix)]
    #[test]
    fn an_unanswered_ping_kills_the_worker() {
        let helper = FakeHelper::protocol("0", ":");
        let llm = fake_llm(&helper, Duration::from_secs(2));
        let err = llm.ping().unwrap_err();
        assert!(is_timeout_error(&err), "{err:#}");
        assert!(llm.worker.lock().is_none());
        assert!(!process_exists(helper.pid()));
    }

    #[cfg(unix)]
    #[test]
    fn forgetting_the_model_mid_chat_stops_the_worker_when_the_chat_ends() {
        let helper = FakeHelper::protocol("1", "");
        let llm = std::sync::Arc::new(fake_llm(&helper, Duration::from_secs(5)));
        let model = PathBuf::from("/models/a.gguf");
        let chat = {
            let (llm, model) = (llm.clone(), model.clone());
            std::thread::spawn(move || llm.chat(&fake_chat(&model), None))
        };
        let pid = helper.pid();
        // Wait until the chat holds the worker.
        let deadline = Instant::now() + Duration::from_secs(5);
        while llm.worker.try_lock().is_some() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        llm.forget_model(&model);
        assert!(process_exists(pid), "the running chat isn't interrupted");
        assert_eq!(chat.join().unwrap().unwrap(), "ok");
        assert!(llm.worker.lock().is_none());
        assert!(!process_exists(pid));
        assert!(llm.forgotten.lock().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_forgotten_model_is_not_reused_by_the_next_request() {
        let helper = FakeHelper::protocol("0", "");
        let llm = fake_llm(&helper, Duration::from_secs(5));
        let (a, b) = (
            PathBuf::from("/models/a.gguf"),
            PathBuf::from("/models/b.gguf"),
        );
        llm.chat(&fake_chat(&a), None).unwrap();
        let first = helper.pid();

        // Another model forgotten: the worker stays.
        llm.forget_model(&b);
        assert!(llm.worker.lock().is_some());

        // Forgotten while someone holds the lock: the next request drops it.
        let held = llm.worker.lock();
        llm.forget_model(&a);
        drop(held);
        assert!(process_exists(first));
        llm.ping().unwrap();
        assert!(!process_exists(first));
        assert!(llm.worker.lock().as_ref().unwrap().loaded.is_none());
        assert_ne!(helper.pid(), first);
        llm.shutdown();
    }

    #[test]
    fn ping_does_not_wait_for_a_running_chat() {
        let llm = BuiltinLlm::new(Duration::from_secs(60));
        // A chat holds the worker for the whole generation.
        let held = llm.worker.lock();
        let start = Instant::now();
        assert_eq!(llm.ping().unwrap(), PingStatus::Busy);
        assert!(start.elapsed() < Duration::from_secs(2));
        drop(held);
    }

    /// A [`BuiltinProvider`] on a fake helper whose `chat` runs `on_chat`
    /// (see [`FakeHelper::with_chat`]) after counting the request in the
    /// returned file.
    #[cfg(unix)]
    fn fake_provider(on_chat: &str) -> (FakeHelper, BuiltinProvider, PathBuf) {
        let count = tempfile::NamedTempFile::new()
            .unwrap()
            .into_temp_path()
            .keep()
            .unwrap();
        let helper =
            FakeHelper::with_chat(&format!("echo x >> '{}'; {on_chat}", count.display()), "");
        let llm: &'static BuiltinLlm =
            Box::leak(Box::new(fake_llm(&helper, Duration::from_secs(5))));
        let provider = BuiltinProvider {
            entry: &local_models::CATALOG[0],
            path: PathBuf::from("/models/fake.gguf"),
            n_ctx: 512,
            llm,
        };
        (helper, provider, count)
    }

    #[cfg(unix)]
    fn chats(count: &Path) -> usize {
        std::fs::read_to_string(count)
            .unwrap_or_default()
            .lines()
            .count()
    }

    /// Shell printing a `done` reply with `text` (JSON-escaped, no `'`).
    #[cfg(unix)]
    fn done(text: &str, finish_reason: &str) -> String {
        format!(
            r#"printf '{{"type":"done","id":%s,"text":"%s","finish_reason":"{finish_reason}","prompt_tokens":1,"completion_tokens":1}}\n' "$id" '{text}'"#
        )
    }

    #[cfg(unix)]
    fn schema() -> Value {
        serde_json::json!({"type":"object","required":["a"],"properties":{"a":{"type":"string"}}})
    }

    #[cfg(unix)]
    #[test]
    fn chat_json_returns_a_cut_off_reply_without_retrying() {
        let (_helper, provider, count) = fake_provider(&done(r#"{\"a"#, "length"));
        let err = provider
            .chat_json(
                &[ChatMessage::user("x")],
                &schema(),
                &ChatOptions::default(),
            )
            .unwrap_err();
        assert!(err.to_string().contains("cut off"), "{err:#}");
        assert_eq!(chats(&count), 1);
        provider.llm.shutdown();
    }

    #[cfg(unix)]
    #[test]
    fn chat_json_returns_a_dead_worker_without_retrying() {
        let (_helper, provider, count) = fake_provider("exit 1");
        let err = provider
            .chat_json(
                &[ChatMessage::user("x")],
                &schema(),
                &ChatOptions::default(),
            )
            .unwrap_err();
        assert!(err.to_string().contains("exited"), "{err:#}");
        assert_eq!(chats(&count), 1);
    }

    #[cfg(unix)]
    #[test]
    fn chat_json_retries_without_a_grammar_for_schema_problems() {
        let valid = done(r#"{\"a\":\"b\"}"#, "stop");
        // The grammar can't be built; the prompt-only request succeeds.
        let (_helper, provider, count) = fake_provider(&format!(
            r#"if printf '%s' "$line" | grep -q '"json_schema":null'; then {valid}; else echo "{{\"type\":\"error\",\"id\":$id,\"message\":\"unsupported JSON schema: bad\"}}"; fi"#
        ));
        let v = provider
            .chat_json(
                &[ChatMessage::user("x")],
                &schema(),
                &ChatOptions::default(),
            )
            .unwrap();
        assert_eq!(v, serde_json::json!({"a":"b"}));
        assert_eq!(chats(&count), 2);
        provider.llm.shutdown();

        // Invalid JSON despite the grammar.
        let (_helper, provider, count) = fake_provider(&format!(
            r#"if printf '%s' "$line" | grep -q '"json_schema":null'; then {valid}; else {}; fi"#,
            done("nope", "stop")
        ));
        let v = provider
            .chat_json(
                &[ChatMessage::user("x")],
                &schema(),
                &ChatOptions::default(),
            )
            .unwrap();
        assert_eq!(v, serde_json::json!({"a":"b"}));
        assert_eq!(chats(&count), 2);
        provider.llm.shutdown();
    }

    /// End-to-end with a real GGUF. Run with
    /// `GUIDY_LLM_TEST_MODEL=/path/model.gguf cargo test -p guidy-core builtin -- --ignored`
    /// after `cargo build -p guidy-llm`.
    #[test]
    #[ignore]
    fn real_model_chat_and_json() {
        let bin = worker_binary().expect("build guidy-llm first");
        let model =
            PathBuf::from(std::env::var("GUIDY_LLM_TEST_MODEL").expect("set GUIDY_LLM_TEST_MODEL"));
        let llm: &'static BuiltinLlm = Box::leak(Box::new(BuiltinLlm::new(IDLE_TIMEOUT)));
        llm.set_binary(bin);
        let reply = llm
            .chat(
                &BuiltinChat {
                    model_path: &model,
                    n_ctx: 4096,
                    messages: &[ChatMessage::user("Say hello in one word.")],
                    max_tokens: 16,
                    temperature: 0.0,
                    json_schema: None,
                },
                None,
            )
            .unwrap();
        assert!(!reply.trim().is_empty());

        let schema = crate::meetings::summarize::summary_schema();
        let messages = crate::meetings::summarize::chunk_messages(
            "[00:00] Alice: Let's ship the beta on Friday.\n[00:05] Me: I'll write the release notes by Thursday.",
            &crate::meetings::summarize::PromptContext {
                user_name: "Me".into(),
                participants: vec!["Alice".into(), "Me".into()],
                meeting_title: Some("Beta sync".into()),
            },
            1,
            1,
        );
        let v = llm
            .chat(
                &BuiltinChat {
                    model_path: &model,
                    n_ctx: 4096,
                    messages: &super::super::with_json_instruction(&messages, &schema),
                    max_tokens: 1024,
                    temperature: 0.0,
                    json_schema: Some(&schema),
                },
                None,
            )
            .unwrap();
        let value = extract_json(&v).unwrap();
        validate_json(&value, &schema).unwrap();
        eprintln!("{value:#}");

        // Cancel kills the worker; the next request respawns it.
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            c2.store(true, Ordering::SeqCst);
        });
        let err = llm
            .chat(
                &BuiltinChat {
                    model_path: &model,
                    n_ctx: 4096,
                    messages: &[ChatMessage::user("Count from 1 to 500, one per line.")],
                    max_tokens: 3000,
                    temperature: 0.0,
                    json_schema: None,
                },
                Some(&cancel),
            )
            .unwrap_err();
        assert!(super::super::is_cancelled_error(&err));
        llm.ping().unwrap();
        llm.shutdown();
    }
}
