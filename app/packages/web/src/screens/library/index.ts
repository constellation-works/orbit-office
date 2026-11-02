/** Browse Library pages, outputs and setup progress. */
import type { Screen } from "../../screen.js";
import { getJson } from "../../api/client.js";
import { errorState, panel, panelBody, row as listRow, rows as listRows, sharedLabel, shareSwitch } from "../../ui/index.js";
import { styles } from "./style.js";

type Row = Record<string, unknown>;
const obj = (v: unknown): Row => v !== null && typeof v === "object" && !Array.isArray(v) ? v as Row : {};
const rows = (v: unknown): Row[] => Array.isArray(v) ? v.map(obj) : [];
const str = (v: unknown): string => typeof v === "string" ? v : "";
const el = (tag: string, text = ""): HTMLElement => { const node = document.createElement(tag); node.textContent = text; return node; };
const names: Record<string, string> = { user: "You", person: "People", client: "Clients", project: "Projects", topic: "Topics", glossary: "Glossary", systems: "Systems" };
const phaseNames: Record<string, string> = { docs: "Documents", kt: "Knowledge transfer", team: "Team", process: "Processes" };
const gapNames: Record<string, string> = { subject_without_page: "Needs a page", procedure_without_runbook: "Needs a how-to", owner_without_person: "Needs a person page", policy_without_current_as_of: "Needs a current policy date" };

