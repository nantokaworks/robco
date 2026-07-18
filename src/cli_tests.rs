use super::*;

#[test]
fn parses_report_subcommand() {
    let args = Args::try_parse_from([
        "robco",
        "report",
        "--message",
        "turn finished",
        "--target",
        "controller",
    ])
    .unwrap();

    let Some(Command::Report(report)) = args.command else {
        panic!("expected report command");
    };
    assert_eq!(report.message.as_deref(), Some("turn finished"));
    assert_eq!(report.target.as_deref(), Some("controller"));
}

#[test]
fn parses_add_subcommand() {
    let args = Args::try_parse_from([
        "robco",
        "add",
        "ssh://git@host:29418/owner/repo.git",
        "--branch",
        "dev",
        "--name",
        "local",
    ])
    .unwrap();
    let Some(Command::Add(args)) = args.command else {
        panic!("expected add command");
    };
    assert_eq!(args.branch.as_deref(), Some("dev"));
    assert_eq!(args.name.as_deref(), Some("local"));
}

#[test]
fn parses_spawn_subcommand() {
    let args = Args::try_parse_from([
        "robco",
        "spawn",
        "--repo",
        "repo",
        "--title",
        "task",
        "--autonomous",
    ])
    .unwrap();
    let Some(Command::Spawn(args)) = args.command else {
        panic!("expected spawn command")
    };
    assert_eq!(args.repo, "repo");
    assert_eq!(args.title, "task");
    assert!(args.autonomous);
}

#[test]
fn parses_overseer_set() {
    let args = Args::try_parse_from(["robco", "overseer", "set", "auto-merge", "on"]).unwrap();
    let Some(Command::Overseer(args)) = args.command else {
        panic!("expected overseer")
    };
    assert!(matches!(args.command, OverseerCommand::Set(_)));
}

#[test]
fn parses_new_subcommand() {
    let args = Args::try_parse_from(["robco", "new", "--title", "x", "--prompt", "y"]).unwrap();
    let Some(Command::New(args)) = args.command else {
        panic!("expected new command");
    };
    assert_eq!(args.title, "x");
    assert_eq!(args.prompt.as_deref(), Some("y"));
}

#[test]
fn parses_version_subcommand() {
    let args = Args::try_parse_from(["robco", "version"]).unwrap();
    assert!(matches!(args.command, Some(Command::Version)));
}

#[test]
fn parses_list_subcommand_with_default_directory() {
    let args = Args::try_parse_from(["robco", "list"]).unwrap();
    let Some(Command::List(args)) = args.command else {
        panic!("expected list command");
    };
    assert_eq!(args.dir, None);
}

#[test]
fn parses_list_subcommand_with_directory() {
    let args = Args::try_parse_from(["robco", "list", "/some/dir"]).unwrap();
    let Some(Command::List(args)) = args.command else {
        panic!("expected list command");
    };
    assert_eq!(args.dir, Some(PathBuf::from("/some/dir")));
}

#[test]
fn parses_list_subcommand_after_launch_directory() {
    let args = Args::try_parse_from(["robco", "/some/dir", "list"]).unwrap();
    assert_eq!(args.launch_dir, Some(PathBuf::from("/some/dir")));
    let Some(Command::List(args)) = args.command else {
        panic!("expected list command");
    };
    assert_eq!(args.dir, None);
}

#[test]
fn bare_invocation_has_no_ephemeral_root() {
    let args = Args::try_parse_from(["robco"]).unwrap();
    assert_eq!(args.launch_dir, None);
}

#[test]
fn rejects_removed_list_flag() {
    assert!(Args::try_parse_from(["robco", "--list"]).is_err());
}

#[test]
fn bare_install_requests_wizard() {
    let args = Args::try_parse_from(["robco", "install"]).unwrap();
    let Some(Command::Install(args)) = args.command else {
        panic!("expected install command");
    };
    assert!(args.wants_wizard());
    assert_eq!(args.target, None);
}

#[test]
fn install_all_and_explicit_target_use_legacy_path() {
    let all = Args::try_parse_from(["robco", "install", "--all"]).unwrap();
    let Some(Command::Install(all)) = all.command else {
        panic!("expected install command");
    };
    assert!(!all.wants_wizard());

    let target = Args::try_parse_from(["robco", "install", "--target", "codex"]).unwrap();
    let Some(Command::Install(target)) = target.command else {
        panic!("expected install command");
    };
    assert!(!target.wants_wizard());
    assert_eq!(target.target, Some(InstallTarget::Codex));
}
