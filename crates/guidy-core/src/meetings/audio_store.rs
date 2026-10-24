//! Crash-safe Opus track files (`.gopus`) and audio retention.
//!
//! One file per track: `<app_data_dir>/meetings/audio/<meeting_id>-{mic,system}.gopus`.
//!
//! ## Format (version 2, all integers little-endian)
//!
//! ```text
//! header (20 bytes)
//!   magic            [u8; 8]  "GDYOPUS\0"
//!   version          u16      2
//!   sample_rate      u32      16000
//!   frame_samples    u16      320 (20 ms)
//!   packets_per_chunk u16     50 (1 s)
//!   pre_skip         u16      encoder lookahead, dropped when decoding
//! records, repeated
//!   len              u32      bytes after this field (4 + plaintext length)
//!   crc              u32      CRC-32 of the plaintext
//!   plaintext        [u8]
//! ```
//!
//! A record's plaintext is either an audio chunk (`0u8, n_packets: u16, then
//! n × (len: u16, opus packet)`) or the end marker (`1u8, total_samples: u64`)
//! written by [`TrackWriter::finish`]. Every audio chunk but the last holds
//! exactly `packets_per_chunk` packets, so record `i` starts at sample
//! `i × 16000` of the encoded stream and range reads can seek without
//! decoding earlier chunks.
//!
//! Chunks are appended as soon as they fill and fsync'd every few seconds, so
//! a crash loses at most the unsynced tail. [`TrackReader`] treats a short or
//! damaged (CRC mismatch) *final* record as the end of the file.
//!
//! Version 1 files were encrypted (a 28-byte header ending in a random nonce
//! prefix, then a 16-byte key check; records ChaCha20-Poly1305 with the
//! meetings data key). They are read only to convert them
//! ([`convert_legacy_track`]); [`TrackReader::open`] refuses them with
//! [`LegacyEncrypted`].

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

use crate::meetings::recorder::MeetingRepo;
use crate::meetings::types::{Meeting, MeetingId, MeetingStatus, Track};

pub const SAMPLE_RATE: u32 = 16_000;
pub const FRAME_SAMPLES: usize = 320; // 20 ms
pub const PACKETS_PER_CHUNK: usize = 50; // 1 s
pub const CHUNK_SAMPLES: u64 = (FRAME_SAMPLES * PACKETS_PER_CHUNK) as u64;
pub const BITRATE: i32 = 24_000;
pub const FILE_EXTENSION: &str = "gopus";

const MAGIC: &[u8; 8] = b"GDYOPUS\0";
const VERSION: u16 = 2;
/// Header of a version 2 file.
const HEADER_LEN: usize = 20;
/// The encrypted version 1: its fixed header (authenticated as AAD), then
/// the key check.
const LEGACY_VERSION: u16 = 1;
const LEGACY_FIXED_HEADER_LEN: usize = 28;
const LEGACY_HEADER_LEN: usize = LEGACY_FIXED_HEADER_LEN + 16;
/// fsync after this many chunks (seconds of audio).
const SYNC_EVERY_CHUNKS: u32 = 5;
/// Sanity bound on a record length; anything larger is a torn write.
const MAX_RECORD_LEN: u32 = 1 << 20;
const MAX_PACKET_BYTES: usize = 1275;

const KIND_AUDIO: u8 = 0;
const KIND_END: u8 = 1;

/// A version 1 (encrypted) file was encrypted with a different key (or its
/// header is damaged).
#[derive(Debug)]
pub struct WrongKey;

impl std::fmt::Display for WrongKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "meeting audio: wrong key or damaged header")
    }
}

impl std::error::Error for WrongKey {}

/// [`TrackReader::open`] found a version 1 (encrypted) file: it must be
/// converted first ([`convert_legacy_track`]).
#[derive(Debug)]
pub struct LegacyEncrypted;

impl std::fmt::Display for LegacyEncrypted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "meeting audio: still in the old encrypted format")
    }
}

impl std::error::Error for LegacyEncrypted {}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// `<app_data_dir>/meetings/audio`.
pub fn audio_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("meetings").join("audio")
}

/// File name (relative to [`audio_dir`]) of one track of a meeting.
pub fn track_file_name(meeting_id: MeetingId, track: Track) -> String {
    format!("{meeting_id}-{}.{FILE_EXTENSION}", track.as_str())
}

/// Resolve a stored relative path, refusing anything that escapes `audio_dir`.
pub fn resolve_track_path(audio_dir: &Path, rel: &str) -> Result<PathBuf> {
    let p = Path::new(rel);
    if p.is_absolute()
        || p.components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        bail!("invalid meeting audio path {rel:?}");
    }
    Ok(audio_dir.join(p))
}

