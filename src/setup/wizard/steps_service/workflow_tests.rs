use std::{collections::BTreeMap, io::Cursor};

use super::{
    materialize_with,
    plan::BootstrapMode,
    probe::ServiceState,
    workflow::{Caller, WorkerState, prepare_with, warning_message},
};
use crate::overseer::command::ActiveWorkers;

fn workers(repos: &[(&str, usize)]) -> ActiveWorkers {
    let repos: BTreeMap<_, _> = repos
        .iter()
        .map(|(repo, count)| ((*repo).to_string(), *count))
        .collect();
    ActiveWorkers {
        count: repos.values().sum(),
        repos,
    }
}

#[test]
fn warning_builder_covers_idle_active_and_unknown() {
    assert_eq!(warning_message(&WorkerState::Active(workers(&[]))), None);
    let active =
        warning_message(&WorkerState::Active(workers(&[("robco", 2), ("dropr", 1)]))).unwrap();
    assert!(active.contains("3 active workers"));
    assert!(active.contains(r#"{"dropr": 1, "robco": 2}"#));
    let unknown = warning_message(&WorkerState::Unknown).unwrap();
    assert!(unknown.contains("could not be determined"));
    assert!(unknown.contains("missing or unreadable"));
}

#[test]
fn shared_entry_covers_every_state_for_both_callers() {
    for caller in [Caller::Wizard, Caller::InstallCommand] {
        for state in [
            ServiceState::NotInstalled,
            ServiceState::Unloaded,
            ServiceState::Loaded,
        ] {
            let answers: &[u8] = match (caller, state) {
                (Caller::Wizard, ServiceState::NotInstalled) => b"y\nn\n",
                (Caller::Wizard, _) => b"\n",
                (Caller::InstallCommand, ServiceState::Loaded) => b"y\n",
                (Caller::InstallCommand, _) => b"",
            };
            let mut input = Cursor::new(answers);
            let mut output = Vec::new();
            let plan = prepare_with(
                &mut input,
                &mut output,
                caller,
                || state,
                || Ok(workers(&[])),
            )
            .unwrap();

            assert!(plan.write_plist);
            assert_eq!(
                matches!(plan.mode, BootstrapMode::Reload),
                state == ServiceState::Loaded
            );
            assert!(String::from_utf8(output).unwrap().contains("launchd"));
        }
    }
}

#[test]
fn declining_busy_warning_executes_neither_rewrite_nor_reload() {
    let mut input = Cursor::new(b"\n\n");
    let mut output = Vec::new();
    let plan = prepare_with(
        &mut input,
        &mut output,
        Caller::Wizard,
        || ServiceState::Loaded,
        || Ok(workers(&[("robco", 1)])),
    )
    .unwrap();
    let mut plist_writes = 0;
    let bootstrap = materialize_with(plan, &mut output, || {
        plist_writes += 1;
        Ok("/tmp/com.robco.overseer.plist".into())
    })
    .unwrap();

    assert_eq!(plist_writes, 0);
    assert!(bootstrap.is_none(), "no plan can execute launchctl bootout");
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("Reload anyway? [y/N]"));
    assert!(output.contains("service left as-is"));
}
