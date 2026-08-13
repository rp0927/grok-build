use super::*;

fn tmp_store() -> (tempfile::TempDir, TeamStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = TeamStore::new(dir.path());
    (dir, store)
}

#[test]
fn find_by_session_matches_lead() {
    let (_dir, store) = tmp_store();
    let team = store.create_for_lead("sess-lead-99", "lead", 1).unwrap();
    let found = store.find_by_session("sess-lead-99").unwrap().unwrap();
    assert_eq!(found.team_id, team.team_id);
    assert!(store.find_by_session("missing").unwrap().is_none());
}

#[test]
fn team_id_uses_first_eight_hex_digits() {
    assert_eq!(
        team_id_from_session("019ffb5b-ad0b-7833-8305-0d9b097d7e74"),
        "session-019ffb5b"
    );
    assert_eq!(team_id_from_session("!!!"), "session-unknown");
}

#[test]
fn create_team_and_add_member() {
    let (_dir, store) = tmp_store();
    let team = store
        .create_for_lead("sess-lead-01", "lead", 1_000)
        .unwrap();
    assert_eq!(team.members.len(), 1);
    assert_eq!(team.lead().unwrap().name, "lead");

    let team = store
        .add_member(&team.team_id, "reviewer", Some("sess-rev".into()), None)
        .unwrap();
    assert_eq!(team.members.len(), 2);
    assert!(team.member("reviewer").is_some());
    assert!(
        store
            .read_inbox(&team.team_id, "reviewer")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn send_message_rejects_unknown_and_self() {
    let (_dir, store) = tmp_store();
    let team = store.create_for_lead("s1", "lead", 1).unwrap();
    store.add_member(&team.team_id, "reviewer", None, None).unwrap();

    let err = send_message(
        &store,
        &team.team_id,
        "lead",
        "lead",
        MailboxKind::Text,
        None,
        "hi".into(),
        2,
        "m1".into(),
    )
    .unwrap_err();
    assert!(matches!(err, MailboxError::SelfSend));

    let err = send_message(
        &store,
        &team.team_id,
        "lead",
        "ghost",
        MailboxKind::Text,
        None,
        "hi".into(),
        2,
        "m2".into(),
    )
    .unwrap_err();
    assert!(matches!(err, MailboxError::UnknownRecipient(_)));
}

#[test]
fn send_message_appends_to_recipient_only() {
    let (_dir, store) = tmp_store();
    let team = store.create_for_lead("s1", "lead", 1).unwrap();
    store.add_member(&team.team_id, "reviewer", None, None).unwrap();

    send_message(
        &store,
        &team.team_id,
        "lead",
        "reviewer",
        MailboxKind::Text,
        Some("review".into()),
        "please read src/lib.rs".into(),
        3,
        "m1".into(),
    )
    .unwrap();

    let inbox = store.read_inbox(&team.team_id, "reviewer").unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].from, "lead");
    assert_eq!(inbox[0].body, "please read src/lib.rs");
    assert!(store.read_inbox(&team.team_id, "lead").unwrap().is_empty());
}

#[test]
fn claim_blocked_until_dependency_completes() {
    let (_dir, store) = tmp_store();
    let team = store.create_for_lead("s1", "lead", 1).unwrap();
    store.add_member(&team.team_id, "builder", None, None).unwrap();
    store.add_member(&team.team_id, "reviewer", None, None).unwrap();

    create_task(
        &store,
        &team.team_id,
        "t-impl".into(),
        "implement".into(),
        vec![],
        2,
    )
    .unwrap();
    create_task(
        &store,
        &team.team_id,
        "t-review".into(),
        "review".into(),
        vec!["t-impl".into()],
        3,
    )
    .unwrap();

    let err = claim_task(&store, &team.team_id, "t-review", "reviewer").unwrap_err();
    assert!(matches!(err, TaskError::Blocked(_, deps) if deps == ["t-impl"]));

    claim_task(&store, &team.team_id, "t-impl", "builder").unwrap();
    complete_task(&store, &team.team_id, "t-impl").unwrap();

    let claimed = claim_task(&store, &team.team_id, "t-review", "reviewer").unwrap();
    assert_eq!(claimed.status, TaskStatus::InProgress);
    assert_eq!(claimed.assignee.as_deref(), Some("reviewer"));
}

#[test]
fn second_claim_fails() {
    let (_dir, store) = tmp_store();
    let team = store.create_for_lead("s1", "lead", 1).unwrap();
    store.add_member(&team.team_id, "a", None, None).unwrap();
    store.add_member(&team.team_id, "b", None, None).unwrap();
    create_task(&store, &team.team_id, "t1".into(), "work".into(), vec![], 2).unwrap();

    claim_task(&store, &team.team_id, "t1", "a").unwrap();
    let err = claim_task(&store, &team.team_id, "t1", "b").unwrap_err();
    assert!(matches!(
        err,
        TaskError::WrongStatus(_, TaskStatus::InProgress)
    ));
}
