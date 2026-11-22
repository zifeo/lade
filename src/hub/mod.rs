mod apply;
mod cap;
mod client;
mod guard;
mod links;
mod peer;
mod release;
mod server;
mod tables;
mod transport;
mod tunnels;
mod wire;

pub(crate) use apply::{
    Cache, CacheHit, EvalCache, RemoteBody, remote_body_get, remote_body_put, session_ttl,
};
pub(crate) use client::{
    CacheListing, HubInfo, HubState, HubStop, forget_names, link_list, link_poll, link_start,
    link_stop, listing, notice_slot, probe, put_t, put_t_notice, scope, set_window, stop,
    tunnel_open, tunnel_poll, tunnel_release, unlink_t, unset_window, version_ledger, window,
};
pub(crate) use guard::{MemoryGuard, memory_guard};
pub(crate) use links::{
    ATTEMPT_ENV, LIFELINE_DETACH, LIFELINE_WALK, LINK_DETACHED, LINK_GIVE_UP, LINK_READY,
    LINK_WALK, SPEC_ENV,
};
pub(crate) use peer::NO_IMAGE;
pub(crate) use peer::{current_uid, peer_uid};

/// False when this image cannot read its own file: it then has no hub.
pub(crate) fn has_image() -> bool {
    peer::instance_token().is_some()
}
pub(crate) use server::serve;
pub(crate) use tables::path_related;
pub(crate) use wire::{
    CacheRow, Lease, LinkPhase, LinkRow, TunnelAsk, TunnelLaunch, TunnelPhase, TunnelRep,
    TunnelRow, VersionLedger,
};

#[cfg(test)]
mod tests;