/// Delete a meeting's track files (missing files are fine). Paths that
/// escape `audio_dir` are never touched. Every track is attempted; the first
/// error is returned after that. Returns how many files were removed.
pub fn delete_track_files(audio_dir: &Path, meeting: &Meeting) -> Result<usize> {
    let mut n = 0;
    let mut first_err = None;
    for rel in [&meeting.mic_audio_path, &meeting.system_audio_path]
        .into_iter()
        .flatten()
    {
        let removed =
            resolve_track_path(audio_dir, rel).and_then(|path| match fs::remove_file(&path) {
                Ok(()) => Ok(true),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
                Err(e) => Err(e).with_context(|| format!("delete {}", path.display())),
            });
        match removed {
            Ok(true) => n += 1,
            Ok(false) => {}
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    match first_err {
        Some(e) => Err(e),
        None => Ok(n),
    }
}

// ---------------------------------------------------------------------------
// Header / record helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Header {
    sample_rate: u32,
    frame_samples: u16,
    packets_per_chunk: u16,
    pre_skip: u16,
}

impl Header {
    fn bytes(&self) -> [u8; HEADER_LEN] {
        let mut b = [0u8; HEADER_LEN];
        b[0..8].copy_from_slice(MAGIC);
        b[8..10].copy_from_slice(&VERSION.to_le_bytes());
        b[10..14].copy_from_slice(&self.sample_rate.to_le_bytes());
        b[14..16].copy_from_slice(&self.frame_samples.to_le_bytes());
        b[16..18].copy_from_slice(&self.packets_per_chunk.to_le_bytes());
        b[18..20].copy_from_slice(&self.pre_skip.to_le_bytes());
        b
    }

    /// The fields after magic and version (the same in both versions).
    fn parse_fields(b: &[u8]) -> Result<Self> {
        let h = Header {
            sample_rate: u32::from_le_bytes(b[10..14].try_into().unwrap()),
            frame_samples: u16::from_le_bytes([b[14], b[15]]),
            packets_per_chunk: u16::from_le_bytes([b[16], b[17]]),
            pre_skip: u16::from_le_bytes([b[18], b[19]]),
        };
        if h.sample_rate != SAMPLE_RATE || h.frame_samples == 0 || h.packets_per_chunk == 0 {
            bail!("unsupported meeting audio parameters");
        }
        Ok(h)
    }

    fn chunk_samples(&self) -> u64 {
        self.frame_samples as u64 * self.packets_per_chunk as u64
    }
}

/// A file's version, from its first 10 bytes.
fn parse_version(b: &[u8]) -> Result<u16> {
    if b.len() < 10 || &b[0..8] != MAGIC {
        bail!("not a meeting audio file");
    }
    Ok(u16::from_le_bytes([b[8], b[9]]))
}

/// Whether the file at `path` is a version 1 (encrypted) track.
pub fn is_legacy_track(path: &Path) -> Result<bool> {
    let mut head = [0u8; 10];
    File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .with_context(|| format!("read {}", path.display()))?;
    Ok(parse_version(&head)? == LEGACY_VERSION)
}

/// The decryption of a version 1 file.
struct LegacyCrypto {
    cipher: ChaCha20Poly1305,
    nonce_prefix: [u8; 8],
    aad: [u8; LEGACY_FIXED_HEADER_LEN],
}

impl LegacyCrypto {
    fn nonce(&self, counter: u32) -> [u8; 12] {
        let mut n = [0u8; 12];
        n[..8].copy_from_slice(&self.nonce_prefix);
        n[8..].copy_from_slice(&counter.to_be_bytes());
        n
    }

    fn open(&self, counter: u32, ct: &[u8]) -> Option<Vec<u8>> {
        let nonce = self.nonce(counter);
        self.cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: ct,
                    aad: &self.aad,
                },
            )
            .ok()
    }

    #[cfg(test)]
    fn seal(&self, counter: u32, msg: &[u8]) -> Vec<u8> {
        let nonce = self.nonce(counter);
        self.cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg,
                    aad: &self.aad,
                },
            )
            .expect("encrypt")
    }
}

/// How a file's records are stored.
enum Records {
    /// Version 2: `crc || plaintext`.
    Plain,
    /// Version 1: ChaCha20-Poly1305 per record.
    Legacy(LegacyCrypto),
}

impl Records {
    /// The plaintext of record `idx` from its stored bytes (`None` when
    /// damaged).
    fn open(&self, idx: u32, stored: &[u8]) -> Option<Vec<u8>> {
        match self {
            Records::Plain => {
                let (crc, pt) = stored.split_at_checked(4)?;
                (u32::from_le_bytes(crc.try_into().ok()?) == crc32fast::hash(pt))
                    .then(|| pt.to_vec())
            }
            Records::Legacy(c) => c.open(idx, stored),
        }
    }

    /// Bounds of a stored record's length.
    fn len_range(&self) -> std::ops::RangeInclusive<u32> {
        match self {
            // The CRC and at least the kind byte.
            Records::Plain => 5..=MAX_RECORD_LEN,
            // At least the AEAD tag.
            Records::Legacy(_) => 16..=MAX_RECORD_LEN,
        }
    }
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

/// What a finished track contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackSummary {
    /// Real (unpadded) samples written, at 16 kHz.
    pub samples: u64,
}

impl TrackSummary {
    pub fn duration_ms(&self) -> u64 {
        self.samples * 1000 / SAMPLE_RATE as u64
    }
}

/// Appends records to a new `.gopus` file.
struct RecordWriter {
    path: PathBuf,
    file: File,
    header: Header,
}

impl RecordWriter {
    /// Create (or truncate) `path`, owner-only, and write the header.
    fn create(path: &Path, header: Header) -> Result<Self> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        let mut opts = OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts
            .open(path)
            .with_context(|| format!("create {}", path.display()))?;
        // Owner-only even when truncating a file that existed with a wider mode.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(&header.bytes())?;
        file.sync_all()?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            header,
        })
    }

    fn write(&mut self, plaintext: &[u8]) -> Result<()> {
        let mut rec = Vec::with_capacity(8 + plaintext.len());
        rec.extend_from_slice(&(4 + plaintext.len() as u32).to_le_bytes());
        rec.extend_from_slice(&crc32fast::hash(plaintext).to_le_bytes());
        rec.extend_from_slice(plaintext);
        // One write per record keeps a torn write confined to the tail.
        self.file
            .write_all(&rec)
            .with_context(|| format!("write {}", self.path.display()))
    }

    fn write_audio(&mut self, packets: &[Vec<u8>]) -> Result<()> {
        let mut pt = Vec::with_capacity(3 + packets.iter().map(|p| p.len() + 2).sum::<usize>());
        pt.push(KIND_AUDIO);
        pt.extend_from_slice(&(packets.len() as u16).to_le_bytes());
        for p in packets {
            pt.extend_from_slice(&(p.len() as u16).to_le_bytes());
            pt.extend_from_slice(p);
        }
        self.write(&pt)
    }

    fn write_end(&mut self, total_samples: u64) -> Result<()> {
        let mut end = vec![KIND_END];
        end.extend_from_slice(&total_samples.to_le_bytes());
        self.write(&end)
    }

    fn sync_data(&mut self) -> Result<()> {
        self.file.sync_data().context("fsync meeting audio")
    }

    fn sync_all(&mut self) -> Result<()> {
        self.file.sync_all().context("fsync meeting audio")
    }
}

