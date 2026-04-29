//! Property tests for test-support helpers.

use std::collections::HashMap;

use proptest::prelude::*;

use crate::support::trace_template_utils::substitute_templates;

proptest! {
    /// `substitute_templates` must always terminate, even on deeply nested
    /// or cyclic `vars` maps, and never panic.
    #[test]
    fn substitute_templates_always_terminates(
        key in "[a-z]{1,8}",
        value in "[a-z{}.]{0,32}",
        input in "[a-z{}.]{0,64}",
    ) {
        let vars = HashMap::from([(key.clone(), serde_json::json!(value))]);
        let mut v = serde_json::json!(input);

        substitute_templates(&mut v, &vars);
    }

    /// The template-expansion limit is always respected.
    #[test]
    fn substitute_templates_respects_expansion_limit(
        n_vars in 1usize..=16,
    ) {
        let vars: HashMap<String, serde_json::Value> = (0..n_vars)
            .map(|i| {
                let key = format!("k{i}");
                let val = serde_json::json!(format!("{{{{k{}}}}}~", (i + 1) % n_vars));
                (key, val)
            })
            .collect();
        let mut v = serde_json::json!("prefix {{k0}} suffix");

        substitute_templates(&mut v, &vars);

        prop_assert!(
            v.as_str()
                .is_some_and(|value| value.contains("{{k") && value.contains("}}")),
            "cyclic expansion should stop with an unresolved template marker: {v:?}"
        );
        prop_assert_eq!(
            v.as_str()
                .expect("expanded template should remain a string")
                .matches('~')
                .count(),
            128
        );
    }

    /// Repeated calls to `setup_test_dir_with_suffix` never produce the same
    /// directory path twice within a single process.
    #[test]
    fn setup_test_dir_with_suffix_produces_unique_paths(n in 2usize..=16) {
        use std::collections::HashSet;

        use crate::support::cleanup::setup_test_dir_with_suffix;

        let base = tempfile::tempdir().expect("should create temp dir");
        let paths: HashSet<_> = (0..n)
            .map(|_| {
                setup_test_dir_with_suffix(base.path(), "prop-test")
                    .expect("should create unique dir")
            })
            .collect();

        prop_assert_eq!(paths.len(), n);
    }
}
