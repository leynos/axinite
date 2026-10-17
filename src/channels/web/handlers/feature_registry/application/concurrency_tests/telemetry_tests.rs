//! Command telemetry under controlled admission, with no global tracing state.

use super::*;

#[tokio::test]
async fn queued_commands_observe_admission_and_total_latency_under_safe_correlation() {
    use super::super::telemetry::test_support::RecordingObserver;
    use tracing::instrument::WithSubscriber;

    let observer = RecordingObserver::default();
    let deployment = DeploymentId::parse("production").expect("valid deployment");
    let flag = FlagName::parse("panel_logs").expect("valid flag");
    let registry = RwLock::new(FeatureFlagRegistry::new());
    let store = ControlledStore {
        pause_production_disable: true,
        ..Default::default()
    };
    let mut first = Box::pin(
        set_override(
            &registry,
            &store,
            override_command(&deployment, &flag, false),
        )
        .with_subscriber(observer.clone().subscriber()),
    );
    assert!(futures::poll!(first.as_mut()).is_pending());
    let mut second = Box::pin(
        set_override(
            &registry,
            &store,
            override_command(&deployment, &flag, true),
        )
        .with_subscriber(observer.clone().subscriber()),
    );
    assert!(futures::poll!(second.as_mut()).is_pending());
    assert_eq!(
        observer
            .observations()
            .iter()
            .filter(|record| record.0["message"] == "feature_flags.admission_wait")
            .count(),
        1
    );
    assert!(
        !observer
            .observations()
            .iter()
            .any(|record| record.0["message"] == "feature_flags.command_latency")
    );
    store.release_first_write.notify_one();
    first.await.expect("first publishes");
    second.await.expect("queued command publishes");
    let observations = observer.observations();
    let admissions = observations
        .iter()
        .filter(|record| record.0["message"] == "feature_flags.admission_wait")
        .collect::<Vec<_>>();
    let commands = observations
        .iter()
        .filter(|record| record.0["message"] == "feature_flags.command_latency")
        .collect::<Vec<_>>();
    assert_eq!(admissions.len(), 2);
    assert_eq!(commands.len(), 2);
    assert_ne!(commands[0].0["operation_id"], commands[1].0["operation_id"]);
    for record in commands {
        let correlation = &record.0["operation_id"];
        assert!(uuid::Uuid::parse_str(correlation).is_ok());
        assert_eq!(record.0["category"], "success");
        assert!(
            admissions
                .iter()
                .any(|admission| admission.0["operation_id"] == *correlation)
        );
        assert!(
            record.0["elapsed_seconds"]
                .parse::<f64>()
                .expect("numeric latency")
                >= 0.0
        );
    }
    assert!(observations.iter().all(|record| {
        !record
            .0
            .values()
            .any(|value| value.contains("production") || value.contains("panel_logs"))
    }));
}

#[rstest::rstest]
#[case::load(FlagStoreError::Load, "load")]
#[case::save(FlagStoreError::Save, "save")]
#[case::invalid_snapshot(FlagStoreError::InvalidSnapshot, "invalid_snapshot")]
#[tokio::test]
async fn command_failures_have_bounded_categories_and_ephemeral_correlation(
    #[case] failure: FlagStoreError,
    #[case] category: &str,
) {
    use super::super::telemetry::test_support::RecordingObserver;
    use tracing::instrument::WithSubscriber;

    let observer = RecordingObserver::default();
    let deployment = DeploymentId::parse("production").expect("valid deployment");
    let flag = FlagName::parse("panel_logs").expect("valid flag");
    let registry = RwLock::new(FeatureFlagRegistry::new());
    let store = ControlledStore {
        fail_next_load: AtomicBool::new(failure == FlagStoreError::Load),
        fail_next_save: AtomicBool::new(failure == FlagStoreError::Save),
        invalid_snapshot: failure == FlagStoreError::InvalidSnapshot,
        ..Default::default()
    };
    assert_eq!(
        set_override(
            &registry,
            &store,
            override_command(&deployment, &flag, true)
        )
        .with_subscriber(observer.clone().subscriber())
        .await,
        Err(failure)
    );
    let observations = observer.observations();
    let failures = observations
        .iter()
        .filter(|record| record.0["message"] == "feature_flags.failure")
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0["category"], category);
    let correlation = &failures[0].0["operation_id"];
    assert!(uuid::Uuid::parse_str(correlation).is_ok());
    assert!(observations.iter().any(|record| record.0["message"]
        == "feature_flags.command_latency"
        && record.0["category"] == category
        && record.0["operation_id"] == *correlation));
    assert!(observations.iter().all(|record| {
        !record
            .0
            .values()
            .any(|value| value.contains("production") || value.contains("panel_logs"))
    }));
}