/// Encodes 16 kHz mono f32 into a `.gopus` file as it arrives.
pub struct TrackWriter {
    out: RecordWriter,
    encoder: opus::Encoder,
    frame: Vec<f32>,
    packets: Vec<Vec<u8>>,
    chunks_since_sync: u32,
    samples: u64,
}

impl TrackWriter {
    /// Create (or truncate) `path` and write the header.
    pub fn create(path: &Path) -> Result<Self> {
        let mut encoder =
            opus::Encoder::new(SAMPLE_RATE, opus::Channels::Mono, opus::Application::Voip)
                .context("opus encoder")?;
        encoder
            .set_bitrate(opus::Bitrate::Bits(BITRATE))
            .context("opus bitrate")?;
        let pre_skip = encoder.get_lookahead().context("opus lookahead")?.max(0) as u16;
        let header = Header {
            sample_rate: SAMPLE_RATE,
            frame_samples: FRAME_SAMPLES as u16,
            packets_per_chunk: PACKETS_PER_CHUNK as u16,
            pre_skip,
        };
        Ok(Self {
            out: RecordWriter::create(path, header)?,
            encoder,
            frame: Vec::with_capacity(FRAME_SAMPLES),
            packets: Vec::with_capacity(PACKETS_PER_CHUNK),
            chunks_since_sync: 0,
            samples: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.out.path
    }

    /// Real samples pushed so far.
    pub fn samples_written(&self) -> u64 {
        self.samples
    }

    /// Append 16 kHz mono samples.
    pub fn push(&mut self, samples: &[f32]) -> Result<()> {
        self.samples += samples.len() as u64;
        self.push_raw(samples)
    }

    fn push_raw(&mut self, mut samples: &[f32]) -> Result<()> {
        while !samples.is_empty() {
            let take = (FRAME_SAMPLES - self.frame.len()).min(samples.len());
            self.frame.extend_from_slice(&samples[..take]);
            samples = &samples[take..];
            if self.frame.len() == FRAME_SAMPLES {
                self.encode_frame()?;
            }
        }
        Ok(())
    }

    fn encode_frame(&mut self) -> Result<()> {
        for s in &mut self.frame {
            *s = if s.is_finite() {
                s.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
        let packet = self
            .encoder
            .encode_vec_float(&self.frame, MAX_PACKET_BYTES)
            .context("opus encode")?;
        self.frame.clear();
        self.packets.push(packet);
        if self.packets.len() == PACKETS_PER_CHUNK {
            self.write_audio_chunk()?;
        }
        Ok(())
    }

    fn write_audio_chunk(&mut self) -> Result<()> {
        if self.packets.is_empty() {
            return Ok(());
        }
        self.out.write_audio(&self.packets)?;
        self.packets.clear();
        self.chunks_since_sync += 1;
        if self.chunks_since_sync >= SYNC_EVERY_CHUNKS {
            self.sync()?;
        }
        Ok(())
    }

    /// fsync what has been written so far.
    pub fn sync(&mut self) -> Result<()> {
        self.chunks_since_sync = 0;
        self.out.sync_data()
    }

    /// Flush the encoder tail, write the final chunk and the end marker, and
    /// fsync. Dropping a writer without calling this leaves a file that reads
    /// like a crash (everything but the partial chunk).
    pub fn finish(mut self) -> Result<TrackSummary> {
        // Push the encoder's lookahead out, then pad the last frame.
        let tail = vec![0.0f32; self.out.header.pre_skip as usize];
        self.push_raw(&tail)?;
        if !self.frame.is_empty() {
            self.frame.resize(FRAME_SAMPLES, 0.0);
            self.encode_frame()?;
        }
        self.write_audio_chunk()?;
        self.out.write_end(self.samples)?;
        self.out.sync_all()?;
        Ok(TrackSummary {
            samples: self.samples,
        })
    }
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

enum Record {
    Audio(Vec<Vec<u8>>),
    End(u64),
}

fn parse_record(pt: &[u8]) -> Option<Record> {
    let (&kind, rest) = pt.split_first()?;
    match kind {
        KIND_AUDIO => {
            let n = u16::from_le_bytes(rest.get(0..2)?.try_into().ok()?) as usize;
            let mut rest = &rest[2..];
            let mut packets = Vec::with_capacity(n);
            for _ in 0..n {
                let len = u16::from_le_bytes(rest.get(0..2)?.try_into().ok()?) as usize;
                packets.push(rest.get(2..2 + len)?.to_vec());
                rest = &rest[2 + len..];
            }
            Some(Record::Audio(packets))
        }
        KIND_END => Some(Record::End(u64::from_le_bytes(
            rest.get(0..8)?.try_into().ok()?,
        ))),
        _ => None,
    }
}

/// Decodes a `.gopus` file back to 16 kHz mono f32.
pub struct TrackReader {
    file: File,
    header: Header,
    records: Records,
    /// `(offset of the stored record, len)` of every complete audio record.
    chunks: Vec<(u64, u32)>,
    /// Real sample count from the end marker (absent after a crash).
    total_samples: Option<u64>,
}

impl TrackReader {
    /// Open a track and index its chunks. A version 1 (encrypted) file is
    /// [`LegacyEncrypted`] (downcastable).
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let mut head = [0u8; HEADER_LEN];
        file.read_exact(&mut head)
            .with_context(|| format!("{}: truncated header", path.display()))?;
        match parse_version(&head)? {
            VERSION => {}
            LEGACY_VERSION => return Err(LegacyEncrypted.into()),
            v => bail!("unsupported meeting audio version {v}"),
        }
        let header = Header::parse_fields(&head)?;
        Self::index(path, file, header, Records::Plain, HEADER_LEN)
    }

    /// Open a version 1 (encrypted) track with the meetings data key it was
    /// written with. Errors with [`WrongKey`] (downcastable) when it isn't.
    pub fn open_legacy(path: &Path, key: &[u8; 32]) -> Result<Self> {
        let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let mut head = [0u8; LEGACY_HEADER_LEN];
        file.read_exact(&mut head)
            .with_context(|| format!("{}: truncated header", path.display()))?;
        let version = parse_version(&head)?;
        if version != LEGACY_VERSION {
            bail!("not an encrypted meeting audio file (version {version})");
        }
        let header = Header::parse_fields(&head)?;
        let crypto = LegacyCrypto {
            cipher: ChaCha20Poly1305::new(Key::from_slice(key)),
            nonce_prefix: head[20..28].try_into().unwrap(),
            aad: head[..LEGACY_FIXED_HEADER_LEN].try_into().unwrap(),
        };
        // The key check: the AEAD tag of an empty message, counter u32::MAX.
        if crypto
            .open(u32::MAX, &head[LEGACY_FIXED_HEADER_LEN..])
            .is_none()
        {
            return Err(WrongKey.into());
        }
        Self::index(
            path,
            file,
            header,
            Records::Legacy(crypto),
            LEGACY_HEADER_LEN,
        )
    }

    /// Index the records after the header by their length prefixes.
    fn index(
        path: &Path,
        mut file: File,
        header: Header,
        records: Records,
        header_len: usize,
    ) -> Result<Self> {
        let file_len = file.metadata()?.len();
        let mut chunks: Vec<(u64, u32)> = Vec::new();
        let mut pos = header_len as u64;
        while pos + 4 <= file_len {
            let mut len = [0u8; 4];
            file.seek(SeekFrom::Start(pos))?;
            file.read_exact(&mut len)?;
            let len = u32::from_le_bytes(len);
            if !records.len_range().contains(&len) || pos + 4 + len as u64 > file_len {
                break; // torn final write
            }
            chunks.push((pos + 4, len));
            pos += 4 + len as u64;
        }

        let mut reader = Self {
            file,
            header,
            records,
            chunks,
            total_samples: None,
        };
        // The last record is either the end marker, a (possibly torn) audio
        // chunk, or garbage from a torn write.
        if let Some(&(off, len)) = reader.chunks.last() {
            let idx = (reader.chunks.len() - 1) as u32;
            match reader.read_record(idx, off, len)? {
                Some(Record::End(total)) => {
                    reader.total_samples = Some(total);
                    reader.chunks.pop();
                }
                Some(Record::Audio(_)) => {}
                None => {
                    log::warn!(
                        "{}: dropping an unreadable final chunk (interrupted recording?)",
                        path.display()
                    );
                    reader.chunks.pop();
                }
            }
        }
        Ok(reader)
    }

    fn read_record(&mut self, idx: u32, off: u64, len: u32) -> Result<Option<Record>> {
        let mut stored = vec![0u8; len as usize];
        self.file.seek(SeekFrom::Start(off))?;
        self.file.read_exact(&mut stored)?;
        Ok(self
            .records
            .open(idx, &stored)
            .and_then(|pt| parse_record(&pt)))
    }

    fn audio_chunk(&mut self, idx: usize) -> Result<Vec<Vec<u8>>> {
        let (off, len) = self.chunks[idx];
        match self.read_record(idx as u32, off, len)? {
            Some(Record::Audio(p)) => Ok(p),
            _ => bail!("meeting audio: chunk {idx} is damaged"),
        }
    }

    /// True when the file was finalized (not interrupted by a crash).
    pub fn is_complete(&self) -> bool {
        self.total_samples.is_some()
    }

    fn pre_skip(&self) -> u64 {
        self.header.pre_skip as u64
    }

    /// Real samples in the track.
    pub fn total_samples(&mut self) -> Result<u64> {
        if let Some(t) = self.total_samples {
            return Ok(t);
        }
        let Some(last) = self.chunks.len().checked_sub(1) else {
            return Ok(0);
        };
        let last_packets = self.audio_chunk(last)?.len() as u64;
        let encoded = last as u64 * self.header.chunk_samples()
            + last_packets * self.header.frame_samples as u64;
        Ok(encoded.saturating_sub(self.pre_skip()))
    }

    pub fn duration_ms(&mut self) -> Result<u64> {
        Ok(self.total_samples()? * 1000 / SAMPLE_RATE as u64)
    }

    /// Decode chunks `[first, last]` into encoded-stream samples.
    fn decode_chunks(&mut self, first: usize, last: usize) -> Result<Vec<f32>> {
        let mut decoder =
            opus::Decoder::new(SAMPLE_RATE, opus::Channels::Mono).context("opus decoder")?;
        let frame = self.header.frame_samples as usize;
        let mut out = Vec::with_capacity((last + 1 - first) * self.header.chunk_samples() as usize);
        let mut buf = vec![0f32; 5760];
        for idx in first..=last {
            for packet in self.audio_chunk(idx)? {
                let n = match decoder.decode_float(&packet, &mut buf, false) {
                    Ok(n) => n,
                    Err(e) => {
                        log::warn!("meeting audio: bad opus packet in chunk {idx}: {e}");
                        buf[..frame].fill(0.0);
                        frame
                    }
                };
                out.extend_from_slice(&buf[..n]);
            }
        }
        Ok(out)
    }

    /// Decode the whole track.
    pub fn read_all(&mut self) -> Result<Vec<f32>> {
        if self.chunks.is_empty() {
            return Ok(Vec::new());
        }
        let total = self.total_samples()?;
        let decoded = self.decode_chunks(0, self.chunks.len() - 1)?;
        let skip = (self.pre_skip() as usize).min(decoded.len());
        let mut out = decoded[skip..].to_vec();
        out.truncate(total as usize);
        Ok(out)
    }

    /// Decode `[t0_ms, t1_ms)`, clipped to the track.
    pub fn read_range(&mut self, t0_ms: i64, t1_ms: i64) -> Result<Vec<f32>> {
        let total = self.total_samples()?;
        let to_sample = |ms: i64| ((ms.max(0) as u64) * SAMPLE_RATE as u64 / 1000).min(total);
        let (s0, s1) = (to_sample(t0_ms), to_sample(t1_ms));
        if s1 <= s0 || self.chunks.is_empty() {
            return Ok(Vec::new());
        }
        let cs = self.header.chunk_samples();
        let (e0, e1) = (s0 + self.pre_skip(), s1 + self.pre_skip());
        let last_chunk = self.chunks.len() - 1;
        // One chunk of pre-roll so the decoder state has settled by e0.
        let first = ((e0 / cs) as usize).saturating_sub(1).min(last_chunk);
        let last = (((e1 - 1) / cs) as usize).min(last_chunk);
        let decoded = self.decode_chunks(first, last)?;
        let base = first as u64 * cs;
        let a = ((e0 - base) as usize).min(decoded.len());
        let b = ((e1 - base) as usize).min(decoded.len());
        Ok(decoded[a..b].to_vec())
    }
}

/// Write the version 1 (encrypted) track at `src` to `dst` as version 2,
/// copying its Opus packets as they are (no re-encoding): the same audio,
/// chunk for chunk, and the end marker only if `src` had one. Returns the
/// track's sample count, which `dst` is checked to read back the same.
pub fn convert_legacy_track(src: &Path, dst: &Path, key: &[u8; 32]) -> Result<u64> {
    let mut reader = TrackReader::open_legacy(src, key)?;
    let total = reader.total_samples()?;
    let mut out = RecordWriter::create(dst, reader.header)?;
    for idx in 0..reader.chunks.len() {
        out.write_audio(&reader.audio_chunk(idx)?)?;
    }
    if let Some(total) = reader.total_samples {
        out.write_end(total)?;
    }
    out.sync_all()?;
    drop(out);
    let mut check = TrackReader::open(dst)?;
    let (chunks, samples) = (check.chunks.len(), check.total_samples()?);
    if chunks != reader.chunks.len() || samples != total {
        bail!(
            "{}: converted copy reads {chunks} chunks / {samples} samples, not {} / {total}",
            dst.display(),
            reader.chunks.len()
        );
    }
    Ok(total)
}

/// `src` (version 2) written as a version 1 file encrypted with `key`,
/// as the app wrote them before.
#[cfg(test)]
pub(crate) fn write_legacy(src: &Path, dst: &Path, key: &[u8; 32]) {
    let mut r = TrackReader::open(src).unwrap();
    let mut aad = [0u8; LEGACY_FIXED_HEADER_LEN];
    aad[..HEADER_LEN].copy_from_slice(&r.header.bytes());
    aad[8..10].copy_from_slice(&LEGACY_VERSION.to_le_bytes());
    aad[20..28].copy_from_slice(&[3u8; 8]);
    let crypto = LegacyCrypto {
        cipher: ChaCha20Poly1305::new(Key::from_slice(key)),
        nonce_prefix: [3u8; 8],
        aad,
    };
    let mut out = aad.to_vec();
    out.extend_from_slice(&crypto.seal(u32::MAX, &[]));
    let mut records: Vec<Vec<u8>> = Vec::new();
    for i in 0..r.chunks.len() {
        let (off, len) = r.chunks[i];
        let mut stored = vec![0u8; len as usize];
        r.file.seek(SeekFrom::Start(off)).unwrap();
        r.file.read_exact(&mut stored).unwrap();
        records.push(stored[4..].to_vec());
    }
    if let Some(total) = r.total_samples {
        let mut end = vec![KIND_END];
        end.extend_from_slice(&total.to_le_bytes());
        records.push(end);
    }
    for (i, pt) in records.iter().enumerate() {
        let ct = crypto.seal(i as u32, pt);
        out.extend_from_slice(&(ct.len() as u32).to_le_bytes());
        out.extend_from_slice(&ct);
    }
    fs::write(dst, out).unwrap();
}

// ---------------------------------------------------------------------------
// Retention (D13)
// ---------------------------------------------------------------------------

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// What [`apply_retention`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetentionOutcome {
    pub files_deleted: usize,
    /// Meetings whose audio paths were cleared.
    pub meetings: Vec<MeetingId>,
}

/// Delete raw audio older than `retention_days` (`None` = keep forever) and
/// clear the meetings' audio paths. Only fully processed meetings
/// (`processing_stage = done`) lose their audio: recording, processing,
/// queued/running in the processor (`is_busy`), failed or never-processed
/// meetings keep it, since processing needs it. Paths are cleared with a
/// status-guarded targeted update *before* the files are deleted; queueing
/// a meeting marks it `Processing` first, so one queued after the checks
/// here is still left alone. Unreferenced
/// `.gopus` files older than the cutoff (left by a failed start) are removed
/// too. Voiceprints are never touched: they live in the database, not here.
pub fn apply_retention(
    repo: &dyn MeetingRepo,
    audio_dir: &Path,
    retention_days: Option<u32>,
    now_ms: i64,
    is_busy: &dyn Fn(MeetingId) -> bool,
) -> Result<RetentionOutcome> {
    let mut out = RetentionOutcome::default();
    let Some(days) = retention_days else {
        return Ok(out);
    };
    let cutoff = now_ms - days as i64 * DAY_MS;
    // Both read before any path is cleared, so files cleared below whose
    // deletion fails are swept as orphans on a later run, not this one.
    let candidates = repo.meetings_with_audio_older_than(cutoff)?;
    let referenced: std::collections::HashSet<String> = repo.audio_paths()?.into_iter().collect();

    // Candidates have audio, aren't recording and are past the cutoff.
    for meeting in &candidates {
        let processed = meeting.status != MeetingStatus::Processing
            && meeting.processing_stage.as_deref() == Some("done");
        if !processed || is_busy(meeting.id) {
            continue;
        }
        // Guarded by status in SQL: false when it was queued or started
        // processing (or was deleted) since the list was read.
        match repo.clear_audio_paths_if_processed(meeting.id) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(e) => {
                log::warn!("retention: meeting {}: {e:#}", meeting.id);
                continue;
            }
        }
        out.meetings.push(meeting.id);
        match delete_track_files(audio_dir, meeting) {
            Ok(n) => {
                out.files_deleted += n;
                log::info!("retention: deleted audio of meeting {}", meeting.id);
            }
            // The paths are already cleared; leftovers become orphans that
            // the sweep below removes on a later run.
            Err(e) => log::warn!("retention: meeting {}: {e:#}", meeting.id),
        }
    }

    // Orphans: files no meeting references.
    if let Ok(entries) = fs::read_dir(audio_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.extension().and_then(|e| e.to_str()) != Some(FILE_EXTENSION)
                || referenced.contains(name.as_str())
            {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .map(system_time_ms);
            if modified.is_some_and(|m| m < cutoff) && fs::remove_file(&path).is_ok() {
                out.files_deleted += 1;
                log::info!("retention: deleted orphaned {}", path.display());
            }
        }
    }
    Ok(out)
}

pub(crate) fn system_time_ms(t: std::time::SystemTime) -> i64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Last-write time of a file, in unix ms.
pub fn file_mtime_ms(path: &Path) -> Option<i64> {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .map(system_time_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::recorder::ports::fakes::MemRepo;

    const KEY: [u8; 32] = [7u8; 32];

    fn tone(secs: f32) -> Vec<f32> {
        let n = (secs * SAMPLE_RATE as f32) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                0.4 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
                    + 0.2 * (2.0 * std::f32::consts::PI * 1250.0 * t).sin()
            })
            .collect()
    }

    fn correlation(a: &[f32], b: &[f32]) -> f64 {
        let n = a.len().min(b.len());
        let (mut ab, mut aa, mut bb) = (0f64, 0f64, 0f64);
        for i in 0..n {
            ab += a[i] as f64 * b[i] as f64;
            aa += (a[i] as f64).powi(2);
            bb += (b[i] as f64).powi(2);
        }
        ab / (aa.sqrt() * bb.sqrt()).max(1e-12)
    }

    fn snr_db(reference: &[f32], decoded: &[f32]) -> f64 {
        let n = reference.len().min(decoded.len());
        let sig: f64 = reference[..n].iter().map(|x| (*x as f64).powi(2)).sum();
        let noise: f64 = (0..n)
            .map(|i| (reference[i] as f64 - decoded[i] as f64).powi(2))
            .sum();
        10.0 * (sig / noise.max(1e-12)).log10()
    }

    fn write(path: &Path, samples: &[f32], finish: bool) -> Option<TrackSummary> {
        let mut w = TrackWriter::create(path).unwrap();
        // Uneven pushes, like audio callbacks.
        for c in samples.chunks(437) {
            w.push(c).unwrap();
        }
        if finish {
            Some(w.finish().unwrap())
        } else {
            drop(w);
            None
        }
    }

    #[test]
    fn round_trip_preserves_the_signal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("1-mic.gopus");
        let input = tone(3.3);
        let summary = write(&path, &input, true).unwrap();
        assert_eq!(summary.samples, input.len() as u64);
        assert_eq!(summary.duration_ms(), 3300);

        let mut r = TrackReader::open(&path).unwrap();
        assert!(r.is_complete());
        assert_eq!(r.duration_ms().unwrap(), 3300);
        let out = r.read_all().unwrap();
        assert_eq!(out.len(), input.len());
        // Skip the encoder's first frames (attack) when judging quality.
        let c = correlation(&input[1600..], &out[1600..]);
        let snr = snr_db(&input[1600..], &out[1600..]);
        assert!(c > 0.95, "correlation {c}");
        assert!(snr > 10.0, "snr {snr} dB");

        // ~24 kbps: 3.3 s should be well under 20 KB including overhead.
        let size = fs::metadata(&path).unwrap().len();
        assert!(size < 20_000, "file is {size} bytes");
    }

