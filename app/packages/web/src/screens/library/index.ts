/** Browse Library pages, outputs and setup progress. */
import type { Screen } from "../../screen.js";
import { getJson } from "../../api/client.js";
import { errorState, sharedLabel, shareSwitch } from "../../ui/index.js";

type Row = Record<string, unknown>;
const obj = (v: unknown): Row => v !== null && typeof v === "object" && !Array.isArray(v) ? v as Row : {};
const rows = (v: unknown): Row[] => Array.isArray(v) ? v.map(obj) : [];
const str = (v: unknown): string => typeof v === "string" ? v : "";
const el = (tag: string, text = ""): HTMLElement => { const node = document.createElement(tag); node.textContent = text; return node; };
const names: Record<string, string> = { user: "You", person: "People", client: "Clients", project: "Projects", topic: "Topics", glossary: "Glossary", systems: "Systems" };
const phaseNames: Record<string, string> = { docs: "Documents", kt: "Knowledge transfer", team: "Team", process: "Processes" };
const gapNames: Record<string, string> = { subject_without_page: "Needs a page", procedure_without_runbook: "Needs a how-to", owner_without_person: "Needs a person page", policy_without_current_as_of: "Needs a current policy date" };

async function render(root: HTMLElement): Promise<void> {
  const marker = el("span"); root.replaceChildren(marker, el("p", "Loading Library."));
  const alive = (): boolean => marker.isConnected;
  const request = async (params = ""): Promise<Row> => obj(await getJson<unknown>(`/api/library${params}`));
  function layout(title: string): HTMLElement {
    const page = el("section"); page.className = "library";
    const heading = el("h1", title); page.append(heading);
    const back = document.createElement("button"); back.type = "button"; back.textContent = "Library"; back.addEventListener("click", () => { void overview(); });
    if (title !== "Library") page.append(back);
    if (alive()) root.replaceChildren(marker, page);
    return page;
  }
  function button(label: string, action: () => void): HTMLButtonElement {
    const b = document.createElement("button"); b.type = "button"; b.textContent = label; b.addEventListener("click", action); return b;
  }
  function documentButton(id: string, title: string): HTMLButtonElement { return button(title || "Open document", () => { void documentView(id); }); }
  /** A listed document with its sharing label: "Shared · from Alice" or "Shared". */
  function entry(row: Row): HTMLElement {
    const item = el("span"); item.className = "library-entry"; item.append(documentButton(str(row.id), str(row.title)));
    const label = sharedLabel(typeof row.shared_by === "string" ? row.shared_by : null, row.shareable === true); if (label) item.append(label);
    return item;
  }
  /** The label and **Share with the team** switch of a document view; a colleague's document only carries the label. */
  function sharing(page: HTMLElement, id: string, data: Row, item: Row): void {
    const sharedBy = typeof (data.shared_by ?? item.shared_by) === "string" ? str(data.shared_by ?? item.shared_by) : null;
    let shareable = item.shareable === true || data.shareable === true;
    const line = el("p"); line.className = "library-sharing";
    const draw = (): void => { const label = sharedLabel(sharedBy, shareable); line.replaceChildren(...(label ? [label] : [])); };
    draw(); page.append(line);
    if (sharedBy !== null) return;
    const noun = data.page !== undefined ? "this page" : data.artifact !== undefined ? "this output" : "this source";
    page.append(shareSwitch({ id, noun, shareable, private: (item.sensitivity ?? data.sensitivity) === "private", onChange: (next) => { shareable = next; draw(); } }));
  }
  function citations(parent: HTMLElement, body: string): void {
    const pattern = /\[((?:src|dom|rb|art)-[^\]\s,;]+(?:\s*[,;]\s*(?:src|dom|rb|art)-[^\]\s,;]+)*)\]/g;
    let start = 0;
    for (const match of body.matchAll(pattern)) {
      parent.append(document.createTextNode(body.slice(start, match.index)));
      for (const cite of (match[1] ?? "").split(/\s*[,;]\s*/)) parent.append(documentButton(cite, "View source"));
      start = (match.index ?? 0) + match[0].length;
    }
    parent.append(document.createTextNode(body.slice(start)));
  }
  function history(page: HTMLElement, value: unknown): void {
    const h = obj(value); page.append(el("h2", "History"));
    if (!Array.isArray(h.revisions)) { page.append(el("p", "History is unavailable.")); return; }
    page.append(el("p", `${typeof h.total === "number" ? h.total : h.revisions.length} changes${h.truncated === true ? " (showing the latest changes)" : ""}`));
    if (h.revisions.length === 0) page.append(el("p", "No changes yet."));
    for (const rev of rows(h.revisions)) page.append(el("p", `${str(rev.time).slice(0, 10)} — ${str(rev.summary)}`));
  }
  async function documentView(citation: string): Promise<void> {
    const [id = "", anchor] = citation.split("#", 2);
    const page = layout("Loading document");
    try {
      const data = await request(`?view=document&id=${encodeURIComponent(id)}${anchor ? `&anchor=${encodeURIComponent(anchor)}` : ""}`); if (!alive() || !page.isConnected) return;
      const item = obj(data.page ?? data.artifact ?? data.source ?? data); const title = str(item.title) || "Document";
      page.querySelector("h1")!.textContent = title;
      sharing(page, id, data, item);
      if (str(item.summary)) page.append(el("p", str(item.summary)));
      const body = str(item.body ?? data.body); if (body) { const content = el("p"); content.className = "library-body"; citations(content, body); page.append(content); }
      if (data.content_withheld === true) page.append(el("p", "This document is private, so its text is withheld here. It is only processed on your chosen private endpoint."));
      const refs = rows(item.refs); if (refs.length) { page.append(el("h2", "References")); for (const ref of refs) { const p = el("p", str(ref.label)); const value = str(ref.value); if (str(ref.target) === "url" && /^https?:\/\//i.test(value)) { const a = document.createElement("a"); a.href = value; a.textContent = " Open reference"; a.rel = "noreferrer"; p.append(a); } else if (str(ref.target) === "artifact" && value) { p.append(documentButton(value, "Open output")); } else if (value && ["path", "account"].includes(str(ref.target))) { p.append(document.createTextNode(`: ${value}`)); } page.append(p); } }
      const cites = Array.isArray(item.cites) ? item.cites : []; if (cites.length) { page.append(el("h2", "Sources")); for (const cite of cites) if (typeof cite === "string") page.append(documentButton(cite, "View source")); }
      const links = rows(data.backlinks); page.append(el("h2", "Linked from")); if (!links.length) page.append(el("p", "No linked documents yet.")); for (const link of links) page.append(documentButton(str(link.id), str(link.title)));
      if (item.template === "person") page.append(el("p", "Task links are not available yet."));
      if (Array.isArray(data.series_history)) { page.append(el("h2", "Earlier versions")); for (const version of rows(data.series_history)) page.append(documentButton(str(version.id), `Version ${String(version.version ?? "")}`)); if (!data.series_history.length) page.append(el("p", "No earlier versions.")); }
      if (data.truncated === true) page.append(el("p", "This document is shortened."));
      history(page, data.history);
    } catch { if (alive() && page.isConnected) page.append(errorState("Document unavailable", "Try again in a moment.")); }
  }
  function outputList(parent: HTMLElement, data: Row): void {
    if (data.truncated === true) parent.append(el("p", "Showing the latest 100"));
    const list = rows(data.artifacts); if (!list.length) parent.append(el("p", "No outputs found."));
    const groups = new Map<string, Row[]>();
    for (const item of list) { const key = `${str(item.kind)} / ${str(item.series)}`; groups.set(key, [...(groups.get(key) ?? []), item]); }
    for (const [name, items] of groups) { parent.append(el("h3", name.replace(/[-_]/g, " "))); for (const item of items) parent.append(entry(item)); }
  }
  async function overview(): Promise<void> {
    const page = layout("Library"); page.append(el("p", "Loading Library."));
    try {
      const data = await request(); if (!alive() || !page.isConnected) return;
      page.replaceChildren(el("h1", "Library"));
      const form = document.createElement("form"); const input = document.createElement("input"); input.type = "search"; input.required = true; input.placeholder = "Search the Library"; input.setAttribute("aria-label", "Search the Library"); const submit = document.createElement("button"); submit.textContent = "Search"; form.append(input, submit); form.addEventListener("submit", (event) => { event.preventDefault(); if (input.value.trim()) void search(input.value.trim()); }); page.append(form);
      page.append(el("h2", "Library setup"));
      if (obj(data.shared).configured === false) {
        const connect = el("section"); connect.className = "library-connect"; connect.append(el("h3", "Connect a shared library"), el("p", "Get the pages and how-tos your team shares, and share yours. Nothing leaves this computer until you switch a document to Share with the team."));
        const link = document.createElement("a"); link.href = "?settings=shared-library#settings"; link.textContent = "Connect a shared library"; link.className = "library-connect-link"; connect.append(link); page.append(connect);
      }
      for (const phase of rows(obj(data.setup).phases)) {
        const section = el("section"); section.append(el("h3", phaseNames[str(phase.phase)] ?? "Other"));
        for (const [key, label] of [["captured", "Captured"], ["digested", "Digested"], ["consolidating", "Being organized"], ["proposed", "Proposed"], ["accepted", "Accepted"], ["dismissed", "Dismissed"], ["overflow_waiting", "Waiting"]] as const) section.append(el("p", `${label}: ${String(phase[key] ?? 0)}`));
        for (const gap of rows(phase.gaps)) { const row = el("p", `${str(gap.subject)} — ${gapNames[str(gap.kind)] ?? "Needs attention"}. `); const action = button("Ask the agent", () => { const message = `Please help with ${str(gap.subject)}: ${gapNames[str(gap.kind)] ?? "needs attention"}.`; void navigator.clipboard.writeText(message).then(() => { action.textContent = "Copied for your agent"; }).catch(() => { action.replaceWith(el("span", message)); }); }); row.append(action); section.append(row); } page.append(section);
      }
      const listing = obj(data.pages); const all = rows(listing.pages); const missing = rows(listing.missing_templates);
      page.append(el("h2", "About"));
      const templates = [...new Set([...Object.keys(names), ...all.filter((p) => p.template !== "runbook" && !str(p.id).startsWith("rb-")).map((p) => str(p.template) || "other"), ...missing.map((p) => str(p.template) || "other")])];
      for (const template of templates) { const items = all.filter((p) => (str(p.template) || "other") === template && !str(p.id).startsWith("rb-")); const absent = missing.filter((p) => (str(p.template) || "other") === template); if (!items.length && !absent.length) continue; page.append(el("h3", names[template] ?? template.replace(/[-_]/g, " "))); for (const item of items) page.append(entry(item)); for (const item of absent) page.append(el("p", `${str(item.title)} — Not set up yet`)); }
      page.append(el("h2", "How-to")); const how = all.filter((p) => str(p.id).startsWith("rb-")); if (!how.length) page.append(el("p", "No how-to pages yet.")); for (const item of how) page.append(entry(item));
      page.append(el("h2", "Outputs")); const filters = document.createElement("form"); const kind = document.createElement("select"); kind.setAttribute("aria-label", "Output kind"); for (const value of ["", "report", "doc", "draft", "script", "data", "review", "other"]) { const option = document.createElement("option"); option.value = value; option.textContent = value || "All kinds"; kind.append(option); } const series = document.createElement("input"); series.placeholder = "Exact series name"; series.setAttribute("aria-label", "Series name"); const apply = document.createElement("button"); apply.textContent = "Show outputs"; filters.append(kind, series, apply); page.append(filters); const results = el("div"); outputList(results, obj(data.artifacts)); page.append(results); filters.addEventListener("submit", (event) => { event.preventDefault(); const params = new URLSearchParams({ view: "outputs" }); if (kind.value) params.set("kind", kind.value); if (series.value.trim()) params.set("series", series.value.trim()); void request(`?${params}`).then((list) => { if (results.isConnected) { results.replaceChildren(); outputList(results, list); } }).catch(() => { if (results.isConnected) results.replaceChildren(errorState("Outputs unavailable", "Try again in a moment.")); }); });
    } catch { if (alive() && page.isConnected) page.append(errorState("Library unavailable", "Try again in a moment.")); }
  }
  async function search(query: string): Promise<void> {
    const page = layout("Search results");
    try { const data = await request(`?view=search&query=${encodeURIComponent(query)}`); if (!alive() || !page.isConnected) return; const hits = rows(data.hits); if (!hits.length) page.append(el("p", "No results found.")); for (const hit of hits) { const row = el("div"); row.append(entry(hit), el("p", hit.content_withheld === true ? "Private — snippet withheld." : str(hit.snippet))); page.append(row); } } catch { if (alive() && page.isConnected) page.append(errorState("Search unavailable", "Try again in a moment.")); }
  }
  await overview();
}
export const screen: Screen = { id: "library", title: "Library", status: "built", render };
