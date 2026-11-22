//! Links the laptop hub holds for `lade connect`: one `lade __remote` process
//! per transport, restarted after a drop. The hub only runs the process and
//! reads its phase. Values travel in `__remote`, never through the hub.
//!
//! Each `lade connect` over a transport adds the walk it resolved to the
//! link running there. The hub writes every walk on the lifeline of each
//! `__remote` it spawns, as `w <walk hex> <json>` lines, and reads its
//! `walk <hex>` acks.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::network::{Stop, configure_child_process};

use super::wire::{LinkPhase, LinkRow};

/// `__remote` exit codes the hub reads. Any other code is a drop to retry.
pub(crate) const LINK_DETACHED: i32 = 0;
/// A failure only a human clears: retrying would only fail again.
pub(crate) const LINK_GIVE_UP: i32 = 2;
pub(crate) const LINK_READY: &str = "ready";
/// `__remote` took up a walk: `walk <hex>` on its stdout.
pub(crate) const LINK_WALK: &str = "walk";
/// A walk on the lifeline: `w <hex> <json>`.
pub(crate) const LIFELINE_WALK: &str = "w";
pub(crate) const SPEC_ENV: &str = "LADE_LINK_SPEC";
pub(crate) const ATTEMPT_ENV: &str = "LADE_LINK_ATTEMPT";
/// Detach, written on the lifeline as a line. Closing it without this line
/// means the hub is gone.
pub(crate) const LIFELINE_DETACH: &str = "d";

const FIRST_DELAY: Duration = Duration::from_secs(1);
const MAX_DELAY: Duration = Duration::from_secs(30);
const ERROR_TAIL: usize = 4096;

struct State {
    phase: LinkPhase,
    error: Option<String>,
    /// Acked by the running `__remote`.
    walks: Vec<String>,
}

type Walks = Arc<Mutex<BTreeMap<String, String>>>;

struct Link {
    host: String,
    state: Arc<Mutex<State>>,
    stop: Stop,
    lifeline: Arc<Mutex<Option<ChildStdin>>>,
    walks: Walks,
}

fn write_walk(lifeline: &mut ChildStdin, walk: &str, resolved: &str) -> std::io::Result<()> {
    writeln!(lifeline, "{LIFELINE_WALK} {walk} {resolved}")?;
    lifeline.flush()
}

impl Link {
    fn row(&self, key: &str) -> LinkRow {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        LinkRow {
            key: key.to_string(),
            host: self.host.clone(),
            phase: state.phase,
            error: state.error.clone(),
            walks: state.walks.clone(),
        }
    }

    fn is_down(&self) -> bool {
        matches!(
            self.state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .phase,
            LinkPhase::Failed | LinkPhase::Stopped
        )
    }

    /// The next `__remote` spawned takes the walk up from the map. One that
    /// misses this line on a dead pipe is replaced, and gets it then.
    fn add(&self, walk: String, resolved: String) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .walks
            .retain(|acked| *acked != walk);
        let mut walks = self.walks.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(lifeline) = self
            .lifeline
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
        {
            let _ = write_walk(lifeline, &walk, &resolved);
        }
        walks.insert(walk, resolved);
    }

    fn detach(&self) {
        self.stop.stop();
        if let Some(mut lifeline) = self
            .lifeline
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = writeln!(lifeline, "{LIFELINE_DETACH}");
        }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.stop();
        self.lifeline
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }
}

#[derive(Default)]
pub(crate) struct Links {
    map: HashMap<String, Link>,
}

impl Links {
    /// Adds the walk to the link running on the transport, or starts one
    /// with it and every walk a link there that went down held.
    pub(super) fn start(
        &mut self,
        key: String,
        host: String,
        spec: String,
        env: Vec<(Vec<u8>, Vec<u8>)>,
        walk: Option<(String, String)>,
    ) -> LinkRow {
        if let Some(link) = self.map.get(&key)
            && !link.is_down()
        {
            if let Some((walk, resolved)) = walk {
                link.add(walk, resolved);
            }
            return link.row(&key);
        }
        let mut held = self
            .map
            .remove(&key)
            .map(|link| {
                std::mem::take(&mut *link.walks.lock().unwrap_or_else(PoisonError::into_inner))
            })
            .unwrap_or_default();
        held.extend(walk);
        let state = Arc::new(Mutex::new(State {
            phase: LinkPhase::Starting,
            error: None,
            walks: Vec::new(),
        }));
        let stop = Stop::default();
        let lifeline = Arc::new(Mutex::new(None));
        let walks = Arc::new(Mutex::new(held));
        let link = Link {
            host,
            state: Arc::clone(&state),
            stop: stop.clone(),
            lifeline: Arc::clone(&lifeline),
            walks: Arc::clone(&walks),
        };
        std::thread::spawn(move || supervise(spec, env, state, stop, lifeline, walks));
        let row = link.row(&key);
        self.map.insert(key, link);
        row
    }

