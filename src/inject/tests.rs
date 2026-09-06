use super::acquire::race_provider_tasks;
use super::*;
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