    #[test]
    fn an_encrypted_track_converts_to_the_same_audio() {
        let dir = tempfile::tempdir().unwrap();
        let (plain, legacy, converted) = (
            dir.path().join("p.gopus"),
            dir.path().join("1-mic.gopus"),
            dir.path().join("1-mic.gopus.plain"),
        );
        let input = tone(2.3);
        write(&plain, &input, true);
        write_legacy(&plain, &legacy, &KEY);
        assert!(is_legacy_track(&legacy).unwrap());
        assert!(!is_legacy_track(&plain).unwrap());
        // The app's reader refuses it until it is converted.
        let err = TrackReader::open(&legacy).err().expect("must fail");
        assert!(err.downcast_ref::<LegacyEncrypted>().is_some(), "{err:#}");

        let samples = convert_legacy_track(&legacy, &converted, &KEY).unwrap();
        assert_eq!(samples, input.len() as u64);
        // Packet for packet: the very bytes of the original plain file.
        assert_eq!(fs::read(&converted).unwrap(), fs::read(&plain).unwrap());
        let mut r = TrackReader::open(&converted).unwrap();
        assert!(r.is_complete());
        assert_eq!(
            r.read_all().unwrap(),
            TrackReader::open(&plain).unwrap().read_all().unwrap()
        );
    }

