//! Plan construction and the additional-careful-call preview.
use serde_json::json;

use super::consolidation;
use super::digest_ops::{
    digest, evidence_digest, hash, input_digest, price, route, signed, subjects,
};
use super::extraction;
use super::types::{Batch, CallGroup, Counts, Extra, PHASES, Plan, Snapshot, phase};
use crate::Result;
use crate::domain::settings::Settings;
use crate::domain::source::{parse_source_meta, source_body, video};

/// Plan the phase using the exact shared video previews supplied by local inspection.
pub fn plan(
    snapshot: &Snapshot,
    settings: &Settings,
    phase_name: &str,
    private: bool,
    videos: Vec<video::VideoPlan>,
) -> Result<Plan> {
    phase(phase_name)?;
    let mut plan = Plan {
        digest: String::new(),
        phase: phase_name.into(),
        sources: Vec::new(),
        deferred_subjects: snapshot
            .batches
            .get(phase_name)
            .map_or_else(Vec::new, |batch| {
                if !batch.overflow.is_empty() {
                    batch.overflow.clone()
                } else if batch.used.iter().all(|used| *used == 0) && batch.extra_used == 0 {
                    batch.plan.deferred_subjects.clone()
                } else {
                    Vec::new()
                }
            }),
        private,
        inputs_digest: String::new(),
        evidence_digest: evidence_digest(snapshot)?,
        counts: Counts::default(),
        groups: Vec::new(),
        videos,
        estimate: None,
        rates_unset: Vec::new(),
    };
    let mut expected = plan
        .deferred_subjects
        .iter()
        .map(|name| json!({"name":name}))
        .collect::<Vec<_>>();
    let mut text_tokens = 0_u64;
    let mut audio_seconds = 0_u64;
    let mut all_private = private;
    for source in &snapshot.sources {
        if let Some(value) = &source.digest {
            digest(value)?;
        }
        if !source.in_phase(phase_name)?
            || (source.digest.is_some() && plan.deferred_subjects.is_empty())
            || source.needs_ocr()?
        {
            continue;
        }
        let fields = source.fields()?;
        let meta = parse_source_meta(&source.markdown);
        let is_private = private || meta.sensitivity == "private";
        all_private |= is_private;
        plan.sources.push(meta.id.clone());
        if source.digest.is_some() {
            continue;
        } // Carry-over needs synthesis only.
        let group_route = route(settings, is_private, false)?;
        if fields["kind"] == "video" {
            let preview = plan
                .videos
                .iter()
                .find(|p| p.id == meta.id)
                .ok_or_else(|| {
                    video::refusal("invalid_bootstrap_data", "video preview is missing")
                })?;
            if preview.sensitivity != if is_private { "private" } else { "normal" }
                || preview.settings_digest != video::settings_digest(settings)?
            {
                return Err(video::refusal(
                    "plan_changed",
                    "video policy changed during batch preview",
                ));
            }
            plan.counts.audio += preview.counts.audio;
            plan.counts.frames += preview.counts.frames;
            plan.counts.extract += preview.counts.extract;
            if preview.transcribe_audio {
                audio_seconds += preview.duration_ms.div_ceil(1000);
            }
            let timeline_tokens = u64::try_from(source_body(&source.markdown).chars().count())
                .unwrap_or(0)
                .div_ceil(4)
                + u64::try_from(preview.counts.frames).unwrap_or(0) * 256
                + if preview.transcribe_audio {
                    preview.duration_ms.div_ceil(60_000) * 200
                } else {
                    0
                };
            text_tokens += timeline_tokens.max(1) * if preview.counts.extract > 1 { 2 } else { 1 };
            for (kind, count) in [
                ("audio", preview.counts.audio),
                ("frames", preview.counts.frames),
                ("extract", preview.counts.extract),
            ] {
                if count > 0 {
                    plan.groups.push(CallGroup {
                        kind: kind.into(),
                        source: Some(meta.id.clone()),
                        calls: count,
                        route: group_route.clone(),
                    });
                }
            }
        } else {
            let windows = extraction::windows(&source_body(&source.markdown));
            let calls = extraction::calls(windows.len());
            plan.counts.digest += calls;
            text_tokens += windows
                .iter()
                .map(|text| u64::try_from(text.chars().count()).unwrap_or(0).div_ceil(4))
                .sum::<u64>()
                + u64::try_from(calls).unwrap_or(0) * 1024
                + if windows.len() > 1 {
                    u64::try_from(windows.len()).unwrap_or(0) * 1024
                } else {
                    0
                };
            plan.groups.push(CallGroup {
                kind: "digest".into(),
                source: Some(meta.id),
                calls,
                route: group_route,
            });
        }
        for line in source_body(&source.markdown)
            .lines()
            .filter(|line| line.starts_with('#'))
        {
            let heading = line.trim_start_matches('#').trim();
            if !heading.is_empty() {
                expected.push(json!({"name":heading}));
            }
        }
    }
    // Earlier phases provide expected names; alias components prevent duplicate calls.
    let phase_index = PHASES.iter().position(|p| *p == phase_name).unwrap_or(0);
    for source in &snapshot.sources {
        if let Some(value) = &source.digest {
            let earlier = PHASES[..phase_index]
                .iter()
                .any(|p| source.in_phase(p).unwrap_or(false));
            if earlier {
                expected.extend(value["subjects"].as_array().into_iter().flatten().cloned());
            }
            if earlier || source.in_phase(phase_name)? {
                all_private |= parse_source_meta(&source.markdown).sensitivity == "private";
            }
        }
    }
    plan.counts.careful = if plan.sources.is_empty() {
        0
    } else {
        subjects(expected.into_iter()).len().max(1)
    };
    if plan.counts.careful > 0 {
        plan.groups.push(CallGroup {
            kind: "careful".into(),
            source: None,
            calls: plan.counts.careful,
            route: route(settings, all_private, true)?,
        });
    }
    text_tokens += u64::try_from(plan.counts.careful).unwrap_or(0) * 2048;
    (plan.estimate, plan.rates_unset) = price(
        settings,
        text_tokens,
        plan.counts.frames,
        audio_seconds,
        !plan.groups.is_empty(),
    )?;
    plan.inputs_digest = input_digest(snapshot, &plan, settings)?;
    plan.digest = signed(&plan)?;
    Ok(plan)
}

