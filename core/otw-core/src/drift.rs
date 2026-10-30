//! The drift suite: the tests that catch the agent's documentation drifting from the code it
//! describes (catalog prose and examples, shipped skills, indicator specs, vocabularies).
//! Run before a release with
//!
//! ```text
//! cargo test -p otw-core drift
//! ```
//!
//! The tests stay where they are, next to the code they guard. Each entry here re-runs one of
//! them by exact name through this same test binary, so the filter `drift` reaches them all
//! without renaming anything. A listed test that was renamed or removed fails here (0 run), so
//! the list cannot go stale silently.

use std::process::Command;

/// Run one test of this binary by its exact path and require that it ran and passed.
fn run_exact(name: &str) {
    let exe = std::env::current_exe().expect("the test binary");
    let out = Command::new(exe)
        .args([name, "--exact", "--test-threads=1"])
        .output()
        .expect("re-running the test binary");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "{name} failed:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("test result: ok. 1 passed"),
        "{name} did not run (renamed or removed? update src/drift.rs):\n{stdout}"
    );
}

macro_rules! drift {
    ($($fn_name:ident => $path:literal,)+) => {
        $(
            #[test]
            fn $fn_name() {
                run_exact($path);
            }
        )+
    };
}

drift! {
    // Catalog prose lint and examples (hardening phase 6).
    catalog_query_params_named_in_descriptions_exist =>
        "mcp::catalog::tests::query_params_named_in_descriptions_exist",
    catalog_description_lint_reads_clauses_and_assignments =>
        "mcp::catalog::tests::the_description_lint_reads_clauses_and_assignments",
    catalog_required_query_params_are_named =>
        "mcp::catalog::tests::required_query_params_are_named_in_the_description",
    catalog_every_example_is_a_valid_body =>
        "mcp::catalog::tests::every_catalog_example_is_a_valid_body",
    catalog_generated_descriptions_follow_the_code =>
        "mcp::catalog::tests::generated_descriptions_follow_the_code",
    // Shipped skills (phase 4).
    skill_backtest_run_json_blocks_are_valid_settings =>
        "agent::builtin::skill_tests::backtest_run_json_blocks_are_valid_settings",
    skill_oos_split_pct_is_described_as_in_sample =>
        "agent::builtin::skill_tests::oos_split_pct_is_described_as_in_sample",
    skill_backtest_run_lists_every_builtin_indicator =>
        "agent::builtin::skill_tests::backtest_run_lists_every_builtin_indicator",
    skill_build_indicator_lists_the_chainable_names =>
        "agent::builtin::skill_tests::build_indicator_lists_the_chainable_names",
    skill_shared_rules_carry_the_hardening_lines =>
        "agent::builtin::skill_tests::shared_rules_carry_the_hardening_lines",
    // Indicator spec completeness (phase 2).
    spec_every_match_arm_has_a_spec =>
        "backtest::specs::tests::every_match_arm_has_a_spec_and_every_spec_an_arm",
    spec_listed_params_are_exactly_the_ones_read =>
        "backtest::specs::tests::listed_params_are_exactly_the_ones_read",
    spec_example_settings_deserialize_and_validate =>
        "backtest::specs::tests::example_settings_deserialize_and_validate",
    spec_the_schema_lists_every_name =>
        "backtest::specs::tests::the_schema_lists_every_name",
    // Vocabulary registry (phase 7).
    vocab_registered_field_names_are_unique =>
        "util::vocab::tests::registered_field_names_are_unique",
}
