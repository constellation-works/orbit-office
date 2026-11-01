use super::*;

#[test]
fn brief_for_a_project_leads_with_its_handoff_and_lists_the_other_project() {
    let l = FakeLibrary::initialized();
    let add = |spec: &str| one(&l, spec);
    add("src-a|2608|project-a/claude|09 handoff: A handoff for ORB-111.\n");
    add("src-b|2708|project-b/codex|09 handoff: B handoff — for ORB-222.\n");
    let response = brief_empty(l, json!({"project": "project-a", "orchestrator": "claude"}));
    expect_text(&response, "handoff", "A handoff for ORB-111.");
    let others = items(&response, "other_sessions");
    assert_eq!(others.len(), 1, "{response}");
    assert_eq!(others[0]["text"], "B handoff — for ORB-222.");
    assert_eq!(others[0]["entry_kind"], "handoff");
    assert_eq!(others[0]["time"], "2026-10-27T09:00:00Z");
}

#[test]
fn brief_for_a_project_falls_back_to_an_older_handoff_in_that_project() {
    let l = FakeLibrary::initialized();
    let add = |spec: &str| one(&l, spec);
    add("src-a-old|2608|project-a/claude|09 handoff: Older A handoff.\n");
    add("src-a-new|2708|project-a/claude|09 decision: New A decision, no handoff.\n");
    let response = brief_empty(l, json!({"project": "project-a", "orchestrator": "claude"}));
    expect_text(&response, "handoff", "Older A handoff.");
    assert_eq!(items(&response, "since")[0]["id"], "src-a-new");
}

#[test]
fn two_orchestrators_in_one_project_keep_their_own_handoffs() {
    let l = FakeLibrary::initialized();
    let add = |spec: &str| one(&l, spec);
    add("src-claude|2608|project-a/claude|09 handoff: Claude thread for ORB-111.\n");
    add("src-codex|2708|project-a/codex|09 handoff: Codex thread for ORB-222.\n");
    let response = brief_empty(l, json!({"project": "project-a", "orchestrator": "claude"}));
    expect_text(&response, "handoff", "Claude thread for ORB-111.");
    let others = items(&response, "other_sessions");
    assert_eq!(others.len(), 1, "{response}");
    assert_eq!(others[0]["text"], "Codex thread for ORB-222.");
}

#[test]
fn since_is_scoped_to_the_project_and_no_project_keeps_todays_output() {
    let build = || {
        let l = FakeLibrary::initialized();
        let add = |spec: &str| one(&l, spec);
        add("src-a|2608|project-a/claude|09 handoff: A handoff for ORB-111.\n");
        add("src-b|2708|project-b/codex|09 handoff: B handoff for ORB-222.\n");
        fixture(l, json!([done("finished-between", "2026-10-26T12:00:00Z")]))
    };
    let (ports, _, _) = build();
    let scoped = brief(
        &ports,
        json!({"project": "project-a", "orchestrator": "claude"}),
    );
    assert_eq!(items(&scoped, "since")[0]["id"], "src-a");
    assert_eq!(items(&scoped, "since")[0]["time"], "2026-10-26T09:00:00Z");
    // Work finished after project A's last session counts as since then.
    expect_ids(&scoped, "agents_working_on|finished-between");
    let (ports, _, _) = build();
    let legacy = brief(&ports, json!({}));
    expect_text(&legacy, "handoff", "B handoff for ORB-222.");
    assert_eq!(items(&legacy, "since")[0]["id"], "src-b");
    assert!(section(&legacy, "other_sessions").is_none());
    assert!(items(&legacy, "agents_working_on").is_empty());
}

#[test]
fn other_sessions_is_bounded_by_window_and_count() {
    let l = FakeLibrary::initialized();
    let add = |spec: &str| one(&l, spec);
    add("src-a|2708|project-a/claude|09 handoff: A handoff for ORB-111.\n");
    for n in 0..7 {
        let row =
            format!("src-other-{n}|2608|project-x{n}/codex|09 handoff: Other {n} for ORB-30{n}.\n");
        one(&l, &row);
    }
    source_in(
        &l,
        "src-ancient",
        "2026-09-01T08:00:00Z",
        "project-old",
        "codex",
        "[2026-09-01T09:00:00Z] handoff: Ancient for ORB-399.\n",
    );
    let response = brief_empty(l, json!({"project": "project-a", "orchestrator": "claude"}));
    let others = items(&response, "other_sessions");
    assert_eq!(others.len(), 5, "{response}");
    assert_eq!(section(&response, "other_sessions").unwrap()["total"], 7);
    assert_eq!(
        section(&response, "other_sessions").unwrap()["truncated"],
        true
    );
    assert!(
        others
            .iter()
            .all(|item| !item["text"].as_str().unwrap().contains("Ancient")),
        "{response}"
    );
}

