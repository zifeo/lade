use std::collections::{HashMap, HashSet};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use super::peer;
use super::wire::{self, ERR_DENIED, Rep, Req};

const SECRET_CAP: usize = 256;
const TICKET_CAP: usize = 128;
const TICKET_TTL: Duration = Duration::from_secs(3600);

#[derive(Clone, PartialEq, Eq, Hash)]
struct SecretKey {
    cwd: String,
    path: String,
    rule: String,
    when: String,
    walk: [u8; 32],
    user: String,
}

struct SecretRow {
    inserted: Instant,
    ttl_ms: u32,
    bindings: Vec<(String, Vec<u8>)>,
}

struct TicketRow {
    inserted: Instant,
    ticket: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct Tables {
    secrets: HashMap<SecretKey, SecretRow>,
    tickets: HashMap<[u8; 4], TicketRow>,
}

impl Tables {
    fn prune(&mut self) {
        self.secrets
            .retain(|_, row| row.inserted.elapsed() < Duration::from_millis(u64::from(row.ttl_ms)));
        self.tickets
            .retain(|_, row| row.inserted.elapsed() < TICKET_TTL);
    }

    fn evict_secrets_if_full(&mut self) {
        while self.secrets.len() > SECRET_CAP {
            let oldest = self
                .secrets
                .iter()
                .min_by_key(|(_, row)| row.inserted)
                .map(|(key, _)| key.clone());
            if let Some(key) = oldest {
                self.secrets.remove(&key);
            } else {
                break;
            }
        }
    }

    fn evict_tickets_if_full(&mut self) {
        while self.tickets.len() > TICKET_CAP {
            let oldest = self
                .tickets
                .iter()
                .min_by_key(|(_, row)| row.inserted)
                .map(|(key, _)| *key);
            if let Some(key) = oldest {
                self.tickets.remove(&key);
            } else {
                break;
            }
        }
    }

    fn handle(&mut self, req: Req) -> Rep {
        self.prune();
        match req {
            Req::Get {
                cwd,
                path,
                rule,
                when,
                walk,
                user,
            } => {
                let key = SecretKey {
                    cwd,
                    path,
                    rule,
                    when,
                    walk,
                    user,
                };
                match self.secrets.get(&key) {
                    Some(row) => Rep::Hit {
                        bindings: row.bindings.clone(),
                    },
                    None => Rep::Miss,
                }
            }
            Req::Put {
                cwd,
                path,
                rule,
                when,
                walk,
                user,
                ttl_ms,
                bindings,
            } => {
                let key = SecretKey {
                    cwd,
                    path,
                    rule,
                    when,
                    walk,
                    user,
                };
                self.secrets.insert(
                    key,
                    SecretRow {
                        inserted: Instant::now(),
                        ttl_ms,
                        bindings,
                    },
                );
                self.evict_secrets_if_full();
                Rep::Ok
            }
            Req::GetT { t } => match self.tickets.get(&t) {
                Some(row) => Rep::Ticket {
                    ticket: row.ticket.clone(),
                },
                None => Rep::Miss,
            },
            Req::PutT { t, ticket } => {
                self.tickets.insert(
                    t,
                    TicketRow {
                        inserted: Instant::now(),
                        ticket,
                    },
                );
                self.evict_tickets_if_full();
                Rep::Ok
            }
            Req::UnlinkT { t } => {
                self.tickets.remove(&t);
                Rep::Ok
            }
            Req::Ping => Rep::Stat {
                pid: std::process::id(),
                secrets: self.secrets.len() as u32,
                tickets: self.tickets.len() as u32,
            },
            Req::Quit => Rep::Ok,
            Req::List => {
                let mut rows = Vec::new();
                for (key, row) in &self.secrets {
                    let left = ttl_left_ms(row);
                    for (name, _) in &row.bindings {
                        rows.push(wire::CacheRow {
                            name: name.clone(),
                            path: key.path.clone(),
                            rule: key.rule.clone(),
                            when: key.when.clone(),
                            user: key.user.clone(),
                            ttl_left_ms: left,
                        });
                    }
                }
                rows.sort_by(|a, b| a.name.cmp(&b.name).then(a.rule.cmp(&b.rule)));
                Rep::Listing {
                    pid: std::process::id(),
                    tickets: self.tickets.len() as u32,
                    rows,
                }
            }
            Req::Forget { names } => {
                let drop: HashSet<String> = names.into_iter().collect();
                for row in self.secrets.values_mut() {
                    row.bindings.retain(|(name, _)| !drop.contains(name));
                }
                self.secrets.retain(|_, row| !row.bindings.is_empty());
                Rep::Ok
            }
        }
    }
}

fn ttl_left_ms(row: &SecretRow) -> u32 {
    let elapsed = u32::try_from(row.inserted.elapsed().as_millis()).unwrap_or(u32::MAX);
    row.ttl_ms.saturating_sub(elapsed)
}

enum AfterConn {
    Continue,
    Quit,
}

pub async fn serve() -> anyhow::Result<()> {
    let snap = peer::snapshot_self().ok_or_else(|| anyhow::anyhow!("hub image inode"))?;
    let want = snap.inode();
    let path = socket_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(_) => return Ok(()),
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    let mut tables = Tables::default();
    loop {
        let (stream, _) = listener.accept().await?;
        if matches!(
            handle_conn(&mut tables, stream, want).await,
            AfterConn::Quit
        ) {
            break;
        }
    }
    drop(listener);
    let _ = std::fs::remove_file(&path);
    Ok(())
}

async fn handle_conn(tables: &mut Tables, mut stream: UnixStream, want: (u64, u64)) -> AfterConn {
    if !peer::peer_allowed(&stream, want) {
        send_denied(&mut stream).await;
        return AfterConn::Continue;
    }
    match read_req(&mut stream).await {
        Some(wire::InReq::Body(req)) => {
            let quit = matches!(req, Req::Quit);
            let Some(frame) = wire::encode_rep(&tables.handle(req)) else {
                return AfterConn::Continue;
            };
            let _ = stream.write_all(&frame).await;
            if quit {
                AfterConn::Quit
            } else {
                AfterConn::Continue
            }
        }
        Some(wire::InReq::WrongVer) => {
            send_denied(&mut stream).await;
            AfterConn::Continue
        }
        None => AfterConn::Continue,
    }
}

async fn send_denied(stream: &mut UnixStream) {
    if let Some(frame) = wire::encode_rep(&Rep::Err(ERR_DENIED)) {
        let _ = stream.write_all(&frame).await;
    }
}

async fn read_req(stream: &mut UnixStream) -> Option<wire::InReq> {
    let mut header = [0u8; 8];
    stream.read_exact(&mut header).await.ok()?;
    let n = wire::header_len(&header)?;
    let mut body = vec![0u8; n as usize];
    stream.read_exact(&mut body).await.ok()?;
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend_from_slice(&header);
    frame.extend_from_slice(&body);
    wire::decode_in_req(&frame)
}

pub fn socket_path() -> PathBuf {
    crate::cache::root().join(format!("hub-{}.sock", peer::instance_token()))
}

pub fn lock_path() -> PathBuf {
    crate::cache::root().join(format!("hub-{}.lock", peer::instance_token()))
}

#[cfg(test)]
pub(crate) fn handle_for_test(tables: &mut Tables, req: Req) -> Rep {
    tables.handle(req)
}

#[cfg(test)]
pub(crate) fn empty_tables() -> Tables {
    Tables::default()
}