async function render(root: HTMLElement): Promise<void> {
  const marker = el("span"); const style = el("style", styles); root.replaceChildren(marker, style, el("p", "Loading Library."));
  const alive = (): boolean => marker.isConnected;
  const request = async (params = ""): Promise<Row> => obj(await getJson<unknown>(`/api/library${params}`));
  /** A screen header (eyebrow, title, and the back button or search on the right) over the view. */
  function layout(title: string, intro = "", ...actions: HTMLElement[]): HTMLElement {
    const page = el("section"); page.className = "library";
    const header = el("header"); header.className = "screen-head";
    const titles = el("div"); const kicker = el("p"); kicker.className = "eyebrow library-kicker"; titles.append(kicker, el("h1", title)); if (intro) { const text = el("p", intro); text.className = "intro"; titles.append(text); }
    const right = el("div"); right.className = "screen-head-actions";
    const back = button("Library", () => { void overview(); }); back.className = "library-back";
    if (title !== "Library") right.append(back);
    right.append(...actions);
    header.append(titles); if (right.childNodes.length > 0) header.append(right);
    page.append(header);
    if (alive()) root.replaceChildren(marker, style, page);
    return page;
  }
  function button(label: string, action: () => void): HTMLButtonElement {
    const b = document.createElement("button"); b.type = "button"; b.textContent = label; b.addEventListener("click", action); return b;
  }
  function documentButton(id: string, title: string, className = "library-link"): HTMLButtonElement { const b = button(title || "Open document", () => { void documentView(id); }); b.className = className; return b; }
  /** A listed document with its sharing label: "Shared · from Alice" or "Shared". */
  function entry(row: Row, details: Node[] = []): HTMLLIElement {
    const label = sharedLabel(typeof row.shared_by === "string" ? row.shared_by : null, row.shareable === true);
    const item = listRow(documentButton(str(row.id), str(row.title)), { details, trail: label ? [label] : [] }); item.classList.add("library-entry");
    return item;
  }
  /** A bordered group with a small caps title and its rows. */
  function group(title: string, items: HTMLElement[], empty?: string): HTMLElement {
    const section = panel(title, { variant: "plain" }); section.classList.add("library-group");
    const body = panelBody(); body.classList.add("library-group-body");
    if (items.length) body.append(listRows(items, title)); else if (empty) body.append(el("p", empty));
    section.append(body); return section;
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
      for (const cite of (match[1] ?? "").split(/\s*[,;]\s*/)) parent.append(documentButton(cite, "View source", "library-cite"));
      start = (match.index ?? 0) + match[0].length;
    }
    parent.append(document.createTextNode(body.slice(start)));
  }
  /** One titled part of a document view, split from the one above by a hairline. */
  function part(parent: HTMLElement, title: string): HTMLElement {
    const section = el("div"); section.className = "library-part"; section.append(el("h2", title)); parent.append(section); return section;
  }
  function history(parent: HTMLElement, value: unknown): void {
    const h = obj(value); const section = part(parent, "History");
    if (!Array.isArray(h.revisions)) { section.append(el("p", "History is unavailable.")); return; }
    section.append(el("p", `${typeof h.total === "number" ? h.total : h.revisions.length} changes${h.truncated === true ? " (showing the latest changes)" : ""}`));
    if (h.revisions.length === 0) section.append(el("p", "No changes yet."));
    for (const rev of rows(h.revisions)) { const line = el("p", `${str(rev.time).slice(0, 10)} — ${str(rev.summary)}`); line.className = "library-revision"; section.append(line); }
  }
  async function documentView(citation: string): Promise<void> {
    const [id = "", anchor] = citation.split("#", 2);
    const page = layout("Loading document");
    try {
      const data = await request(`?view=document&id=${encodeURIComponent(id)}${anchor ? `&anchor=${encodeURIComponent(anchor)}` : ""}`); if (!alive() || !page.isConnected) return;
      const item = obj(data.page ?? data.artifact ?? data.source ?? data); const title = str(item.title) || "Document";
      page.querySelector("h1")!.textContent = title;
      const kicker = page.querySelector(".library-kicker"); if (kicker) kicker.textContent = data.page !== undefined ? (names[str(item.template)] ?? (str(item.id).startsWith("rb-") ? "How-to" : "Page")) : data.artifact !== undefined ? "Output" : data.source !== undefined ? "Source" : "";
      const article = el("article"); article.className = "panel library-document"; const doc = panelBody(); doc.classList.add("library-document-body"); article.append(doc); page.append(article);
      sharing(doc, id, data, item);
      if (str(item.summary)) { const summary = el("p", str(item.summary)); summary.className = "library-summary"; doc.append(summary); }
      const body = str(item.body ?? data.body); if (body) { const content = el("p"); content.className = "library-body"; citations(content, body); doc.append(content); }
      if (data.content_withheld === true) doc.append(el("p", "This document is private, so its text is withheld here. It is only processed on your chosen private endpoint."));
      const refs = rows(item.refs); if (refs.length) { const section = part(doc, "References"); for (const ref of refs) { const p = el("p", str(ref.label)); const value = str(ref.value); if (str(ref.target) === "url" && /^https?:\/\//i.test(value)) { const a = document.createElement("a"); a.href = value; a.textContent = " Open reference"; a.rel = "noreferrer"; p.append(a); } else if (str(ref.target) === "artifact" && value) { p.append(documentButton(value, "Open output", "library-cite")); } else if (value && ["path", "account"].includes(str(ref.target))) { p.append(document.createTextNode(`: ${value}`)); } section.append(p); } }
      const cites = Array.isArray(item.cites) ? item.cites : []; if (cites.length) { const section = part(doc, "Sources"); const line = el("div"); line.className = "library-inline"; for (const cite of cites) if (typeof cite === "string") line.append(documentButton(cite, "View source", "library-cite")); section.append(line); }
      const links = rows(data.backlinks); const linked = part(doc, "Linked from"); if (!links.length) linked.append(el("p", "No linked documents yet.")); else { const line = el("div"); line.className = "library-inline"; for (const link of links) line.append(documentButton(str(link.id), str(link.title))); linked.append(line); }
      if (item.template === "person") linked.append(el("p", "Task links are not available yet."));
      if (Array.isArray(data.series_history)) { const section = part(doc, "Earlier versions"); const line = el("div"); line.className = "library-inline"; for (const version of rows(data.series_history)) line.append(documentButton(str(version.id), `Version ${String(version.version ?? "")}`)); section.append(line); if (!data.series_history.length) section.append(el("p", "No earlier versions.")); }
      if (data.truncated === true) doc.append(el("p", "This document is shortened."));
      history(doc, data.history);
    } catch { if (alive() && page.isConnected) page.append(errorState("Document unavailable", "Try again in a moment.")); }
  }
  function outputList(parent: HTMLElement, data: Row): void {
    if (data.truncated === true) { const notice = el("p", "Showing the latest 100"); notice.className = "mono-meta"; parent.append(notice); }
    const list = rows(data.artifacts); if (!list.length) parent.append(el("p", "No outputs found."));
    const groups = new Map<string, Row[]>();
    for (const item of list) { const key = `${str(item.kind)} / ${str(item.series)}`; groups.set(key, [...(groups.get(key) ?? []), item]); }
    for (const [name, items] of groups) { parent.append(el("h3", name.replace(/[-_]/g, " ")), listRows(items.map((item) => entry(item)), name)); }
  }
  function searchForm(): HTMLFormElement {
    const form = document.createElement("form"); form.className = "library-search"; form.setAttribute("role", "search");
    const input = document.createElement("input"); input.type = "search"; input.required = true; input.placeholder = "Search the Library"; input.setAttribute("aria-label", "Search the Library");
    const submit = document.createElement("button"); submit.textContent = "Search"; submit.className = "secondary-action";
    form.append(input, submit); form.addEventListener("submit", (event) => { event.preventDefault(); if (input.value.trim()) void search(input.value.trim()); });
    return form;
  }
  async function overview(): Promise<void> {
    const page = layout("Library", "Pages, how-tos and outputs your office keeps, each cited to where it came from.", searchForm());
    const loading = el("p", "Loading Library."); page.append(loading);
    try {
      const data = await request(); if (!alive() || !page.isConnected) return;
      loading.remove();
      const columns = el("div"); columns.className = "library-layout";
      const aside = el("div"); aside.className = "library-aside"; aside.setAttribute("aria-label", "Pages");
      const main = el("div"); main.className = "library-main";
      columns.append(aside, main); page.append(columns);

      const setup = panel("Library setup"); setup.classList.add("library-setup"); const setupBody = panelBody(); setup.append(setupBody);
      if (obj(data.shared).configured === false) {
        const connect = el("section"); connect.className = "library-connect"; connect.append(el("h3", "Connect a shared library"), el("p", "Get the pages and how-tos your team shares, and share yours. Nothing leaves this computer until you switch a document to Share with the team."));
        const link = document.createElement("a"); link.href = "?settings=shared-library#settings"; link.textContent = "Connect a shared library"; link.className = "library-connect-link"; connect.append(link); setupBody.append(connect);
      }
      const phases: HTMLElement[] = [];
      for (const phase of rows(obj(data.setup).phases)) {
        const counts = el("span", ([["captured", "Captured"], ["digested", "Digested"], ["consolidating", "Being organized"], ["proposed", "Proposed"], ["accepted", "Accepted"], ["dismissed", "Dismissed"], ["overflow_waiting", "Waiting"]] as const).map(([key, label]) => `${label}: ${String(phase[key] ?? 0)}`).join(" · ")); counts.className = "mono-meta";
        const gaps = rows(phase.gaps).map((gap) => { const action = button("Ask the agent", () => { const message = `Please help with ${str(gap.subject)}: ${gapNames[str(gap.kind)] ?? "needs attention"}.`; void navigator.clipboard.writeText(message).then(() => { action.textContent = "Copied for your agent"; }).catch(() => { action.replaceWith(el("span", message)); }); }); action.className = "secondary-action"; return listRow(`${str(gap.subject)} — ${gapNames[str(gap.kind)] ?? "Needs attention"}.`, { dot: "attention", trail: [action] }); });
        phases.push(listRow(phaseNames[str(phase.phase)] ?? "Other", { details: gaps.length ? [counts, listRows(gaps, "Gaps")] : [counts] }));
      }
      if (phases.length) setupBody.append(listRows(phases, "Setup phases"));
      main.append(setup);

      const listing = obj(data.pages); const all = rows(listing.pages); const missing = rows(listing.missing_templates);
      const templates = [...new Set([...Object.keys(names), ...all.filter((p) => p.template !== "runbook" && !str(p.id).startsWith("rb-")).map((p) => str(p.template) || "other"), ...missing.map((p) => str(p.template) || "other")])];
      const about = el("h2", "About"); about.className = "eyebrow library-heading"; aside.append(about);
      for (const template of templates) { const items = all.filter((p) => (str(p.template) || "other") === template && !str(p.id).startsWith("rb-")); const absent = missing.filter((p) => (str(p.template) || "other") === template); if (!items.length && !absent.length) continue; aside.append(group(names[template] ?? template.replace(/[-_]/g, " "), [...items.map((item) => entry(item)), ...absent.map((item) => listRow(`${str(item.title)} — Not set up yet`, { dot: "neutral" }))])); }
      const how = all.filter((p) => str(p.id).startsWith("rb-")); aside.append(group("How-to", how.map((item) => entry(item)), "No how-to pages yet."));

      const outputs = panel("Outputs"); outputs.classList.add("library-outputs"); const outputsBody = panelBody(); outputs.append(outputsBody);
      const filters = document.createElement("form"); filters.className = "library-filters"; const kind = document.createElement("select"); kind.setAttribute("aria-label", "Output kind"); for (const value of ["", "report", "doc", "draft", "script", "data", "review", "other"]) { const option = document.createElement("option"); option.value = value; option.textContent = value || "All kinds"; kind.append(option); } const series = document.createElement("input"); series.placeholder = "Exact series name"; series.setAttribute("aria-label", "Series name"); const apply = document.createElement("button"); apply.textContent = "Show outputs"; apply.className = "secondary-action"; filters.append(kind, series, apply); outputsBody.append(filters); const results = el("div"); results.className = "library-results"; outputList(results, obj(data.artifacts)); outputsBody.append(results); filters.addEventListener("submit", (event) => { event.preventDefault(); const params = new URLSearchParams({ view: "outputs" }); if (kind.value) params.set("kind", kind.value); if (series.value.trim()) params.set("series", series.value.trim()); void request(`?${params}`).then((list) => { if (results.isConnected) { results.replaceChildren(); outputList(results, list); } }).catch(() => { if (results.isConnected) results.replaceChildren(errorState("Outputs unavailable", "Try again in a moment.")); }); });
      main.append(outputs);
    } catch { if (alive() && page.isConnected) { loading.remove(); page.append(errorState("Library unavailable", "Try again in a moment.")); } }
  }
  async function search(query: string): Promise<void> {
    const page = layout("Search results");
    try {
      const data = await request(`?view=search&query=${encodeURIComponent(query)}`); if (!alive() || !page.isConnected) return; const hits = rows(data.hits);
      const results = panel(`${hits.length} ${hits.length === 1 ? "result" : "results"}`, { variant: "plain" }); results.classList.add("library-group"); const body = panelBody(); results.append(body); page.append(results);
      if (!hits.length) body.append(el("p", "No results found."));
      else body.append(listRows(hits.map((hit) => { const snippet = el("p", hit.content_withheld === true ? "Private — snippet withheld." : str(hit.snippet)); snippet.className = "library-snippet"; return entry(hit, [snippet]); }), "Search results"));
    } catch { if (alive() && page.isConnected) page.append(errorState("Search unavailable", "Try again in a moment.")); }
  }
  await overview();
}
export const screen: Screen = { id: "library", title: "Library", status: "built", render };
