//! Signals domain: fixture histories produce exactly the expected
//! candidates with their evidence ids (specs/proposals.md#signals).

use crate::domain::settings::SignalThresholds;
use crate::domain::signals::{
    AutomationFact, FrictionFact, PageFact, ProposalKind, SignalFacts, SourceFact, TaskFact, scan,
};

fn thresholds() -> SignalThresholds {
    SignalThresholds {
        repetition_count: 3,
        window_days: 30,
        stale_days: 90,
        friction_count: 2,
        new_subject_count: 2,
        unused_days: 30,
    }
}

fn task(id: &str, title: &str) -> TaskFact {
    TaskFact {
        id: id.into(),
        title: title.into(),
        description: String::new(),
        tags: vec!["owner:you".into(), "report".into()],
        status: "done".into(),
        completed_days_ago: Some(5),
        user_corrected: false,
        has_runbook: false,
    }
}

#[test]
fn repetition_fires_at_threshold_and_not_below() {
    let mut facts = SignalFacts::default();
    for i in 1..=3 {
        facts
            .tasks
            .push(task(&format!("t{i}"), "weekly status report"));
    }
    let found = scan(&facts, &thresholds());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].signal, "repetition");
    assert_eq!(found[0].kind, ProposalKind::Routine);
    assert_eq!(found[0].evidence, vec!["t1", "t2", "t3"]);
    facts.tasks.pop();
    assert!(scan(&facts, &thresholds()).is_empty());
}

#[test]
fn repetition_ignores_old_and_unowned_tasks() {
    let mut facts = SignalFacts::default();
    for i in 1..=3 {
        let mut t = task(&format!("t{i}"), "weekly status report");
        t.completed_days_ago = Some(90);
        facts.tasks.push(t);
    }
    let mut unowned = task("t9", "weekly status report");
    unowned.tags = vec!["report".into()];
    facts.tasks.push(unowned);
    assert!(scan(&facts, &thresholds()).is_empty());
}

#[test]
fn changing_a_library_threshold_changes_the_result() {
    let mut facts = SignalFacts::default();
    for i in 1..=3 {
        facts
            .tasks
            .push(task(&format!("t{i}"), "weekly status report"));
    }
    let mut raised = thresholds();
    raised.repetition_count = 4;
    assert!(scan(&facts, &raised).is_empty());
    let mut friction = SignalFacts::default();
    for i in 1..=2 {
        friction.frictions.push(FrictionFact {
            id: format!("f{i}"),
            text: "deploy script fails on macos".into(),
            link: None,
        });
    }
    assert_eq!(scan(&friction, &thresholds()).len(), 1);
    let mut raised = thresholds();
    raised.friction_count = 3;
    assert!(scan(&friction, &raised).is_empty());
}

#[test]
fn friction_recurrence_by_similarity_and_link() {
    let mut facts = SignalFacts::default();
    facts.frictions.push(FrictionFact {
        id: "f1".into(),
        text: "deploy script fails on macos".into(),
        link: None,
    });
    facts.frictions.push(FrictionFact {
        id: "f2".into(),
        text: "deploy script fails again on macos".into(),
        link: None,
    });
    facts.frictions.push(FrictionFact {
        id: "f3".into(),
        text: "unrelated lunch order issue".into(),
        link: None,
    });
    let found = scan(&facts, &thresholds());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].signal, "friction_recurrence");
    assert_eq!(found[0].kind, ProposalKind::RunbookFromFriction);
    assert_eq!(found[0].evidence, vec!["f1", "f2"]);
}