fn globals_library() -> FakeLibrary {
    let l = FakeLibrary::initialized();
    let add = |spec: &str| one(&l, spec);
    add(
        "src-a|3008|orbit-office/claude|09 handoff: (global) Upgrade orbit to 0.9 on every host.\n09:01 handoff: (global until 2026-10-31T00:00:00Z) Drain paused until Friday.\n09:02 handoff: (global) Orbit defect: rerun drains with --resume.\n09:03 handoff: (global) Local scope note.\n09:04 resolved: #L4 closed here.\n09:05 handoff: Office next: ship the brief.\n",
    );
    add(
        "src-b|3010|orbit/claude|10:01 resolved: src-a#L3 — fixed in orbit 0.9.1.\n10:02 resolved: src-a#L30 is another entry.\n10:03 handoff: Orbit next: cut 0.9.2.\n",
    );
    l
}

#[test]
fn global_handoffs_reach_every_project_until_they_expire_or_are_resolved() {
    for project in ["orbit-office", "orbit"] {
        let response = brief_at_empty(
            globals_library(),
            "2026-10-30T12:00:00Z",
            json!({"project": project, "orchestrator": "claude"}),
        );
        // Before expiry both open globals reach both projects; resolved ones never do.
        assert_eq!(
            ids(&response, "global"),
            ["src-a#L2", "src-a#L1"],
            "{project}"
        );
        let lead = items(&response, "handoff");
        assert!(
            lead[0]["text"].as_str().unwrap().contains("next"),
            "{response}"
        );
        // Globals never become a project thread's handoff or another session.
        assert!(
            items(&response, "other_sessions")
                .iter()
                .all(|i| !i["text"].as_str().unwrap().starts_with("(global"))
        );
    }
    // After `until` passes it is left out; the brief without a project carries open globals too.
    let response = brief_at_empty(
        globals_library(),
        "2026-10-31T00:00:00Z",
        json!({"project": "orbit"}),
    );
    expect_ids(&response, "global|src-a#L1");
    let response = brief_at_empty(globals_library(), "2026-11-01T00:00:00Z", json!({}));
    expect_ids(&response, "global|src-a#L1");
    expect_ids(&response, "handoff|src-b#L3");
    let global = &items(&response, "global")[0];
    assert_eq!(
        global["text"],
        "(global) Upgrade orbit to 0.9 on every host."
    );
    assert_eq!(global["entry_kind"], "handoff");
}

#[test]
fn project_handoff_leads_then_capped_globals_newest_first_with_a_cited_global_once() {
    let l = FakeLibrary::initialized();
    let add = |spec: &str| one(&l, spec);
    let globals: String = (0..7)
        .map(|n| format!("0{n}:00 handoff: (global) Global note {n}.\n"))
        .collect();
    ps(&l, "src-g", "2900", "orbit/claude", &globals);
    add(
        "src-p|3000|orbit-office/claude|01 handoff: Next: follow src-g#L1 and src-g#L7 before shipping.\n",
    );
    let response = noon_brief(
        l,
        json!({"project": "orbit-office", "orchestrator": "claude"}),
    );
    expect_kinds_prefix(&response, "handoff,global");
    expect_ids(&response, "handoff|src-p#L1");
    // Newest first, capped at five; cited oldest stays, cited newest is not repeated.
    expect_ids(
        &response,
        "global|src-g#L7,src-g#L6,src-g#L5,src-g#L4,src-g#L3,src-g#L1",
    );
    let global = section(&response, "global").unwrap();
    assert_eq!(global["total"], 7);
    assert_eq!(global["truncated"], true);
}

