/** Fixture-backed metrics and proposal titles for Insights. */
import type { Feature } from "../../http/types.js";
import { createOrbitClient, type OrbitToolClient } from "../../orbit/client.js";

function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function createInsightsFeature(client: Pick<OrbitToolClient, "pollTool">, now = () => new Date()): Feature {
  return {
    name: "insights", status: "built", routes: [{
      method: "GET", path: "/api/insights",
      async handle(_request, response) {
        try {
          const end = new Date(now());
          end.setUTCHours(0, 0, 0, 0);
          end.setUTCDate(end.getUTCDate() + 1);
          const start = new Date(end);
          start.setUTCDate(start.getUTCDate() - 84);
          const date = (value: Date) => value.toISOString().slice(0, 10);
          const raw = await client.pollTool("office.metrics", { since: date(start), until: date(end) });
          if (!object(raw) || raw.ok === false) throw new Error("metrics unavailable");
          const report = raw.ok === true ? raw.output : raw;
          if (!object(report) || !object(report.totals) || !Array.isArray(report.weeks) || !Array.isArray(report.totals.conversions)) throw new Error("invalid metrics");
          const conversions = [];
          for (const conversion of report.totals.conversions) {
            if (!object(conversion) || typeof conversion.cluster !== "string" || typeof conversion.proposal !== "string") throw new Error("invalid conversion");
            const proposal = await client.pollTool("orbit.task.show", { id: conversion.proposal, fields: ["title"] });
            if (!object(proposal) || typeof proposal.title !== "string" || !proposal.title.trim()) throw new Error("proposal unavailable");
            conversions.push({ name: conversion.cluster.replace(/[-_]+/g, " "), proposal_title: proposal.title });
          }
          response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
          response.end(JSON.stringify({ ...report, totals: { ...report.totals, conversions } }));
        } catch {
          response.writeHead(502, { "content-type": "application/json", "cache-control": "no-store" });
          response.end(JSON.stringify({ error: "insights_unavailable" }));
        }
      },
    }],
  };
}

export const feature: Feature = createInsightsFeature(createOrbitClient());