/// Validate a confirmed plan against the current source and settings snapshot.
pub fn verify_plan(snapshot: &Snapshot, settings: &Settings, plan: &Plan) -> Result<()> {
    if plan.digest != signed(plan)? || plan.inputs_digest != input_digest(snapshot, plan, settings)?
    {
        return Err(video::refusal(
            "plan_changed",
            "batch sources, settings or route changed; preview and confirm again",
        ));
    }
    Ok(())
}

pub(super) fn extra(snapshot: &Snapshot, settings: &Settings, batch: &Batch) -> Result<Extra> {
    let mut values = Vec::new();
    let mut runbooks = Vec::new();
    let mut private = batch.plan.private
        || batch
            .plan
            .groups
            .iter()
            .any(|group| group.kind == "careful" && group.route.route.starts_with("private:"));
    for source in &snapshot.sources {
        if source.in_phase(&batch.plan.phase)? {
            let value = source.digest.as_ref().ok_or_else(|| {
                video::refusal(
                    "digests_incomplete",
                    "finish the batch digests before previewing additional careful calls",
                )
            })?;
            digest(value)?;
            let meta = source.fields()?;
            let procedures = value["procedures"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|raw| {
                    consolidation::procedure(
                        raw,
                        source,
                        meta["id"].as_str().unwrap_or_default(),
                        &value["as_of"],
                    )
                })
                .collect::<Vec<_>>();
            for subject in consolidation::candidate_subjects(value, &procedures) {
                if subject["template"] == "runbook" {
                    runbooks.push(subject);
                } else {
                    values.push(subject);
                }
            }
            private |= parse_source_meta(&source.markdown).sensitivity == "private";
        }
    }
    let groups = subjects(values.into_iter());
    let runbooks = subjects(runbooks.into_iter());
    let calls = (groups.len() + runbooks.len()).saturating_sub(batch.plan.counts.careful);
    let route = route(settings, private, true)?;
    let (estimate, rates_unset) = price(
        settings,
        u64::try_from(calls).unwrap_or(0) * 2048,
        0,
        0,
        calls > 0,
    )?;
    let digest = hash(
        &json!({"plan":batch.plan.digest,"evidence":evidence_digest(snapshot)?,"groups":groups,"runbooks":runbooks,"route":route,"estimate":estimate,"rates_unset":rates_unset}),
    )?;
    Ok(Extra {
        digest,
        calls,
        route,
        estimate,
        rates_unset,
    })
}
