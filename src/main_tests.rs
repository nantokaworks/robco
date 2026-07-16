use super::*;

fn parsed_list_roots(args: &[&str]) -> Vec<PathBuf> {
    let args = Args::try_parse_from(args).unwrap();
    let Some(Command::List(list_args)) = args.command else {
        panic!("expected list command");
    };
    effective_roots(
        std::path::Path::new("/managed"),
        list_args.dir.as_deref().or(args.launch_dir.as_deref()),
    )
}

fn mapped_report_error(args: &[&str]) -> Option<&'static str> {
    let raw = args.iter().map(OsString::from).collect::<Vec<_>>();
    let error = Args::try_parse_from(args).unwrap_err();
    cli::report_parse_error_message(&error, cli::invocation_targets_report(&raw))
}

#[test]
fn list_defaults_to_managed_root() {
    assert_eq!(
        parsed_list_roots(&["robco", "list"]),
        vec![PathBuf::from("/managed")]
    );
}

#[test]
fn list_directory_prefers_subcommand_directory() {
    assert_eq!(
        parsed_list_roots(&["robco", "list", "/d"]),
        vec![PathBuf::from("/managed"), PathBuf::from("/d")]
    );
}

#[test]
fn list_directory_falls_back_to_launch_directory() {
    assert_eq!(
        parsed_list_roots(&["robco", "/d", "list"]),
        vec![PathBuf::from("/managed"), PathBuf::from("/d")]
    );
}

#[test]
fn effective_roots_deduplicates_identical_arguments() {
    assert_eq!(
        effective_roots(
            std::path::Path::new("/managed"),
            Some(std::path::Path::new("/managed"))
        ),
        vec![PathBuf::from("/managed")]
    );
}

#[test]
fn report_argument_errors_use_concise_mapping() {
    let expected = Some("robco report: invalid arguments (see --help)");
    for args in [
        &["robco", "report"][..],
        &["robco", "report", "--unknown"][..],
        &["robco", "/d", "report"][..],
        &["robco", "--program=x", "report", "--unknown"][..],
    ] {
        assert_eq!(mapped_report_error(args), expected);
    }
}

#[test]
fn non_argument_report_errors_keep_clap_mapping() {
    for args in [
        &["robco", "report", "--help"][..],
        &["robco", "report", "--version"][..],
        &["robco", "install", "--unknown"][..],
    ] {
        assert_eq!(mapped_report_error(args), None);
    }
    let raw = ["robco", "--program", "report"].map(OsString::from);
    assert!(!cli::invocation_targets_report(&raw));
}