    #[test]
    fn an_interrupted_encrypted_track_converts_without_an_end_marker() {
        let dir = tempfile::tempdir().unwrap();
        let (plain, legacy, converted) = (
            dir.path().join("p.gopus"),
            dir.path().join("l.gopus"),
            dir.path().join("c.gopus"),
        );
        write(&plain, &tone(2.5), false);
        write_legacy(&plain, &legacy, &KEY);
        convert_legacy_track(&legacy, &converted, &KEY).unwrap();
        let mut r = TrackReader::open(&converted).unwrap();
        assert!(!r.is_complete());
        assert_eq!(
            r.total_samples().unwrap(),
            TrackReader::open(&plain).unwrap().total_samples().unwrap()
        );
    }

    #[test]
    fn wrong_key_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let (plain, legacy) = (dir.path().join("p.gopus"), dir.path().join("l.gopus"));
        write(&plain, &tone(1.5), true);
        write_legacy(&plain, &legacy, &KEY);
        let err = TrackReader::open_legacy(&legacy, &[8u8; 32])
            .err()
            .expect("must fail");
        assert!(err.downcast_ref::<WrongKey>().is_some(), "{err:#}");
        assert!(convert_legacy_track(&legacy, &dir.path().join("c.gopus"), &[8u8; 32]).is_err());
    }

    #[test]
    fn tampered_chunk_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.gopus");
        write(&path, &tone(3.0), true);
        let mut bytes = fs::read(&path).unwrap();
        bytes[HEADER_LEN + 10] ^= 0xff; // inside the first chunk
        fs::write(&path, &bytes).unwrap();
        let mut r = TrackReader::open(&path).unwrap();
        assert!(r.read_all().is_err());
    }

    #[test]
    fn crash_without_finish_keeps_complete_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.gopus");
        let input = tone(2.5);
        write(&path, &input, false);
        let mut r = TrackReader::open(&path).unwrap();
        assert!(!r.is_complete());
        let out = r.read_all().unwrap();
        // Two full chunks made it to disk; the partial third did not.
        let expected = 2 * CHUNK_SAMPLES as usize - r.pre_skip() as usize;
        assert_eq!(out.len(), expected);
        assert!(correlation(&input[1600..expected], &out[1600..]) > 0.95);
    }

    #[test]
    fn truncated_final_chunk_is_tolerated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.gopus");
        let input = tone(3.0);
        write(&path, &input, true);
        let full = fs::read(&path).unwrap();
        let r = TrackReader::open(&path).unwrap();
        let chunks = r.chunks.clone();
        drop(r);

        // Cut in the middle of the last audio chunk (a torn write).
        let (off, len) = chunks[2];
        fs::write(&path, &full[..(off + len as u64 / 2) as usize]).unwrap();
        let mut r = TrackReader::open(&path).unwrap();
        assert!(!r.is_complete());
        let out = r.read_all().unwrap();
        assert_eq!(
            out.len(),
            2 * CHUNK_SAMPLES as usize - r.pre_skip() as usize
        );

        // Cut inside a length prefix.
        fs::write(&path, &full[..(chunks[1].0 - 2) as usize]).unwrap();
        let mut r = TrackReader::open(&path).unwrap();
        assert_eq!(
            r.read_all().unwrap().len(),
            CHUNK_SAMPLES as usize - r.pre_skip() as usize
        );

        // A full-length final record with garbage in it (torn sector).
        let mut garbled = full[..(off + len as u64) as usize].to_vec();
        let last = garbled.len() - 5;
        garbled[last] ^= 0x55;
        fs::write(&path, &garbled).unwrap();
        let mut r = TrackReader::open(&path).unwrap();
        assert_eq!(
            r.read_all().unwrap().len(),
            2 * CHUNK_SAMPLES as usize - r.pre_skip() as usize
        );

        // Header only.
        fs::write(&path, &full[..HEADER_LEN]).unwrap();
        let mut r = TrackReader::open(&path).unwrap();
        assert!(r.read_all().unwrap().is_empty());
        assert_eq!(r.duration_ms().unwrap(), 0);

        // Less than a header is an error, not a panic.
        fs::write(&path, &full[..10]).unwrap();
        assert!(TrackReader::open(&path).is_err());
    }

    #[test]
    fn range_reads_match_the_full_decode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.gopus");
        let input = tone(4.2);
        write(&path, &input, true);
        let mut r = TrackReader::open(&path).unwrap();
        let all = r.read_all().unwrap();

        for (t0, t1) in [
            (0, 500),
            (1500, 2600),
            (2990, 3010),
            (3900, 4200),
            (100, 4200),
        ] {
            let part = r.read_range(t0, t1).unwrap();
            let s0 = t0 as usize * 16;
            let s1 = t1 as usize * 16;
            assert_eq!(part.len(), s1 - s0, "range {t0}..{t1}");
            // Pre-roll means it matches the full decode (nearly) exactly.
            let c = correlation(&all[s0..s1], &part);
            assert!(c > 0.99, "range {t0}..{t1}: correlation {c}");
            // And it lines up with the original audio in time.
            if s1 - s0 > 1000 && s0 >= 1600 {
                let c = correlation(&input[s0..s1], &part);
                assert!(c > 0.9, "range {t0}..{t1} vs input: correlation {c}");
            }
        }
        // Clipped and empty ranges.
        assert_eq!(r.read_range(4000, 9000).unwrap().len(), 200 * 16);
        assert!(r.read_range(5000, 6000).unwrap().is_empty());
        assert!(r.read_range(300, 300).unwrap().is_empty());
        assert!(r.read_range(-500, 0).unwrap().is_empty());
    }

    #[test]
    fn empty_track_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("e.gopus");
        let s = TrackWriter::create(&path).unwrap().finish().unwrap();
        assert_eq!(s.samples, 0);
        let mut r = TrackReader::open(&path).unwrap();
        assert!(r.is_complete());
        assert!(r.read_all().unwrap().is_empty());
    }

    #[test]
    fn paths_stay_inside_the_audio_dir() {
        let dir = Path::new("/data/meetings/audio");
        assert_eq!(track_file_name(12, Track::Mic), "12-mic.gopus");
        assert_eq!(track_file_name(12, Track::System), "12-system.gopus");
        assert_eq!(
            resolve_track_path(dir, "12-mic.gopus").unwrap(),
            dir.join("12-mic.gopus")
        );
        assert!(resolve_track_path(dir, "../x.gopus").is_err());
        assert!(resolve_track_path(dir, "/etc/passwd").is_err());
        assert_eq!(
            audio_dir(Path::new("/data")),
            PathBuf::from("/data/meetings/audio")
        );
    }

    fn meeting(id_hint: i64, ended_at: i64, status: MeetingStatus, dir: &Path) -> Meeting {
        let mic = format!("{id_hint}-mic.gopus");
        let sys = format!("{id_hint}-system.gopus");
        fs::write(dir.join(&mic), b"x").unwrap();
        fs::write(dir.join(&sys), b"x").unwrap();
        Meeting {
            id: 0,
            title: format!("m{id_hint}"),
            calendar_event_id: None,
            calendar_event: None,
            started_at: ended_at - 1000,
            ended_at: Some(ended_at),
            mic_audio_path: Some(mic),
            system_audio_path: Some(sys),
            // Processed meetings went through the whole pipeline.
            processing_stage: (status == MeetingStatus::Processed).then(|| "done".into()),
            description: None,
            title_source: Default::default(),
            has_summary: false,
            status,
            error: None,
        }
    }

    fn never_busy(_: MeetingId) -> bool {
        false
    }

    #[test]
    fn retention_deletes_only_old_idle_audio() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path();
        let now = 100 * DAY_MS;
        let repo = MemRepo::default();
        let old = repo
            .insert_meeting(&meeting(
                1,
                now - 4 * DAY_MS,
                MeetingStatus::Processed,
                audio,
            ))
            .unwrap();
        let fresh = repo
            .insert_meeting(&meeting(
                2,
                now - 2 * DAY_MS,
                MeetingStatus::Processed,
                audio,
            ))
            .unwrap();
        let busy = repo
            .insert_meeting(&meeting(
                3,
                now - 9 * DAY_MS,
                MeetingStatus::Processing,
                audio,
            ))
            .unwrap();
        let failed = repo
            .insert_meeting(&meeting(4, now - 9 * DAY_MS, MeetingStatus::Failed, audio))
            .unwrap();

        // Forever: nothing happens.
        assert_eq!(
            apply_retention(&repo, audio, None, now, &never_busy)
                .unwrap()
                .files_deleted,
            0
        );
        assert!(audio.join("1-mic.gopus").exists());

        let outcome = apply_retention(&repo, audio, Some(3), now, &never_busy).unwrap();
        assert_eq!(outcome.files_deleted, 2);
        assert_eq!(outcome.meetings, vec![old]);
        let get = |id| repo.get_meeting(id).unwrap().unwrap();
        assert_eq!(get(old).mic_audio_path, None);
        assert_eq!(get(old).system_audio_path, None);
        // Only the paths changed (targeted update).
        assert_eq!(get(old).title, "m1");
        assert!(!audio.join("1-mic.gopus").exists());
        assert!(!audio.join("1-system.gopus").exists());
        // Failed (never fully processed) keeps its audio for a retry.
        assert!(get(failed).mic_audio_path.is_some());
        assert!(audio.join("4-mic.gopus").exists());
        assert!(get(fresh).mic_audio_path.is_some());
        assert!(audio.join("2-mic.gopus").exists());
        assert!(get(busy).mic_audio_path.is_some());
        assert!(audio.join("3-mic.gopus").exists());
        // The rows themselves (and anything hanging off them) stay.
        assert_eq!(repo.list_meetings().unwrap().len(), 4);

        // Idempotent.
        assert_eq!(
            apply_retention(&repo, audio, Some(3), now, &never_busy).unwrap(),
            Default::default()
        );
    }

    #[test]
    fn retention_skips_unprocessed_and_busy_meetings() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path();
        let now = 100 * DAY_MS;
        let repo = MemRepo::default();
        // Recorded (never processed), old: its audio is still needed.
        let recorded = repo
            .insert_meeting(&meeting(
                1,
                now - 9 * DAY_MS,
                MeetingStatus::Recorded,
                audio,
            ))
            .unwrap();
        // Processed but mid-way through a re-run (stage not done).
        let mut partial = meeting(2, now - 9 * DAY_MS, MeetingStatus::Processed, audio);
        partial.processing_stage = Some("vad".into());
        let partial = repo.insert_meeting(&partial).unwrap();
        // Fully processed, but queued in the processor for a re-run.
        let queued = repo
            .insert_meeting(&meeting(
                3,
                now - 9 * DAY_MS,
                MeetingStatus::Processed,
                audio,
            ))
            .unwrap();
        let out = apply_retention(&repo, audio, Some(3), now, &|id| id == queued).unwrap();
        assert_eq!(out, RetentionOutcome::default());
        for id in [recorded, partial, queued] {
            assert!(repo
                .get_meeting(id)
                .unwrap()
                .unwrap()
                .mic_audio_path
                .is_some());
        }
        assert!(audio.join("3-mic.gopus").exists());
        // Once the processor is done with it, it goes.
        let out = apply_retention(&repo, audio, Some(3), now, &never_busy).unwrap();
        assert_eq!(out.meetings, vec![queued]);
    }

    #[test]
    fn retention_clear_is_guarded_by_status() {
        // The row started processing after the list was read: the guarded
        // update refuses and the files stay.
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path();
        let now = 100 * DAY_MS;
        let repo = MemRepo::default();
        let id = repo
            .insert_meeting(&meeting(
                1,
                now - 9 * DAY_MS,
                MeetingStatus::Processed,
                audio,
            ))
            .unwrap();
        let flip = |_: MeetingId| {
            repo.rows.lock()[0].status = MeetingStatus::Processing;
            false
        };
        let out = apply_retention(&repo, audio, Some(3), now, &flip).unwrap();
        assert!(out.meetings.is_empty());
        assert!(audio.join("1-mic.gopus").exists());
        assert!(repo
            .get_meeting(id)
            .unwrap()
            .unwrap()
            .mic_audio_path
            .is_some());
    }

    #[cfg(unix)]
    #[test]
    fn track_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("1-mic.gopus");
        fs::write(&path, b"old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let w = TrackWriter::create(&path).unwrap();
        w.finish().unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn retention_removes_old_orphans_only() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path();
        let repo = MemRepo::default();
        fs::write(audio.join("99-mic.gopus"), b"x").unwrap();
        fs::write(audio.join("notes.txt"), b"x").unwrap();
        let now = system_time_ms(std::time::SystemTime::now());
        // Fresh orphan (e.g. a recording being started): kept.
        assert_eq!(
            apply_retention(&repo, audio, Some(3), now, &never_busy)
                .unwrap()
                .files_deleted,
            0
        );
        assert!(audio.join("99-mic.gopus").exists());
        // Seen from 4 days in the future it is old: removed. Other files stay.
        assert_eq!(
            apply_retention(&repo, audio, Some(3), now + 4 * DAY_MS, &never_busy)
                .unwrap()
                .files_deleted,
            1
        );
        assert!(!audio.join("99-mic.gopus").exists());
        assert!(audio.join("notes.txt").exists());
    }
}
