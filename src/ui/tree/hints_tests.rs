use super::*;

#[test]
fn footer_carries_only_the_essential_hints() {
    let line = hints_line(None, None, None, false).to_string();
    assert_eq!(line, "[↵] ATTACH [n] NEW [m] MERGE [?] HELP [q] QUIT");
}

#[test]
fn agent_row_advertises_its_own_actions() {
    let line = hints_line(
        None,
        Some(Selection::Agent { repo: 0, agent: 0 }),
        None,
        false,
    )
    .to_string();
    assert_eq!(
        line,
        "[↵] ATTACH [r] RESTART [m] MERGE [p] PR [g] MANAGE [x] REMOVE [?] HELP [q] QUIT"
    );
}

#[test]
fn repo_row_advertises_its_own_actions() {
    let line = hints_line(None, Some(Selection::Repo(0)), None, false).to_string();
    assert_eq!(
        line,
        "[n] NEW [a] ADD [r] RELOAD [g] MANAGE [?] HELP [q] QUIT"
    );
}

#[test]
fn overseer_ai_row_advertises_attach_and_instruct() {
    let line = hints_line(None, Some(Selection::OverseerAi), None, false).to_string();
    assert_eq!(line, "[↵] ATTACH [i] INSTRUCT [?] HELP [q] QUIT");
}

#[test]
fn inbox_category_advertises_expand_and_clear() {
    let line = hints_line(
        None,
        Some(Selection::OverseerCategory(OverseerCategory::Inbox)),
        None,
        false,
    )
    .to_string();
    assert_eq!(line, "[l] EXPAND [D] CLEAR [?] HELP [q] QUIT");
}

#[test]
fn other_overseer_categories_carry_no_extra_action() {
    let line = hints_line(
        None,
        Some(Selection::OverseerCategory(OverseerCategory::Health)),
        None,
        false,
    )
    .to_string();
    assert_eq!(line, "[?] HELP [q] QUIT");
}

#[test]
fn inbox_item_advertises_answer_approve_dismiss_clear() {
    let line = hints_line(None, Some(Selection::OverseerInbox(0)), None, false).to_string();
    assert_eq!(
        line,
        "[↵] ANSWER [y] APPROVE [d] DISMISS [D] CLEAR [?] HELP [q] QUIT"
    );
}

#[test]
fn child_worktree_advertises_attach_only() {
    let line = hints_line(
        None,
        Some(Selection::ChildWorktree {
            repo: 0,
            agent: 0,
            child: 0,
        }),
        None,
        false,
    )
    .to_string();
    assert_eq!(line, "[↵] ATTACH [?] HELP [q] QUIT");
}

#[test]
fn dropr_task_list_focus_advertises_move_open_start_and_back() {
    let line = hints_line(
        None,
        Some(Selection::Repo(0)),
        Some(DroprTaskFocus { task: 0 }),
        false,
    )
    .to_string();
    assert_eq!(
        line,
        "[j/k] MOVE [↵] OPEN [n] START [o] BROWSER [esc] BACK [?] HELP [q] QUIT"
    );
}

#[test]
fn reading_a_task_body_advertises_scroll_start_browser_and_back_only() {
    let line = hints_line(
        None,
        Some(Selection::Repo(0)),
        Some(DroprTaskFocus { task: 0 }),
        true,
    )
    .to_string();
    assert_eq!(line, "[j/k] SCROLL [s] START [o] BROWSER [esc] BACK");
}

#[test]
fn a_repo_row_without_a_drill_down_keeps_its_own_hints() {
    let line = hints_line(None, Some(Selection::Repo(0)), None, false).to_string();
    assert_eq!(
        line,
        "[n] NEW [a] ADD [r] RELOAD [g] MANAGE [?] HELP [q] QUIT"
    );
}

#[test]
fn discord_channel_advertises_retry_and_remove() {
    let line = hints_line(None, Some(Selection::DiscordChannel(0)), None, false).to_string();
    assert_eq!(line, "[↵] ATTACH [r] RETRY [x] REMOVE [?] HELP [q] QUIT");
}

#[test]
fn orphan_advertises_attach_and_remove() {
    let line = hints_line(None, Some(Selection::Orphan(0)), None, false).to_string();
    assert_eq!(line, "[↵] ATTACH [x] REMOVE [?] HELP [q] QUIT");
}

#[test]
fn section_headers_carry_no_extra_action() {
    let line = hints_line(None, Some(Selection::OtherHeader), None, false).to_string();
    assert_eq!(line, "[?] HELP [q] QUIT");
    let line = hints_line(None, Some(Selection::OrphanHeader), None, false).to_string();
    assert_eq!(line, "[?] HELP [q] QUIT");
}
