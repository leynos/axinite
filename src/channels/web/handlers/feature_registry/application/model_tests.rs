//! Generated bounded traces compare real application operations with durable/cache state.
//!
//! The generator varies three deployments, three flags, load/save failures,
//! cached and uncached queries, and delayed persistence responses in either
//! release order. Controlled polls test the named schedule, not arbitrary Tokio
//! scheduling; cancellation after durable commit remains outside this model.

use super::model_support::{ModelStore, Reference, values};
use super::*;
use crate::feature_flags::FlagName;
use proptest::prelude::*;
use proptest::test_runner::TestCaseResult;
use std::sync::atomic::Ordering;
use std::task::Poll;

// kind, deployment, flag, value, failure, second deployment/flag/value, early release.
type Step = (u8, u8, u8, bool, u8, u8, u8, bool, bool);
const DEPLOYMENTS: [&str; 3] = ["production", "staging", "preview"];
const FLAGS: [&str; 3] = ["route_memory", "panel_logs", "route_jobs"];

fn command(deployment: u8, flag: u8, enabled: bool) -> Result<FlagOverrideCommand, TestCaseError> {
    Ok(FlagOverrideCommand {
        deployment: DeploymentId::parse(DEPLOYMENTS[usize::from(deployment)])
            .map_err(|error| TestCaseError::fail(error.to_string()))?,
        flag: FlagName::parse(FLAGS[usize::from(flag)])
            .map_err(|error| TestCaseError::fail(error.to_string()))?,
        enabled,
    })
}

async fn check_state(
    registry: &RwLock<FeatureFlagRegistry>,
    store: &ModelStore,
    model: &Reference,
) -> TestCaseResult {
    prop_assert_eq!(&*store.durable.lock().await, &model.durable);
    let cache = registry.read().await;
    prop_assert_eq!(cache.resources(), model.resources());
    for deployment in DEPLOYMENTS {
        let id = DeploymentId::parse(deployment)
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let actual = cache.snapshot(&id).as_ref().map(values);
        prop_assert_eq!(actual.as_ref(), model.cache.get(deployment));
    }
    Ok(())
}

async fn run_query(
    registry: &RwLock<FeatureFlagRegistry>,
    store: &ModelStore,
    model: &Reference,
    command: &FlagOverrideCommand,
) -> TestCaseResult {
    let (before, cached) = {
        let cache = registry.read().await;
        (cache.resources(), cache.snapshot(&command.deployment))
    };
    let failure = store.failure.load(Ordering::Relaxed);
    let expected = model.queried(command.deployment.as_str(), failure);
    let actual = query(cached, Some(store), &command.deployment)
        .await
        .map(|snapshot| values(&snapshot));
    prop_assert_eq!(actual, expected);
    prop_assert_eq!(registry.read().await.resources(), before);
    Ok(())
}

struct TraceContext<'a> {
    registry: &'a RwLock<FeatureFlagRegistry>,
    store: &'a ModelStore,
    model: &'a mut Reference,
}

async fn run_pair(
    context: TraceContext<'_>,
    commands: (&FlagOverrideCommand, &FlagOverrideCommand),
    release_early: bool,
    failure: u8,
) -> TestCaseResult {
    let TraceContext {
        registry,
        store,
        model,
    } = context;
    let (first_command, second_command) = commands;
    store.failure.store(0, Ordering::Relaxed);
    store.pause_next_save.store(true, Ordering::Relaxed);
    let mut first = Box::pin(set_override(registry, store, first_command.clone()));
    prop_assert!(futures::poll!(first.as_mut()).is_pending());
    model
        .begin_write(first_command.deployment.as_str(), 0)
        .map_err(|error| TestCaseError::fail(error.to_string()))?;
    model.persist(first_command);
    check_state(registry, store, model).await?;
    if release_early {
        store.release_save.notify_one();
        prop_assert_eq!(first.await, Ok(()));
        model.publish(first_command);
        store.failure.store(failure, Ordering::Relaxed);
        let expected = model.write(second_command, failure);
        prop_assert_eq!(
            set_override(registry, store, second_command.clone()).await,
            expected
        );
        return check_state(registry, store, model).await;
    }
    store.failure.store(failure, Ordering::Relaxed);
    complete_delayed_pair(
        TraceContext {
            registry,
            store,
            model,
        },
        first,
        (first_command, second_command),
        failure,
    )
    .await
}

async fn complete_delayed_pair<F>(
    context: TraceContext<'_>,
    first: F,
    commands: (&FlagOverrideCommand, &FlagOverrideCommand),
    failure: u8,
) -> TestCaseResult
where
    F: Future<Output = Result<(), FlagStoreError>>,
{
    let TraceContext {
        registry,
        store,
        model,
    } = context;
    let (first_command, second_command) = commands;
    let mut second = Box::pin(set_override(registry, store, second_command.clone()));
    let same_deployment = first_command.deployment == second_command.deployment;
    let completion = futures::poll!(second.as_mut());
    if same_deployment {
        prop_assert_eq!(completion, Poll::Pending);
    } else {
        prop_assert_eq!(
            completion,
            Poll::Ready(model.write(second_command, failure))
        );
    }
    check_state(registry, store, model).await?;
    store.release_save.notify_one();
    prop_assert_eq!(first.await, Ok(()));
    model.publish(first_command);
    if same_deployment {
        let expected = model.write(second_command, failure);
        prop_assert_eq!(second.await, expected);
    }
    check_state(registry, store, model).await
}

async fn replay(steps: Vec<Step>) -> TestCaseResult {
    let registry = RwLock::new(FeatureFlagRegistry::new());
    let store = ModelStore::default();
    let mut model = Reference::default();
    for (
        kind,
        deployment,
        flag,
        enabled,
        failure,
        second_deployment,
        second_flag,
        second_enabled,
        release_early,
    ) in steps
    {
        let first = command(deployment, flag, enabled)?;
        store.failure.store(failure, Ordering::Relaxed);
        match kind {
            0 => {
                let expected = model.write(&first, failure);
                prop_assert_eq!(set_override(&registry, &store, first).await, expected);
            }
            1 => run_query(&registry, &store, &model, &first).await?,
            _ => {
                let second = command(second_deployment, second_flag, second_enabled)?;
                run_pair(
                    TraceContext {
                        registry: &registry,
                        store: &store,
                        model: &mut model,
                    },
                    (&first, &second),
                    release_early,
                    failure,
                )
                .await?;
            }
        }
        check_state(&registry, &store, &model).await?;
    }
    Ok(())
}

proptest! {
    #[test]
    fn bounded_flag_command_traces_preserve_model_state(steps in prop::collection::vec(
        (0u8..3, 0u8..3, 0u8..3, any::<bool>(), 0u8..3,
            0u8..3, 0u8..3, any::<bool>(), any::<bool>()), 1..25)) {
        let runtime = tokio::runtime::Builder::new_current_thread().build()
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        // Manual completion probes must not confuse cooperative-budget yields with admission.
        runtime.block_on(tokio::task::unconstrained(replay(steps)))?;
    }
}
