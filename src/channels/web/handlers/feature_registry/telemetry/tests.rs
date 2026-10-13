//! Availability attribution observes degraded policy without identifier labels.

use super::test_support::RecordingObserver;
use super::*;
use crate::feature_flags::{FlagName, FlagSnapshot};
use std::collections::HashMap;

#[test]
fn degraded_availability_has_fixed_subsystem_and_surface_observations() {
    let observer = RecordingObserver::default();
    let unavailable =
        crate::feature_flags::availability::SubsystemAvailability::default().unavailable_flags();
    tracing::subscriber::with_default(observer.clone().subscriber(), || {
        availability_decisions(&unavailable, &HashMap::new(), &FlagSnapshot::default());
    });
    let records = observer.observations();
    let absent = records
        .iter()
        .filter(|record| record.0["message"] == "feature_flags.unavailable_subsystem")
        .collect::<Vec<_>>();
    let disabled = records
        .iter()
        .filter(|record| record.0["message"] == "feature_flags.availability_disable")
        .collect::<Vec<_>>();
    assert_eq!(absent.len(), 5);
    assert_eq!(disabled.len(), 10);
    for record in records {
        assert!(matches!(
            record.0["subsystem"].as_str(),
            "jobs" | "routines" | "extensions" | "skills" | "logs"
        ));
        assert!(
            record
                .0
                .keys()
                .all(|key| matches!(key.as_str(), "message" | "subsystem" | "surface"))
        );
    }
}

#[rstest::rstest]
#[case::explicit_enable(true)]
#[case::explicit_disable(false)]
fn explicit_overrides_do_not_attribute_disables_to_availability(#[case] enabled: bool) {
    let observer = RecordingObserver::default();
    let snapshot = FlagSnapshot::from_values([(
        FlagName::parse("action_job_restart").expect("known flag"),
        enabled,
    )]);
    let environment = HashMap::from([("route_jobs", enabled)]);
    tracing::subscriber::with_default(observer.clone().subscriber(), || {
        availability_decisions(
            &["route_jobs", "action_job_restart"],
            &environment,
            &snapshot,
        );
    });
    let records = observer.observations();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].0["message"],
        "feature_flags.unavailable_subsystem"
    );
    assert_eq!(records[0].0["subsystem"], "jobs");
}
