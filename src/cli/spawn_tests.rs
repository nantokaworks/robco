use super::*;

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
    assert_eq!(args.title.as_deref(), Some("task"));
    assert!(args.autonomous);
}

#[test]
fn parses_spawn_subcommand_with_dropr_task() {
    let args = Args::try_parse_from([
        "robco",
        "spawn",
        "--repo",
        "repo",
        "--dropr-task",
        "538",
        "--autonomous",
    ])
    .unwrap();
    let Some(Command::Spawn(args)) = args.command else {
        panic!("expected spawn command")
    };
    assert_eq!(args.dropr_task.as_deref(), Some("538"));
    assert_eq!(args.title, None);
}

#[test]
fn spawn_requires_either_title_or_dropr_task() {
    let result = Args::try_parse_from(["robco", "spawn", "--repo", "repo"]);
    assert!(result.is_err());
}

#[test]
fn spawn_rejects_dropr_task_combined_with_an_explicit_title() {
    let result = Args::try_parse_from([
        "robco",
        "spawn",
        "--repo",
        "repo",
        "--dropr-task",
        "538",
        "--title",
        "hand-picked",
    ]);
    assert!(result.is_err());
}

#[test]
fn spawn_rejects_dropr_task_combined_with_an_explicit_prompt() {
    let result = Args::try_parse_from([
        "robco",
        "spawn",
        "--repo",
        "repo",
        "--dropr-task",
        "538",
        "--prompt",
        "hand-picked",
    ]);
    assert!(result.is_err());
}

#[test]
fn spawn_rejects_dropr_task_combined_with_an_explicit_name_slug() {
    let result = Args::try_parse_from([
        "robco",
        "spawn",
        "--repo",
        "repo",
        "--dropr-task",
        "538",
        "--name-slug",
        "hand-picked",
    ]);
    assert!(result.is_err());
}
