use super::*;

/// Write one v2 SCR into the fake Library, as a scribe pass left it.
fn insert_scr(
    library: &FakeLibrary,
    id: &str,
    scope: &str,
    project: Option<&str>,
    updated: &str,
    next_task: &str,
) -> String {
    let project = project.map_or_else(|| "null".to_string(), |name| format!("\"{name}\""));
    let path = format!("sources/conversation/2026/11/{id}.md");
    let text = format!(
        "---\nid: \"{id}\"\ntitle: \"Title of {id}\"\ntitle_by: \"scribe\"\n\
         scope: \"{scope}\"\nproject: {project}\nsession: \"session-{id}\"\n\
         started: \"{updated}\"\nupdated: \"{updated}\"\nprevious: null\n\
         scribed_through: \"m-0002\"\nprovider: \"claude-code\"\nmodel: null\n\
         host: \"macbook\"\ncwd: \"/work/x\"\nbranch: null\ntranscript: null\n\
         parsed: null\nsensitivity: \"normal\"\n---\n## Summary\n\
         (scribe) Summary of {id}.\n\n## Goals\n- Goal for {next_task}.\n\n\
         ## Outcome and Blockers\n- Outcome of {id}.\n\n## Next\n\
         - {next_task} — continue {id}. owner: agent.\n\n## Tags\n"
    );
    library.insert_scribe_file(&path, &text);
    path
}

#[test]
fn project_scr_leads_and_global_scr_is_first_in_global() {
    let build = || {
        let l = FakeLibrary::initialized();
        // Legacy entries are newer than both SCRs; the SCR still wins its
        // scope, and the legacy global follows the global SCR, marked.
        ps(
            &l,
            "src-legacy",
            "3000",
            "orbit-office/claude",
            "01 handoff: Legacy office next.\n01:01 handoff: (global) Legacy global note.\n",
        );
        insert_scr(
            &l,
            "SCR-20261029-0900-aaaa01",
            "project",
            Some("orbit-office"),
            "2026-10-29T09:00:00Z",
            "ORB-11111",
        );
        insert_scr(
            &l,
            "SCR-20261028-0900-bbbb02",
            "global",
            None,
            "2026-10-28T09:00:00Z",
            "ORB-22222",
        );
        l
    };
    for input in [
        json!({"project": "orbit-office", "orchestrator": "claude"}),
        json!({"view": "handoffs", "project": "orbit-office", "orchestrator": "claude"}),
    ] {
        let response = if input.get("view").is_some() {
            let (ports, orbit) = at(build(), "2026-10-30T12:00:00Z", vec![]);
            let response = brief(&ports, input);
            assert!(orbit.calls().is_empty());
            response
        } else {
            noon_brief(build(), input)
        };
        let handoff = &items(&response, "handoff")[0];
        assert_eq!(handoff["id"], "SCR-20261029-0900-aaaa01", "{response}");
        assert_eq!(
            handoff["link"],
            "sources/conversation/2026/11/SCR-20261029-0900-aaaa01.md"
        );
        assert_eq!(handoff["time"], "2026-10-29T09:00:00Z");
        assert_eq!(handoff["entry_kind"], "handoff");
        assert!(handoff.get("legacy").is_none(), "{response}");
        let text = handoff["text"].as_str().unwrap();
        for want in [
            "Title of SCR-20261029-0900-aaaa01",
            "2026-10-29T09:00:00Z",
            "(scribe) Summary of SCR-20261029-0900-aaaa01.",
            "Goals:",
            "Outcome and Blockers:",
            "Next:",
            "ORB-11111 — continue SCR-20261029-0900-aaaa01.",
        ] {
            assert!(text.contains(want), "{want} in {text}");
        }
        let order: Vec<_> = ["Summary:", "Goals:", "Outcome and Blockers:", "Next:"]
            .iter()
            .map(|label| text.find(label).unwrap())
            .collect();
        assert!(order.windows(2).all(|p| p[0] < p[1]), "{text}");
        let global = items(&response, "global");
        assert_eq!(global.len(), 2, "{response}");
        assert_eq!(global[0]["id"], "SCR-20261028-0900-bbbb02");
        assert!(global[0].get("legacy").is_none(), "{response}");
        assert_eq!(global[1]["id"], "src-legacy#L2");
        assert_eq!(global[1]["legacy"], true);
        assert_eq!(global[1]["text"], "(global) Legacy global note.");
        assert_eq!(section(&response, "global").unwrap()["total"], 2);
    }
}

#[test]
fn two_concurrent_scrs_put_the_newer_first_and_the_other_in_other_sessions() {
    let l = FakeLibrary::initialized();
    insert_scr(
        &l,
        "SCR-20261030-0900-aaaa01",
        "project",
        Some("orbit-office"),
        "2026-10-30T09:00:00Z",
        "ORB-11111",
    );
    insert_scr(
        &l,
        "SCR-20261029-0900-bbbb02",
        "project",
        Some("orbit-office"),
        "2026-10-29T09:00:00Z",
        "ORB-22222",
    );
    // Ten days older than the newest: outside the seven-day window.
    insert_scr(
        &l,
        "SCR-20261020-0900-cccc03",
        "project",
        Some("orbit-office"),
        "2026-10-20T09:00:00Z",
        "ORB-33333",
    );
    // Another project inside the window: a different scope, not listed.
    insert_scr(
        &l,
        "SCR-20261030-0800-dddd04",
        "project",
        Some("orbit"),
        "2026-10-30T08:00:00Z",
        "ORB-44444",
    );
    let response = noon_brief(
        l,
        json!({"project": "orbit-office", "orchestrator": "claude"}),
    );
    expect_ids(&response, "handoff|SCR-20261030-0900-aaaa01");
    expect_ids(&response, "other_sessions|SCR-20261029-0900-bbbb02");
    assert_eq!(section(&response, "other_sessions").unwrap()["total"], 1);
    let other = &items(&response, "other_sessions")[0];
    assert_eq!(other["time"], "2026-10-29T09:00:00Z");
    assert!(
        other["text"]
            .as_str()
            .unwrap()
            .contains("(scribe) Summary of SCR-20261029-0900-bbbb02."),
        "{response}"
    );
}

