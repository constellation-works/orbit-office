//! Consolidate a confirmed batch into one reviewed proposal per eligible page.
use std::collections::{BTreeMap, BTreeSet};

use office_core::domain::bootstrap::{self, Batch, Mutation, consolidation};
use office_core::domain::source::video;
use office_core::domain::template::Template;
use office_core::ports::Sensitivity;
use office_core::ports::{ExtractRequest, ModelTier};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};
use crate::tools::proposals::create;

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "consolidate",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::None,
    },
    &["batch_id", "requested"],
    run,
);

fn mutate(call: &Call<'_>, mutation: Mutation) -> Result<Batch> {
    let snapshot = call.ports.library.bootstrap_snapshot()?;
    call.ports.library.mutate_batch(
        &snapshot.revision(&call.ports.library.settings()?)?,
        mutation,
    )
}

fn run(call: &Call<'_>) -> Result<Value> {
    let input: consolidation::Request = serde_json::from_value(Value::Object(call.input.clone()))
        .map_err(|_| {
        Error::InvalidArgument(
            "consolidate requires batch_id and optional requested subject names".into(),
        )
    })?;
    bootstrap::phase(&input.batch_id)?;
    if input.requested.len() > 256
        || input
            .requested
            .iter()
            .any(|s| s.trim().is_empty() || s.len() > 256)
    {
        return Err(Error::InvalidArgument(
            "requested must contain at most 256 nonempty subject names, each at most 256 bytes"
                .into(),
        ));
    }
    let snapshot = call.ports.library.bootstrap_snapshot()?;
    let settings = call.ports.library.settings()?;
    let batch = snapshot.batches.get(&input.batch_id).ok_or_else(|| {
        video::refusal(
            "plan_unconfirmed",
            "preview and confirm this batch before consolidation",
        )
    })?;
    if batch.confirmed_at.is_none() {
        return Err(video::refusal(
            "plan_unconfirmed",
            "confirm this batch before consolidation",
        ));
    }
    if !batch.is_pending_consolidation() {
        return Err(video::refusal(
            "batch_started",
            "this batch already started consolidation; inspect its proposals and preview a new batch before retrying",
        ));
    }
    let mut templates = BTreeMap::new();
    for file in call.ports.library.templates()? {
        let markdown = file.markdown.ok_or_else(|| {
            video::refusal(
                "invalid_bootstrap_data",
                "consolidation requires readable templates",
            )
        })?;
        let template = Template::from_markdown(&file.path, &markdown)?;
        templates
            .entry(template.template.clone())
            .or_insert(template);
    }
    let groups = consolidation::groups(&snapshot, batch, &templates)?;
    let budget = batch.plan.sources.len().div_ceil(3);
    let mut selected = Vec::new();
    let mut below_bar = Vec::new();
    let mut deferred = Vec::new();
    let mut new_pages = 0;
    let mut ids = BTreeSet::new();
    for group in groups {
        let template = templates
            .get(&group.template)
            .ok_or_else(|| Error::UnknownTemplate {
                id: group.key.clone(),
                template: group.template.clone(),
            })?;
        if !group.passes(template, &input.requested) {
            below_bar.push(group.key);
            continue;
        }
        if group.existing.is_none() && new_pages >= budget {
            deferred.push(group.key);
            continue;
        }
        if !ids.insert(group.page_id(template)) {
            return Err(video::refusal(
                "ambiguous_subject",
                "different subjects produce the same page id; refine their names before consolidation",
            ));
        }
        new_pages += usize::from(group.existing.is_none());
        selected.push(group);
    }
    // Preconditions and the complete call budget are checked before any side effect.
    let original = batch.plan.groups.iter().position(|g| g.kind == "careful");
    let allowance = original.map_or(0, |index| batch.plan.groups[index].calls);
    let additional = selected.len().saturating_sub(allowance);
    if additional > 0
        && (batch.extra_confirmed_at.is_none()
            || batch
                .extra
                .as_ref()
                .is_none_or(|extra| extra.calls < additional))
    {
        return Err(video::refusal(
            "plan_exceeded",
            "preview and confirm additional_careful calls with office.batch-plan before consolidation",
        ));
    }
    if additional > 0 {
        let digest = batch
            .extra
            .as_ref()
            .ok_or_else(|| video::refusal("plan_exceeded", "no additional careful preview"))?
            .digest
            .clone();
        bootstrap::apply(
            &snapshot,
            &settings,
            Mutation::Reserve {
                phase: input.batch_id.clone(),
                digest,
                group: 0,
                extra: true,
            },
        )?;
    }
    // Validate the saved approval against current sources/settings even for an empty result.
    bootstrap::verify_plan(&snapshot, &settings, &batch.plan)?;
    let policy = video::settings_digest(&settings)?;
    let private = batch
        .plan
        .groups
        .iter()
        .any(|g| g.kind == "careful" && g.route.route.starts_with("private:"))
        || selected
            .iter()
            .any(|g| g.private || templates[&g.template].sensitivity == Sensitivity::Private);
    if private
        && settings
            .open_weights
            .as_ref()
            .is_none_or(|e| !e.accepts(office_core::domain::endpoint::Capability::Text))
    {
        return Err(video::refusal(
            "private_endpoint_unverified",
            "configure and verify the private endpoint before consolidating private pages",
        ));
    }
    // Compare-and-swap claims the batch once. A competing call cannot start another
    // runner from the same snapshot; failed/uncertain attempts are never retried here.
    let active = selected.iter().map(|g| g.key.clone()).collect::<Vec<_>>();
    call.ports.library.mutate_batch(
        &snapshot.revision(&settings)?,
        Mutation::Progress {
            phase: input.batch_id.clone(),
            consolidating: active.clone(),
            overflow: deferred.clone(),
        },
    )?;
    let mut proposed = Vec::new();
    let mut merged = Vec::new();
    let mut conflicts = Vec::new();
    let mut failed = Vec::new();
    let mut suppressed = Vec::new();
    for (index, group) in selected.iter().enumerate() {
        let result = (|| {
            let template = &templates[&group.template];
            let current = call.ports.library.bootstrap_snapshot()?;
            // Compare this subject's evidence, allowing unrelated Library activity.
            let current_group = consolidation::groups(&current, batch, &templates)?
                .into_iter()
                .find(|candidate| candidate.key == group.key);
            if current_group.as_ref() != Some(group) {
                return Err(video::refusal(
                    "plan_changed",
                    "consolidation evidence changed during the run",
                ));
            }
            let (digest, slot, extra) = if index < allowance {
                (
                    batch.plan.digest.clone(),
                    original
                        .ok_or_else(|| video::refusal("plan_exceeded", "no careful allowance"))?,
                    false,
                )
            } else {
                (
                    batch
                        .extra
                        .as_ref()
                        .ok_or_else(|| {
                            video::refusal("plan_exceeded", "no additional careful allowance")
                        })?
                        .digest
                        .clone(),
                    0,
                    true,
                )
            };
            mutate(
                call,
                Mutation::Reserve {
                    phase: input.batch_id.clone(),
                    digest,
                    group: slot,
                    extra,
                },
            )?;
            let reply = call.ports.models.extract_planned(
                &ExtractRequest {
                    tier: ModelTier::Careful,
                    text: group.prompt(template)?,
                    schema: consolidation::schema(),
                    sensitivity: if private {
                        Sensitivity::Private
                    } else {
                        Sensitivity::Normal
                    },
                },
                &policy,
            )?;
            let checked = consolidation::checked_reply(group, reply.value)?;
            let markdown = consolidation::markdown(group, template, &checked.body)?;
            let payload = match consolidation::base_sha256(group) {
                Some(base) => {
                    json!({"kind":"knowledge_update","markdown":markdown,"base_sha256":base})
                }
                None => json!({"kind":"knowledge_add","markdown":markdown}),
            };
            let evidence = group
                .cites()
                .into_iter()
                .chain(group.sources.iter().cloned())
                .collect::<BTreeSet<_>>();
            let input = create::object(json!({"title":format!("Knowledge: {}", group.title),
                "description":format!("Consolidated {} sources for {} using the {} template.",group.sources.len(),group.title,template.template),
                "evidence":evidence,"payload":payload,"tags":[format!("bootstrap:{}",input.batch_id)]}))?;
            let handler = create::TOOL.handler().ok_or_else(|| {
                video::refusal(
                    "proposal_unavailable",
                    "office.proposal-create is unavailable",
                )
            })?;
            let output = handler(&Call {
                input: &input,
                ports: call.ports,
                context: call.context,
            })?;
            Ok((output, checked.conflicts))
        })();
        match result {
            Ok((output, found)) => {
                let page = group.page_id(&templates[&group.template]);
                let entry = json!({"subject":group.key,"page":page,"proposal":output["id"]});
                if output["suppressed"] == true {
                    suppressed.push(entry);
                } else {
                    proposed.push(entry.clone());
                    if group.existing.is_some() {
                        merged.push(entry);
                    }
                }
                if !found.is_empty() {
                    conflicts.push(json!({"subject":group.key,"claims":found}));
                }
            }
            Err(error) => {
                failed.push(json!({"subject":group.key,"code":error.code()}));
            }
        }
    }
    // Keep failed subjects visible for inspection; successful groups are no longer active.
    mutate(
        call,
        Mutation::Progress {
            phase: input.batch_id.clone(),
            consolidating: failed
                .iter()
                .filter_map(|v| v["subject"].as_str().map(str::to_owned))
                .collect(),
            overflow: deferred.clone(),
        },
    )?;
    Ok(
        json!({"batch_id":input.batch_id,"new_page_budget":budget,"proposed":proposed,"merged":merged,"below_bar":below_bar,"deferred":deferred,"conflicts":conflicts,"failed":failed,"suppressed":suppressed}),
    )
}
