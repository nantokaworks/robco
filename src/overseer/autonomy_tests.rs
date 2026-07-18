use super::*;

fn known_low_risk() -> ChangeFacts {
    ChangeFacts {
        facts_known: true,
        files_changed: 2,
        lines_changed: 40,
        only_docs_or_tests: true,
        ..ChangeFacts::default()
    }
}

fn full_auto() -> OverseerConfig {
    OverseerConfig {
        autonomy_level: AutonomyLevel::FullAuto,
        ..OverseerConfig::default()
    }
}

fn assert_full_auto_hard_stop(facts: ChangeFacts, expected: RiskCategory) {
    let Decision::Escalate(risks) = classify(&facts, &full_auto()) else {
        panic!("hard stop was allowed")
    };
    assert_eq!(risks, vec![expected]);
}

#[test]
fn full_auto_escalates_destructive_changes_alone() {
    assert_full_auto_hard_stop(
        ChangeFacts {
            is_destructive: true,
            ..known_low_risk()
        },
        RiskCategory::IrreversibleOrDestructive,
    );
}

#[test]
fn full_auto_escalates_security_changes_alone() {
    assert_full_auto_hard_stop(
        ChangeFacts {
            touches_security: true,
            ..known_low_risk()
        },
        RiskCategory::SecuritySensitive,
    );
}

#[test]
fn full_auto_escalates_repeated_failures_alone() {
    let config = full_auto();
    assert_full_auto_hard_stop(
        ChangeFacts {
            consecutive_failures: config.failure_circuit_threshold,
            ..known_low_risk()
        },
        RiskCategory::RepeatedFailures,
    );
}

#[test]
fn full_auto_escalates_exhausted_budget_alone() {
    let config = full_auto();
    assert_full_auto_hard_stop(
        ChangeFacts {
            llm_calls_today: config.daily_llm_budget,
            ..known_low_risk()
        },
        RiskCategory::BudgetExceeded,
    );
}

#[test]
fn full_auto_escalates_external_side_effects_alone() {
    assert_full_auto_hard_stop(
        ChangeFacts {
            has_external_side_effect: true,
            ..known_low_risk()
        },
        RiskCategory::ExternalSideEffect,
    );
}

#[test]
fn full_auto_allows_each_soft_risk_alone() {
    let soft_risks = [
        ChangeFacts {
            touches_dependencies: true,
            ..known_low_risk()
        },
        ChangeFacts {
            files_changed: LARGE_FILE_COUNT + 1,
            ..known_low_risk()
        },
        ChangeFacts {
            touches_prod_or_ci: true,
            ..known_low_risk()
        },
        ChangeFacts {
            ambiguous_requirements: true,
            ..known_low_risk()
        },
    ];
    for facts in soft_risks {
        assert_eq!(classify(&facts, &full_auto()), Decision::Auto);
    }
}

#[test]
fn repeated_failure_threshold_is_exact() {
    let config = full_auto();
    let below = ChangeFacts {
        consecutive_failures: config.failure_circuit_threshold - 1,
        ..known_low_risk()
    };
    let at = ChangeFacts {
        consecutive_failures: config.failure_circuit_threshold,
        ..known_low_risk()
    };
    assert_eq!(classify(&below, &config), Decision::Auto);
    assert!(matches!(classify(&at, &config), Decision::Escalate(_)));
}

#[test]
fn budget_threshold_is_exact() {
    let config = full_auto();
    let below = ChangeFacts {
        llm_calls_today: config.daily_llm_budget - 1,
        ..known_low_risk()
    };
    let at = ChangeFacts {
        llm_calls_today: config.daily_llm_budget,
        ..known_low_risk()
    };
    assert_eq!(classify(&below, &config), Decision::Auto);
    assert!(matches!(classify(&at, &config), Decision::Escalate(_)));
}

#[test]
fn large_diff_thresholds_trip_only_above_the_limits() {
    let config = OverseerConfig::default();
    for facts in [
        ChangeFacts {
            facts_known: true,
            files_changed: LARGE_FILE_COUNT,
            lines_changed: LARGE_LINE_COUNT,
            ..ChangeFacts::default()
        },
        ChangeFacts {
            facts_known: true,
            files_changed: LARGE_FILE_COUNT + 1,
            ..ChangeFacts::default()
        },
        ChangeFacts {
            facts_known: true,
            lines_changed: LARGE_LINE_COUNT + 1,
            ..ChangeFacts::default()
        },
    ] {
        let has_large_diff = risks(&facts, &config).contains(&RiskCategory::LargeDiff);
        let should_trip =
            facts.files_changed > LARGE_FILE_COUNT || facts.lines_changed > LARGE_LINE_COUNT;
        assert_eq!(has_large_diff, should_trip);
    }
}

#[test]
fn unknown_facts_escalate_under_every_level_after_gates_pass() {
    for autonomy_level in [
        AutonomyLevel::ApprovalOnly,
        AutonomyLevel::Conservative,
        AutonomyLevel::FullAuto,
    ] {
        let config = OverseerConfig {
            autonomy_level,
            ..OverseerConfig::default()
        };
        assert_eq!(
            merge_envelope_decision(true, true, &ChangeFacts::default(), &config),
            Decision::Escalate(Vec::new())
        );
    }
}

#[test]
fn known_low_risk_changes_auto_merge_under_conservative() {
    assert_eq!(
        merge_envelope_decision(true, true, &known_low_risk(), &OverseerConfig::default()),
        Decision::Auto
    );
}

#[test]
fn deterministic_gates_cannot_be_bypassed_by_any_level() {
    for autonomy_level in [
        AutonomyLevel::ApprovalOnly,
        AutonomyLevel::Conservative,
        AutonomyLevel::FullAuto,
    ] {
        let config = OverseerConfig {
            autonomy_level,
            ..OverseerConfig::default()
        };
        assert!(matches!(
            merge_envelope_decision(false, true, &known_low_risk(), &config),
            Decision::Escalate(_)
        ));
        assert!(matches!(
            merge_envelope_decision(true, false, &known_low_risk(), &config),
            Decision::Escalate(_)
        ));
    }
}