#[test]
fn staleness_gap_and_unused_automation() {
    let mut facts = SignalFacts::default();
    facts.pages.push(PageFact {
        id: "dom-a".into(),
        title: "A".into(),
        last_verified_days_ago: Some(120),
        cited_by: vec!["src-1".into()],
        has_newer_evidence: true,
        ..PageFact::default()
    });
    facts.pages.push(PageFact {
        id: "dom-b".into(),
        title: "B".into(),
        last_verified_days_ago: Some(10),
        cited_by: vec!["src-2".into()],
        has_newer_evidence: true,
        ..PageFact::default()
    });
    let mut corrected = task("t1", "draft the memo");
    corrected.user_corrected = true;
    facts.tasks.push(corrected);
    let mut covered = task("t2", "draft the memo");
    covered.user_corrected = true;
    covered.has_runbook = true;
    facts.tasks.push(covered);
    facts.automation.push(AutomationFact {
        id: "auto-1".into(),
        name: "nightly sync".into(),
        last_opened_days_ago: None,
    });
    facts.automation.push(AutomationFact {
        id: "auto-2".into(),
        name: "daily digest".into(),
        last_opened_days_ago: Some(1),
    });
    let found = scan(&facts, &thresholds());
    let signals: Vec<&str> = found.iter().map(|c| c.signal).collect();
    assert_eq!(signals, vec!["gap", "staleness", "unused_automation"]);
    assert_eq!(found[0].evidence, vec!["t1"]);
    assert_eq!(found[1].evidence, vec!["dom-a", "src-1"]);
    assert_eq!(found[2].evidence, vec!["auto-1"]);
    assert_eq!(found[2].kind, ProposalKind::Retire);
}

#[test]
fn new_subject_fires_at_threshold_and_not_when_covered_or_below() {
    let mut facts = SignalFacts::default();
    for i in 1..=2 {
        facts.sources.push(SourceFact {
            id: format!("src-{i}"),
            participants: vec!["Priya Nair".into()],
            tags: vec!["client:acme".into()],
            body: String::new(),
        });
    }
    facts.pages.push(PageFact {
        id: "dom-acme".into(),
        title: "Acme".into(),
        ..PageFact::default()
    });
    let found = scan(&facts, &thresholds());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].signal, "new_subject");
    assert_eq!(found[0].kind, ProposalKind::KnowledgeAdd);
    assert_eq!(found[0].evidence, vec!["src-1", "src-2"]);
    assert!(found[0].summary.contains("priya nair"));
    // Below threshold: one source only.
    facts.sources.pop();
    assert!(scan(&facts, &thresholds()).is_empty());
    // Covered subject: a page titled for the participant suppresses it.
    facts.sources.push(SourceFact {
        id: "src-3".into(),
        participants: vec!["Priya Nair".into()],
        tags: Vec::new(),
        body: String::new(),
    });
    facts.pages.push(PageFact {
        id: "dom-priya".into(),
        title: "Priya Nair".into(),
        ..PageFact::default()
    });
    assert!(scan(&facts, &thresholds()).is_empty());
}

#[test]
fn unrecorded_direction_fires_only_when_standing_page_does_not_cite_source() {
    let mut facts = SignalFacts::default();
    facts.sources.push(SourceFact {
        id: "src-1".into(),
        body: "[2026-10-27T10:00:00Z] direction: always cite sources\n[2026-10-27T10:00:00Z] rhythm: weekly report".into(),
        ..SourceFact::default()
    });
    let found = scan(&facts, &thresholds());
    let signals: Vec<&str> = found.iter().map(|c| c.signal).collect();
    assert_eq!(signals, vec!["declared_cadence", "unrecorded_direction"]);
    assert_eq!(found[1].kind, ProposalKind::KnowledgeUpdate);
    assert_eq!(found[1].evidence, vec!["src-1"]);
    facts.pages.push(PageFact {
        id: "dom-standing-directions".into(),
        cited_by: vec!["src-1".into()],
        ..PageFact::default()
    });
    assert_eq!(
        scan(&facts, &thresholds()),
        found,
        "an incoming citation does not record a direction"
    );
    facts.pages.clear();
    // Once the standing-directions page cites the source, it is recorded.
    facts.pages.push(PageFact {
        id: "dom-standing-directions".into(),
        title: "Standing directions".into(),
        cites: vec!["src-1#L1".into()],
        ..PageFact::default()
    });
    let found = scan(&facts, &thresholds());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].signal, "declared_cadence");
}

#[test]
fn declared_cadence_fires_only_when_no_automation_covers_it() {
    let mut facts = SignalFacts::default();
    facts.sources.push(SourceFact {
        id: "src-1".into(),
        body: "rhythm: weekly report - every monday".into(),
        ..SourceFact::default()
    });
    let found = scan(&facts, &thresholds());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].signal, "declared_cadence");
    assert_eq!(found[0].kind, ProposalKind::AutoTask);
    assert_eq!(found[0].evidence, vec!["src-1"]);
    facts.automation.push(AutomationFact {
        id: "auto-1".into(),
        name: "weekly report".into(),
        last_opened_days_ago: Some(1),
    });
    assert!(scan(&facts, &thresholds()).is_empty());
}
