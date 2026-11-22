use anyhow::Result;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::context::InvocationContext;
use crate::files::remove_files;
use crate::network::{self, AcquiredNetwork};

use super::acquire::acquire_secrets_and_network;
use super::pins::{PinCleanup, apply_pins, select_tool_env};
use super::work::{CacheScope, ProviderWork, SecretHydrate, resolve_provider_work};
use super::{merge_env_with_conflicts, show_loader_warnings};

/// The plan of a command Lade starts outside `exec::run`, `lade mcp`'s
/// server: the same pins and provider work as `lade --`.
pub(crate) struct Plan {
    pins: PinCleanup,
    tool_env: HashMap<String, String>,
    work: Option<ProviderWork>,
}

impl Plan {
    pub(crate) fn disclaimers(&self) -> &[String] {
        self.work.as_ref().map_or(&[], |work| &work.disclaimers)
    }

    /// A pin applied, or a secret or tunnel to hydrate.
    pub(crate) fn prepares(&self) -> bool {
        !self.tool_env.is_empty()
            || self
                .work
                .as_ref()
                .is_some_and(ProviderWork::prepares_bindings)
    }

    /// `log` and `matches` of the matched rules, `None` when none matched.
    pub(crate) fn matched(&self) -> Option<(bool, &Value)> {
        self.work.as_ref().map(|work| (work.log, &work.matches))
    }
}

/// Access held for as long as the command runs. Dropping it removes the
/// `.file` outputs, the pinned mise config, and the tunnel lease.
pub(crate) struct Attached {
    pub(crate) env: HashMap<String, String>,
    pub(crate) cached: HashSet<String>,
    tool_env: HashMap<String, String>,
    files: HashMap<PathBuf, HashMap<String, String>>,
    private: HashMap<String, String>,
    _network: AcquiredNetwork,
    _pins: PinCleanup,
}

impl Attached {
    pub(crate) fn scrub_values(&self) -> HashMap<String, String> {
        super::scrub_values(&self.env, &self.files, &self.private)
    }

    /// The bindings plus the pins' `PATH` and env, for a child process.
    pub(crate) fn child_env(&self) -> Result<HashMap<String, String>> {
        let mut env = self.env.clone();
        select_tool_env(&mut env, self.tool_env.clone())?;
        Ok(env)
    }
}

impl Drop for Attached {
    fn drop(&mut self) {
        // § fz8r6d2ka.d@14n8c
        let _ = remove_files(&mut self.files.keys());
    }
}

/// A failed pin exits 1 here, as on `lade --`.
pub(crate) async fn plan(
    ctx: &InvocationContext,
    config: &Config,
    command: &str,
    cwd: &Path,
    saved_user: &Option<String>,
) -> Result<Plan> {
    // § g6sqmp8tb.c@1t38p
    let pins = apply_pins(config, command, cwd, saved_user, ctx.audience, None).await?;
    let pin_cleanup = PinCleanup(pins.cleanup.clone());
    let work =
        resolve_provider_work(config, command, ctx.audience, None, false, saved_user).await?;
    // § end g6sqmp8tb.c
    Ok(Plan {
        pins: pin_cleanup,
        tool_env: pins.env,
        work,
    })
}

/// A failed secret or tunnel shows the shared box and exits 1 here, as on
/// `lade --`.
pub(crate) async fn acquire(
    plan: Plan,
    ctx: &InvocationContext,
    config: &Config,
    command: &str,
    cwd: &Path,
    saved_user: &Option<String>,
) -> Result<Attached> {
    let Plan {
        pins,
        tool_env,
        work,
    } = plan;
    let Some(work) = work.filter(ProviderWork::needs_providers) else {
        return Ok(Attached {
            env: HashMap::new(),
            cached: HashSet::new(),
            tool_env,
            files: HashMap::new(),
            private: HashMap::new(),
            _network: AcquiredNetwork::empty(),
            _pins: pins,
        });
    };
    let patterned = config.collect_for_with_pattern(command, ctx.audience)?;
    let tunnel_cwd = cwd.to_path_buf();
    // § g6sqmp8tb.c@ghs0s
    let acquired = acquire_secrets_and_network(
        ctx,
        SecretHydrate {
            secrets: &work.secrets,
            progress: &work.progress,
            ticket_unlink: None,
            cache: Some(CacheScope {
                cwd,
                walk: config.walk_hash(),
                saved_user,
                patterned: &patterned,
            }),
        },
        work.network_bindings,
        None,
        None,
        move |bindings, sink, cancel| {
            network::start_attached_network_session(bindings, sink, &tunnel_cwd, cancel)
        },
    )
    .await;
    // § end g6sqmp8tb.c
    let Some(((mut env, files, _sources, _maskable, warnings, cached, private), network)) =
        acquired
    else {
        unreachable!("only a human may run a command anyway, and an MCP client is an agent")
    };
    let attached = |env, network| Attached {
        env,
        cached,
        tool_env,
        files,
        private,
        _network: network,
        _pins: pins,
    };
    if show_loader_warnings(ctx, &warnings).await {
        drop(attached(env, network));
        std::process::exit(crate::exit_codes::INTERRUPTED);
    }
    // Each MCP door watches the stop signals from here on, so one that
    // arrives before the bridge or the server starts still drops the access.
    crate::child_signals::ChildWatch::new();
    // § t7kcndtra.a@6ny5m
    if let Err(error) = merge_env_with_conflicts(&mut env, network.env.clone()) {
        drop(attached(HashMap::new(), network));
        return Err(error);
    }
    Ok(attached(env, network))
}
