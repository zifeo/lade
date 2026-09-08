use super::acquire::race_provider_tasks;
use super::*;
use std::collections::HashMap;

#[test]
fn select_tool_env_rejects_pin_secret_collision() {
    let mut env = HashMap::from([("RUSTUP_TOOLCHAIN".to_string(), "stable".to_string())]);
    let tool = HashMap::from([("RUSTUP_TOOLCHAIN".to_string(), "1.96.0".to_string())]);
    let err = select_tool_env(&mut env, tool).unwrap_err();
    assert!(err.to_string().contains("conflicting env"), "{err}");
}

#[test]
fn select_tool_env_keeps_matching_secret() {
    let mut env = HashMap::from([("RUSTUP_TOOLCHAIN".to_string(), "1.96.0".to_string())]);
    let tool = HashMap::from([("RUSTUP_TOOLCHAIN".to_string(), "1.96.0".to_string())]);
    select_tool_env(&mut env, tool).unwrap();
    assert_eq!(env.get("RUSTUP_TOOLCHAIN").unwrap(), "1.96.0");
}

#[test]
fn select_tool_env_path_always_wins() {
    let mut env = HashMap::from([("PATH".to_string(), "/usr/bin".to_string())]);
    let tool = HashMap::from([("PATH".to_string(), "/pin/bin:/usr/bin".to_string())]);
    select_tool_env(&mut env, tool).unwrap();
    assert_eq!(env.get("PATH").unwrap(), "/pin/bin:/usr/bin");
}
use crate::provider_progress::{start_provider_progress, stop_provider_progress};
use std::time::{Duration, Instant};

#[tokio::test]
async fn provider_race_fails_fast_and_does_not_deadlock_progress_renderer() {
    let mut provider_progress = Some(start_provider_progress(false));
    let sink = provider_progress.as_ref().unwrap().sink();
    let started = Instant::now();

    let acquisition = {
        let secret_task = async move {
            let _sink = sink;
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok::<_, anyhow::Error>((
                HashMap::new(),
                HashMap::new(),
                HashMap::new(),
                FxHashSet::default(),
                Vec::new(),
            ))
        };
        let network_task =
            async { Err::<(), _>(anyhow::anyhow!("network provider error: fast failure")) };

        race_provider_tasks(secret_task, network_task).await
    };

    assert!(matches!(acquisition, Acquisition::Failed(_)));
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "network failure should not wait for the slow secret provider"
    );

    tokio::time::timeout(
        Duration::from_secs(1),
        tokio::task::spawn_blocking(move || stop_provider_progress(&mut provider_progress)),
    )
    .await
    .expect("progress renderer should not block after fail-fast")
    .expect("progress renderer join task should complete");
}