#[test]
fn a_scope_without_an_scr_keeps_its_legacy_handoff_and_a_structured_one_renders_unchanged() {
    let l = FakeLibrary::initialized();
    // orbit has an SCR; orbit-office has only a structured legacy entry.
    insert_scr(
        &l,
        "SCR-20261030-0900-aaaa01",
        "project",
        Some("orbit"),
        "2026-10-30T09:00:00Z",
        "ORB-11111",
    );
    let structured = json!({
        "objective": "Finish the repair for ORB-12345.",
        "next": "Run the acceptance checks and read the report.",
        "authorization": "The orchestrator owns the repair; checks are authorized.",
        "observations": [{"text": "Two workers were active.", "observed_at": "2026-11-01T10:00:00Z", "recheck": "Recheck worker state."}],
        "evidence": ["orbit://task/ORB-12345"]
    });
    source_in(
        &l,
        "src-structured",
        "2026-10-30T08:00:00Z",
        "orbit-office",
        "claude",
        &format!(
            "[2026-10-30T09:00:00Z] handoff: (structured) {}\n",
            serde_json::to_string(&structured).unwrap()
        ),
    );
    let response = noon_brief(
        l,
        json!({"project": "orbit-office", "orchestrator": "claude"}),
    );
    let item = &items(&response, "handoff")[0];
    assert_eq!(item["id"], "src-structured#L1", "{response}");
    assert_eq!(item["structured_handoff"], structured);
    assert!(item.get("legacy").is_none(), "{response}");
    let text = item["text"].as_str().unwrap();
    let order: Vec<_> = [
        "Objective:",
        "Next:",
        "Authorization:",
        "Observations:",
        "Evidence:",
    ]
    .iter()
    .map(|label| text.find(label).unwrap())
    .collect();
    assert!(order.windows(2).all(|p| p[0] < p[1]), "{text}");
    assert!(text.contains("observed at 2026-11-01T10:00:00Z"), "{text}");
}

#[test]
fn manual_projects_list_the_latest_scr_per_project_and_legacy_only_projects() {
    let manual = mode_library("manual");
    write_office_orbit(&manual);
    insert_scr(
        &manual,
        "SCR-20261029-0900-aaaa01",
        "project",
        Some("orbit-office"),
        "2026-10-29T09:00:00Z",
        "ORB-11111",
    );
    insert_scr(
        &manual,
        "SCR-20261031-0900-bbbb02",
        "project",
        Some("orbit-office"),
        "2026-10-31T09:00:00Z",
        "ORB-22222",
    );
    let response = noon_brief(
        manual,
        json!({"project": "orbit-office", "orchestrator": "claude", "session_start": true}),
    );
    assert!(section(&response, "handoff").is_none(), "{response}");
    let projects = items(&response, "projects");
    assert_eq!(projects.len(), 2, "{response}");
    // The SCR project lists its latest SCR; the legacy-only project keeps
    // its anchored entry.
    assert_eq!(projects[0]["id"], "orbit-office");
    assert_eq!(
        projects[0]["link"],
        "sources/conversation/2026/11/SCR-20261031-0900-bbbb02.md"
    );
    assert_eq!(projects[0]["time"], "2026-10-31T09:00:00Z");
    assert_eq!(projects[1]["id"], "orbit");
    assert_eq!(
        projects[1]["link"],
        "sources/2026/10/src-orbit/source.md#L1"
    );
}

#[test]
fn auto_assignment_fallback_finds_the_newest_scr_whose_next_cites_the_task() {
    let l = FakeLibrary::initialized();
    // A legacy handoff in orbit-office cites the task, but the SCR rule is
    // tried first: the orbit SCR's Next cites it too.
    one(
        &l,
        "src-office|3002|orbit-office/claude|03 handoff: ORB-77007: legacy next.\n",
    );
    insert_scr(
        &l,
        "SCR-20261030-0900-aaaa01",
        "project",
        Some("orbit"),
        "2026-10-30T09:00:00Z",
        "ORB-77007",
    );
    let (ports, _) = at(
        l,
        "2026-10-30T12:00:00Z",
        vec![
            Ok(json!({"id":"ORB-77007","status":"in-progress"})),
            empty_tasks(),
        ],
    );
    let response = brief(
        &ports,
        json!({"orchestrator": "claude", "session_start": true, "task": "ORB-77007"}),
    );
    expect_ids(&response, "handoff|SCR-20261030-0900-aaaa01");
    assert!(section(&response, "projects").is_none(), "{response}");
}