    pub(super) fn poll(&self, key: &str) -> Vec<LinkRow> {
        self.map
            .get(key)
            .map(|link| link.row(key))
            .into_iter()
            .collect()
    }

    /// True when a link ran on `key`.
    pub(super) fn stop(&mut self, key: &str) -> bool {
        match self.map.remove(key) {
            Some(link) => {
                link.detach();
                true
            }
            None => false,
        }
    }

    pub(super) fn list(&self) -> Vec<LinkRow> {
        let mut rows = self
            .map
            .iter()
            .map(|(key, link)| link.row(key))
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.key.cmp(&b.key));
        rows
    }

    /// A stopped link is gone. A failed one stays listed for `lade connect`
    /// to read, but keeps no hub up.
    pub(super) fn prune(&mut self) {
        self.map.retain(|_, link| {
            link.state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .phase
                != LinkPhase::Stopped
        });
    }

    pub(super) fn is_empty(&self) -> bool {
        // § cbz2he75h.e@78e39
        self.map.values().all(Link::is_down)
    }
}

fn set(state: &Mutex<State>, phase: LinkPhase, error: Option<String>) {
    let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
    state.phase = phase;
    state.error = error;
}

fn supervise(
    spec: String,
    env: Vec<(Vec<u8>, Vec<u8>)>,
    state: Arc<Mutex<State>>,
    stop: Stop,
    lifeline: Arc<Mutex<Option<ChildStdin>>>,
    walks: Walks,
) {
    // § dzex6kwmq@3g2fx
    use std::os::unix::ffi::OsStrExt;
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return set(&state, LinkPhase::Failed, Some(e.to_string())),
    };
    let mut attempt = 0u32;
    let mut ever_ready = false;
    let mut delay = FIRST_DELAY;
    loop {
        if stop.is_stopped() {
            return set(&state, LinkPhase::Stopped, None);
        }
        let mut command = Command::new(&exe);
        command
            .arg("__remote")
            .env_clear()
            .envs(env.iter().map(|(k, v)| {
                (
                    std::ffi::OsStr::from_bytes(k),
                    std::ffi::OsStr::from_bytes(v),
                )
            }))
            .env(SPEC_ENV, &spec)
            .env(ATTEMPT_ENV, attempt.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_child_process(&mut command);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => return set(&state, LinkPhase::Failed, Some(e.to_string())),
        };
        state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .walks
            .clear();
        {
            // Held across the writes so a walk `start` adds meanwhile is
            // written once, after these.
            let walks = walks.lock().unwrap_or_else(PoisonError::into_inner);
            let mut stdin = child.stdin.take();
            if let Some(pipe) = stdin.as_mut() {
                for (walk, resolved) in walks.iter() {
                    let _ = write_walk(pipe, walk, resolved);
                }
            }
            *lifeline.lock().unwrap_or_else(PoisonError::into_inner) = stdin;
        }
        let errors = Arc::new(Mutex::new(Vec::<u8>::new()));
        let stderr = child.stderr.take().map(|mut pipe| {
            let errors = Arc::clone(&errors);
            std::thread::spawn(move || {
                let mut buf = [0u8; 1024];
                while let Ok(n) = pipe.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let mut errors = errors.lock().unwrap_or_else(PoisonError::into_inner);
                    errors.extend_from_slice(&buf[..n]);
                    let over = errors.len().saturating_sub(ERROR_TAIL);
                    errors.drain(..over);
                }
            })
        });
        let mut became_ready = false;
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Some(walk) = line
                    .trim()
                    .strip_prefix(LINK_WALK)
                    .and_then(|rest| rest.strip_prefix(' '))
                {
                    state
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .walks
                        .push(walk.to_string());
                } else if line.trim() == LINK_READY {
                    became_ready = true;
                    ever_ready = true;
                    delay = FIRST_DELAY;
                    set(&state, LinkPhase::Ready, None);
                }
            }
        }
        let status = child.wait();
        if let Some(handle) = stderr {
            let _ = handle.join();
        }
        let error = last_line(&errors.lock().unwrap_or_else(PoisonError::into_inner));
        if stop.is_stopped() {
            return set(&state, LinkPhase::Stopped, None);
        }
        let code = status.ok().and_then(|status| status.code());
        match code {
            Some(LINK_DETACHED) => return set(&state, LinkPhase::Stopped, None),
            Some(LINK_GIVE_UP) => return set(&state, LinkPhase::Failed, error),
            _ if !ever_ready => return set(&state, LinkPhase::Failed, error),
            _ => set(&state, LinkPhase::Retrying, error),
        }
        if !became_ready {
            delay = (delay * 2).min(MAX_DELAY);
        }
        if stop.sleep(delay) {
            return set(&state, LinkPhase::Stopped, None);
        }
        attempt += 1;
    }
}

fn last_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(str::to_string)
}