#[test]
fn session_start_mode_manual_lists_projects_and_auto_leads_with_the_project() {
    let manual = mode_library("manual");
    write_office_orbit(&manual);
    let input = json!({"project": "orbit-office", "orchestrator": "claude", "session_start": true});
    let response = noon_brief(manual, input.clone());
    expect_kinds_prefix(&response, "global,projects,since");
    expect_ids(&response, "global|src-office#L2");
    expect_ids(&response, "projects|orbit,orbit-office");
    let project = &items(&response, "projects")[0];
    assert_eq!(project["link"], "sources/2026/10/src-orbit/source.md#L1");
    assert_eq!(project["time"], "2026-10-30T03:00:00Z");
    let auto = FakeLibrary::initialized();
    write_office_orbit(&auto);
    let response = noon_brief(auto, input);
    expect_kinds_prefix(&response, "handoff,global,other_sessions");
    expect_ids(&response, "handoff|src-office#L1");
    assert!(section(&response, "projects").is_none());
    // Mode governs only the SessionStart brief: explicit request still gets its project.
    let manual = mode_library("manual");
    write_office_orbit(&manual);
    let response = noon_brief(
        manual,
        json!({"project": "orbit-office", "orchestrator": "claude"}),
    );
    expect_ids(&response, "handoff|src-office#L1");
}

#[test]
fn auto_without_a_project_falls_back_to_the_open_assignment_or_to_manual_output() {
    let write = |l: &FakeLibrary| {
        one(
            l,
            "src-orbit|3000|orbit/claude|01 handoff: ORB-77: next, land the drain fix.\n",
        );
        one(
            l,
            "src-office|3002|orbit-office/claude|03 handoff: Office next.\n",
        );
    };
    // The caller's open assignment names the thread's project.
    let l = FakeLibrary::initialized();
    write(&l);
    let (ports, orbit) = at(
        l,
        "2026-10-30T12:00:00Z",
        vec![
            Ok(json!({"id":"ORB-77","status":"in-progress"})),
            empty_tasks(),
        ],
    );
    let input = json!({"orchestrator": "claude", "session_start": true, "task": "ORB-77"});
    let response = brief(&ports, input.clone());
    expect_ids(&response, "handoff|src-orbit#L1");
    assert!(section(&response, "projects").is_none());
    assert_eq!(orbit.calls()[0].input["id"], "ORB-77");
    // A finished, unknown or unreadable assignment falls back to manual.
    for reply in [
        Ok(json!({"id":"ORB-77","status":"done"})),
        Err(office_core::Error::OrbitUnavailable),
    ] {
        let l = FakeLibrary::initialized();
        write(&l);
        let response = brief_at(
            l,
            "2026-10-30T12:00:00Z",
            vec![reply, empty_tasks()],
            input.clone(),
        );
        assert!(section(&response, "handoff").is_none(), "{response}");
        expect_ids(&response, "projects|orbit-office,orbit");
    }
    // No assignment at all: manual output, and no Orbit task lookup.
    let l = FakeLibrary::initialized();
    write(&l);
    let (ports, orbit) = at(l, "2026-10-30T12:00:00Z", vec![empty_tasks()]);
    let response = brief(
        &ports,
        json!({"orchestrator": "claude", "session_start": true}),
    );
    assert!(section(&response, "handoff").is_none(), "{response}");
    expect_ids(&response, "projects|orbit-office,orbit");
    assert_eq!(orbit.calls().len(), 1);
    // `task` is only the SessionStart fallback.
    let (ports, _) = at(FakeLibrary::initialized(), "2026-10-30T12:00:00Z", vec![]);
    let response = call(&ports, json!({"task": "ORB-77"}));
    assert_eq!(response["error"]["code"], "invalid_argument", "{response}");
}

#[test]
fn handoffs_view_returns_the_merged_handoff_or_the_projects_without_task_reads() {
    let l = office_orbit_library();
    let (ports, orbit) = at(l, "2026-10-30T12:00:00Z", vec![]);
    let response = brief(
        &ports,
        json!({"view": "handoffs", "project": "orbit", "orchestrator": "claude"}),
    );
    expect_kinds(&response, "handoff,global");
    expect_ids(&response, "handoff|src-orbit#L1");
    expect_ids(&response, "global|src-office#L2");
    let response = brief(&ports, json!({"view": "handoffs"}));
    expect_kinds(&response, "global,projects");
    expect_ids(&response, "projects|orbit,orbit-office");
    assert!(orbit.calls().is_empty(), "the handoff view reads no tasks");
    for input in [
        json!({"view": "handoffs", "session_start": true}),
        json!({"view": "everything"}),
    ] {
        let response = call(&ports, input);
        assert_eq!(response["error"]["code"], "invalid_argument", "{response}");
    }
}
