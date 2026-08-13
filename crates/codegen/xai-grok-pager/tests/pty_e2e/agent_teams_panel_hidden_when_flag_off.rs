// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// Flag off (default): after a normal turn the team panel chrome must not
/// appear. Spawn-and-row (flag on) is covered by unit tests on
/// `TeamPanel::desired_height` + `pending_to_materialize`; a live
/// `spawn_teammate` tool loop is out of scope for this smoke case.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run the owning pty_e2e_* Cargo test with --ignored (see Cargo.toml)"]
async fn agent_teams_panel_hidden_when_flag_off() {
    let content = ContentController::start().await.expect("start content");
    content.set_response(format!("{MOCK_RESPONSE_SENTINEL} no team here."));

    let binary = pager_binary().expect("resolve pager binary");
    let mut harness = PtyHarness::spawn_with_content_env(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &[],
        &[("GROK_EXPERIMENTAL_AGENT_TEAMS", "0")],
    )
    .expect("spawn pager with content");

    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome text");

    harness
        .inject_keys(format!("{PROMPT}\r").as_bytes())
        .expect("submit prompt");
    harness
        .wait_for_text(MOCK_RESPONSE_SENTINEL, Duration::from_secs(30))
        .expect("response on screen");

    let screen = harness.screen_contents();
    assert!(
        !screen.contains("enter open · x stop"),
        "team panel footer leaked with the flag off:\n{screen}"
    );
    assert!(
        !screen.contains(" ctrl+t tasks "),
        "team panel task hint leaked with the flag off:\n{screen}"
    );

    harness.quit().expect("clean quit");
}
