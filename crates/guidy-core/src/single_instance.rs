//! Single-instance handling (replacement for tauri-plugin-single-instance).
//!
//! The first process listens on a local socket. A second launch connects,
//! forwards its argv as one JSON line, waits for the `ok` acknowledgement and
//! exits; the first process acts on the args (`--toggle-transcription`,
//! `--toggle-post-process`, `--cancel`, or "show the window").

use interprocess::local_socket::{
    prelude::*, GenericFilePath, GenericNamespaced, ListenerOptions, Name, Stream,
};
use std::io::{self, BufRead, BufReader, Write};

const ACK: &str = "ok";

fn socket_name(id: &str) -> io::Result<Name<'static>> {
    let name = format!("{id}.sock");
    if GenericNamespaced::is_supported() {
        name.to_ns_name::<GenericNamespaced>()
    } else {
        std::env::temp_dir()
            .join(name)
            .to_fs_name::<GenericFilePath>()
    }
}

/// Outcome of trying to become the primary instance.
pub enum Instance {
    /// We are the primary instance; keep the guard alive for the process
    /// lifetime (dropping it stops listening).
    Primary(PrimaryGuard),
    /// Another instance is running and received our args.
    Forwarded,
}

pub struct PrimaryGuard {
    _thread: std::thread::JoinHandle<()>,
}

/// Become the primary instance for `id`, or forward `args` to the running one.
/// `on_args` runs on a background thread for every forwarded launch.
pub fn acquire<F>(id: &str, args: &[String], on_args: F) -> io::Result<Instance>
where
    F: Fn(Vec<String>) + Send + 'static,
{
    if forward(id, args).is_ok() {
        return Ok(Instance::Forwarded);
    }

    // No live primary. `try_overwrite` replaces a stale socket file left by a
    // crashed instance (filesystem-name platforms).
    let listener = ListenerOptions::new()
        .name(socket_name(id)?)
        .try_overwrite(true)
        .create_sync()?;

    let thread = std::thread::Builder::new()
        .name("single-instance".into())
        .spawn(move || {
            for conn in listener.incoming() {
                let Ok(conn) = conn else { continue };
                let mut reader = BufReader::new(conn);
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let forwarded: Vec<String> = serde_json::from_str(line.trim()).unwrap_or_default();
                let _ = reader.get_mut().write_all(format!("{ACK}\n").as_bytes());
                drop(reader);
                on_args(forwarded);
            }
        })?;

    Ok(Instance::Primary(PrimaryGuard { _thread: thread }))
}

/// Send `args` to a running primary; `Err` when there is none.
pub fn forward(id: &str, args: &[String]) -> io::Result<()> {
    let stream = Stream::connect(socket_name(id)?)?;
    let mut conn = BufReader::new(stream);
    let mut payload = serde_json::to_string(args).map_err(io::Error::other)?;
    payload.push('\n');
    conn.get_mut().write_all(payload.as_bytes())?;
    let mut reply = String::new();
    conn.read_line(&mut reply)?;
    if reply.trim() == ACK {
        Ok(())
    } else {
        Err(io::Error::other("primary instance did not acknowledge"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn unique_id() -> String {
        format!(
            "dev.guidy.test.{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

    #[test]
    fn second_launch_forwards_args_to_the_primary() {
        let id = unique_id();
        let (tx, rx) = mpsc::channel();
        let first = acquire(&id, &[], move |args| tx.send(args).unwrap()).unwrap();
        assert!(matches!(first, Instance::Primary(_)));

        let args = vec!["guidy".to_string(), "--toggle-transcription".to_string()];
        let second = acquire(&id, &args, |_| panic!("second instance must not listen")).unwrap();
        assert!(matches!(second, Instance::Forwarded));

        let got = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(got, args);
        drop(first);
    }

    #[test]
    fn forward_fails_without_a_primary() {
        assert!(forward(&unique_id(), &["x".into()]).is_err());
    }
}
