//! Applying batch mutations and validating a saved ledger.
use std::collections::{BTreeMap, BTreeSet};

use super::digest_ops::{evidence_digest, marked, signed};
use super::planning::{extra, verify_plan};
use super::types::{Batch, Mutation, Snapshot, Write, phase};
use crate::Result;
use crate::domain::settings::Settings;
use crate::domain::source::{parse_source_meta, video};

/// Apply under the Library writer lock after checking Snapshot::revision.
pub fn apply(snapshot: &Snapshot, settings: &Settings, mutation: Mutation) -> Result<Write> {
    let mut batches = snapshot.batches.clone();
    let mut sources = BTreeMap::new();
    let phase_name;
    match mutation {
        Mutation::Plan { plan, confirm, at } => {
            phase_name = phase(&plan.phase)?.to_string();
            verify_plan(snapshot, settings, &plan)?;
            if plan.evidence_digest != evidence_digest(snapshot)? {
                return Err(video::refusal(
                    "plan_changed",
                    "batch digest evidence changed; preview again",
                ));
            }
            let previous = batches
                .get(&phase_name)
                .filter(|old| old.plan.digest == plan.digest)
                .cloned();
            let mut batch = previous.unwrap_or_else(|| Batch {
                used: vec![0; plan.groups.len()],
                plan: *plan,
                confirmed_at: None,
                extra: None,
                extra_confirmed_at: None,
                extra_used: 0,
                consolidating: Vec::new(),
                overflow: Vec::new(),
            });
            if let Some(digest) = confirm {
                if snapshot
                    .batches
                    .get(&phase_name)
                    .is_none_or(|old| old.plan.digest != digest)
                    || batch.plan.digest != digest
                {
                    return Err(video::refusal(
                        "plan_changed",
                        "confirm the digest of the stored preview",
                    ));
                }
                if batch.confirmed_at.is_none() {
                    batch.confirmed_at = Some(at);
                }
            }
            if batch.plan.private {
                for source in &snapshot.sources {
                    if batch
                        .plan
                        .sources
                        .contains(&parse_source_meta(&source.markdown).id)
                    {
                        let text = marked(&source.markdown)?;
                        if text != source.markdown {
                            sources.insert(source.path.clone(), text);
                        }
                    }
                }
            }
            batches.insert(phase_name.clone(), batch);
        }
        Mutation::Extra {
            phase: name,
            confirm,
            at,
        } => {
            phase_name = phase(&name)?.into();
            let batch = batches.get_mut(&phase_name).ok_or_else(|| {
                video::refusal("plan_unconfirmed", "preview and confirm the batch first")
            })?;
            verify_plan(snapshot, settings, &batch.plan)?;
            if batch.confirmed_at.is_none() {
                return Err(video::refusal(
                    "plan_unconfirmed",
                    "confirm the batch before additional careful calls",
                ));
            }
            let preview = extra(snapshot, settings, batch)?;
            if let Some(digest) = confirm {
                if preview.digest != digest
                    || batch.extra.as_ref().is_none_or(|e| e.digest != digest)
                {
                    return Err(video::refusal(
                        "plan_changed",
                        "additional careful preview changed; confirm its current digest",
                    ));
                }
                if batch.extra_confirmed_at.is_none() {
                    batch.extra_confirmed_at = Some(at);
                }
            } else if batch.extra.as_ref() != Some(&preview) {
                if batch.extra_used > preview.calls {
                    return Err(video::refusal(
                        "plan_exceeded",
                        "revised careful estimate is below consumed calls",
                    ));
                }
                batch.extra_confirmed_at = None;
            }
            batch.extra = Some(preview);
        }
        Mutation::Reserve {
            phase: name,
            digest,
            group,
            extra: additional,
        } => {
            phase_name = phase(&name)?.into();
            let batch = batches.get_mut(&phase_name).ok_or_else(|| {
                video::refusal("plan_unconfirmed", "preview and confirm the batch first")
            })?;
            verify_plan(snapshot, settings, &batch.plan)?;
            if batch.confirmed_at.is_none() {
                return Err(video::refusal(
                    "plan_unconfirmed",
                    "confirm the batch before inference",
                ));
            }
            if additional {
                if group != 0 {
                    return Err(video::refusal(
                        "plan_exceeded",
                        "additional careful calls use group zero",
                    ));
                }
                let preview = extra(snapshot, settings, batch)?;
                if batch.extra_confirmed_at.is_none()
                    || batch.extra.as_ref() != Some(&preview)
                    || digest != preview.digest
                {
                    return Err(video::refusal(
                        "plan_changed",
                        "confirm the current additional careful preview",
                    ));
                }
                if batch.extra_used >= preview.calls {
                    return Err(video::refusal(
                        "plan_exceeded",
                        "additional careful allowance exhausted",
                    ));
                }
                batch.extra_used += 1;
            } else {
                if digest != batch.plan.digest {
                    return Err(video::refusal("plan_changed", "batch digest changed"));
                }
                let allowance = batch
                    .plan
                    .groups
                    .get(group)
                    .ok_or_else(|| video::refusal("plan_exceeded", "unknown batch call group"))?
                    .calls;
                let used = batch.used.get_mut(group).ok_or_else(|| {
                    video::refusal("invalid_bootstrap_data", "invalid call ledger")
                })?;
                if *used >= allowance {
                    return Err(video::refusal(
                        "plan_exceeded",
                        "batch call group exhausted; preview additional careful calls when needed",
                    ));
                }
                *used += 1;
            }
        }
        Mutation::Progress {
            phase: name,
            consolidating,
            overflow,
        } => {
            phase_name = phase(&name)?.into();
            let batch = batches
                .get_mut(&phase_name)
                .ok_or_else(|| video::refusal("plan_unconfirmed", "batch is unavailable"))?;
            if batch.confirmed_at.is_none() {
                return Err(video::refusal(
                    "plan_unconfirmed",
                    "confirm the batch first",
                ));
            }
            batch.consolidating = consolidating
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            batch.overflow = overflow
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
        }
    }
    let batch = batches.get(&phase_name).cloned().ok_or_else(|| {
        video::refusal(
            "invalid_bootstrap_data",
            "batch mutation produced no ledger",
        )
    })?;
    Ok(Write {
        batches,
        sources,
        batch,
    })
}

/// Refuse a corrupted saved ledger instead of granting unbounded or unsigned calls.
pub fn validate_batches(batches: &BTreeMap<String, Batch>) -> Result<()> {
    for (name, batch) in batches {
        phase(name)?;
        if batch.plan.phase != *name
            || batch.plan.digest != signed(&batch.plan)?
            || batch.used.len() != batch.plan.groups.len()
            || batch
                .used
                .iter()
                .zip(&batch.plan.groups)
                .any(|(used, group)| *used > group.calls)
            || (batch.confirmed_at.is_none()
                && (batch.used.iter().any(|n| *n > 0) || batch.extra_confirmed_at.is_some()))
            || batch.extra_used > batch.extra.as_ref().map_or(0, |extra| extra.calls)
            || (batch.extra_used > 0 && batch.extra_confirmed_at.is_none())
            || (batch.extra.is_none() && batch.extra_confirmed_at.is_some())
        {
            return Err(video::refusal(
                "invalid_bootstrap_data",
                "bootstrap ledger has inconsistent approvals or call counts",
            ));
        }
    }
    Ok(())
}
