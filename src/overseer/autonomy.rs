use serde::{Deserialize, Serialize};

use super::config::OverseerConfig;

const LARGE_FILE_COUNT: u32 = 20;
const LARGE_LINE_COUNT: u32 = 500;
const LOW_RISK_FILE_COUNT: u32 = 5;
const LOW_RISK_LINE_COUNT: u32 = 200;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyLevel {
    ApprovalOnly,
    #[default]
    Conservative,
    FullAuto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskCategory {
    AmbiguousRequirements,
    IrreversibleOrDestructive,
    SecuritySensitive,
    DependencyBump,
    LargeDiff,
    ProdOrCiConfig,
    RepeatedFailures,
    BudgetExceeded,
    ExternalSideEffect,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChangeFacts {
    pub facts_known: bool,
    pub files_changed: u32,
    pub lines_changed: u32,
    pub only_docs_or_tests: bool,
    pub ambiguous_requirements: bool,
    pub is_destructive: bool,
    pub touches_security: bool,
    pub touches_dependencies: bool,
    pub touches_prod_or_ci: bool,
    pub consecutive_failures: u32,
    pub llm_calls_today: u32,
    pub has_external_side_effect: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Auto,
    Escalate(Vec<RiskCategory>),
}

pub fn classify(facts: &ChangeFacts, config: &OverseerConfig) -> Decision {
    let risks = risks(facts, config);
    match config.autonomy_level {
        AutonomyLevel::ApprovalOnly => Decision::Escalate(risks),
        AutonomyLevel::Conservative => {
            let low_risk = facts.only_docs_or_tests
                && facts.files_changed <= LOW_RISK_FILE_COUNT
                && facts.lines_changed <= LOW_RISK_LINE_COUNT;
            if risks.is_empty() && low_risk {
                Decision::Auto
            } else {
                Decision::Escalate(risks)
            }
        }
        AutonomyLevel::FullAuto => {
            let hard_stops = risks
                .into_iter()
                .filter(|risk| {
                    matches!(
                        risk,
                        RiskCategory::IrreversibleOrDestructive
                            | RiskCategory::SecuritySensitive
                            | RiskCategory::RepeatedFailures
                            | RiskCategory::BudgetExceeded
                            | RiskCategory::ExternalSideEffect
                    )
                })
                .collect::<Vec<_>>();
            if hard_stops.is_empty() {
                Decision::Auto
            } else {
                Decision::Escalate(hard_stops)
            }
        }
    }
}

pub fn merge_envelope_decision(
    protection_verified: bool,
    checks_green: bool,
    facts: &ChangeFacts,
    config: &OverseerConfig,
) -> Decision {
    if !protection_verified || !checks_green {
        return Decision::Escalate(Vec::new());
    }
    if !facts.facts_known {
        return Decision::Escalate(Vec::new());
    }
    classify(facts, config)
}

fn risks(facts: &ChangeFacts, config: &OverseerConfig) -> Vec<RiskCategory> {
    let mut risks = Vec::new();
    let candidates = [
        (
            facts.ambiguous_requirements,
            RiskCategory::AmbiguousRequirements,
        ),
        (
            facts.is_destructive,
            RiskCategory::IrreversibleOrDestructive,
        ),
        (facts.touches_security, RiskCategory::SecuritySensitive),
        (facts.touches_dependencies, RiskCategory::DependencyBump),
        (
            facts.files_changed > LARGE_FILE_COUNT || facts.lines_changed > LARGE_LINE_COUNT,
            RiskCategory::LargeDiff,
        ),
        (facts.touches_prod_or_ci, RiskCategory::ProdOrCiConfig),
        (
            facts.consecutive_failures >= config.failure_circuit_threshold,
            RiskCategory::RepeatedFailures,
        ),
        (
            facts.llm_calls_today >= config.daily_llm_budget,
            RiskCategory::BudgetExceeded,
        ),
        (
            facts.has_external_side_effect,
            RiskCategory::ExternalSideEffect,
        ),
    ];
    risks.extend(
        candidates
            .into_iter()
            .filter_map(|(tripped, risk)| tripped.then_some(risk)),
    );
    risks
}

#[cfg(test)]
#[path = "autonomy_tests.rs"]
mod tests;
