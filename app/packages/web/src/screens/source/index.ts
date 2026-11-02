/**
 * Source: capture originals, review processing costs, and open citations at
 * their span. A selected source shows a back link, its title and a meta
 * line; the player with a timeline of its chapters and gotchas; the
 * transcript as speaker turns, each tagged with the marks that fall in it;
 * chapters and keyframes; and an aside with Cited by and Where it ran.
 * Everything comes from `GET /api/sources?id=…`; nothing is inferred.
 */
import type { BootstrapPhase, CapturedSource, SourcePlan, SourceView } from "@orbit-office/contracts";
import type { Screen } from "../../screen.js";
import { ApiError, capturePaste, getSource, getSourceCitation, markSourcePrivate, sourcePlan, uploadSource, getSelectionCount, putSelection, clearSelections } from "../../api/client.js";
import { sourceHref } from "../../api/source-location.js";
import { anchorTarget, clock, droppedFiles, parseTurn, seconds, sourceLength, sourceMarks, supportedFile, turnMarks, type Mark } from "./model.js";
import { styles } from "./style.js";
import { panel, panelBody, pill, sharedLabel, shareSwitch, type Tone } from "../../ui/index.js";

const el = <K extends keyof HTMLElementTagNameMap>(tag: K, text = ""): HTMLElementTagNameMap[K] => {
  const node = document.createElement(tag); node.textContent = text; return node;
};
const button = (label: string, action: () => void): HTMLButtonElement => {
  const node = el("button", label); node.type = "button"; node.addEventListener("click", action); return node;
};
function privateBox(): { label: HTMLLabelElement; input: HTMLInputElement } {
  const input = el("input"); input.type = "checkbox";
  const label = el("label"); label.append(input, document.createTextNode("Private — use only your chosen endpoint")); return { label, input };
}
const phaseNames = { docs: "Documents", kt: "Knowledge transfer", team: "Team", process: "Processes" };
function message(error: unknown): string {
  if (error instanceof ApiError && error.code === "upload_too_large") return "This file is too large. Choose a file under 256 MiB.";
  if (error instanceof ApiError && error.code === "source_refused") return "The Library refused this request. Check your private route and get a fresh preview before processing.";
  return "This request could not finish. Check the connection before trying again; it may already have been saved.";
}
async function render(root: HTMLElement): Promise<void> {
  const selected = new URLSearchParams(window.location.search).get("source");
  // A new anchor on the source already shown highlights and seeks in place.
  const shown = root.querySelector<HTMLElement>(":scope > .source-page");
  const reuse = shown ? drawn.get(shown) : undefined;
  if (selected && reuse && reuse.id === selected && SAME_SOURCE.test(window.location.hash)) { reuse.apply(); return; }
  const page = el("section"); page.className = "source-page";
  const header = el("header"); header.className = "source-head";
  const heading = el("h1", "Source");
  const meta = el("p", selected ? "" : "Keep the original beside the words, with every citation in place."); meta.className = "intro";
  if (selected) { const back = el("a", "Library · Sources"); back.href = "#library"; back.className = "source-back"; header.append(back); }
  header.append(heading, meta);
  page.append(el("style", styles), header); root.replaceChildren(page);
  const alive = (): boolean => page.isConnected;
  const capture = el("section"); capture.className = "source-drop";
  capture.append(el("h2", "Add a source"), el("p", "Drop a file or folder here, choose files, or paste text below."));
  const privacy = privateBox(); capture.append(privacy.label);
  const phase = el("select"); phase.setAttribute("aria-label", "Folder phase");
  for (const [key, title] of Object.entries(phaseNames)) { const option = el("option", title); option.value = key; phase.append(option); }
  const phaseLabel = el("label", "Organize folders as "); phaseLabel.append(phase); capture.append(phaseLabel);
  const files = el("input"); files.type = "file"; files.multiple = true; files.setAttribute("aria-label", "Choose files");
  const folder = el("input"); folder.type = "file"; folder.multiple = true; folder.setAttribute("webkitdirectory", ""); folder.setAttribute("aria-label", "Choose a folder");
  const actions = el("div"); actions.className = "source-actions"; const fileLabel = el("label", "Files"); fileLabel.append(files); const folderLabel = el("label", "Folder"); folderLabel.append(folder); actions.append(fileLabel, folderLabel); capture.append(actions);
  const paste = el("form"); paste.className = "source-paste";
  const title = el("input"); title.required = true; title.placeholder = "Source title"; title.setAttribute("aria-label", "Source title");
  const text = el("textarea"); text.required = true; text.placeholder = "Paste an email, notes, or a conversation"; text.setAttribute("aria-label", "Pasted text");
  const save = el("button", "Save pasted text"); save.type = "submit"; paste.append(title, text, save); capture.append(paste);
  const note = el("p"); note.className = "source-message"; note.setAttribute("role", "status"); capture.append(note);
  const result = el("div"); result.className = "source-result";
  // A selected source comes first; capturing another folds away below it.
  if (selected) { const disclosure = el("details"); disclosure.className = "source-more"; disclosure.append(el("summary", "Add another source"), capture); page.append(result, disclosure); } else page.append(capture, result);
  let busy = false;
  function locked(value: boolean): void { busy = value; for (const node of capture.querySelectorAll<HTMLInputElement | HTMLButtonElement | HTMLSelectElement | HTMLTextAreaElement>("input,button,select,textarea")) node.disabled = value; }
  function open(source: CapturedSource): void { window.location.assign(sourceHref(source.id)); }
  async function plan(target: { id: string } | { phase: BootstrapPhase }, privateSource: boolean): Promise<void> {
    result.replaceChildren(el("p", "Preparing a preview. No model processing has started."));
    try { const preview = await sourcePlan({ ...target, private: privateSource }); if (alive()) drawPlan(preview, target); }
    catch (error) { if (alive()) result.replaceChildren(el("p", message(error))); }
  }
  function drawPlan(preview: SourcePlan, target: { id: string } | { phase: BootstrapPhase }): void {
    const panel = el("section"); panel.className = "source-plan"; panel.setAttribute("aria-label", "Processing preview");
    panel.append(el("h2", preview.phase ? `${phaseNames[preview.phase]} preview` : "Video preview"));
    panel.append(el("p", `${preview.sources} ${preview.sources === 1 ? "source" : "sources"} · ${Math.ceil(preview.minutes)} minutes · ${preview.frames} frames · ${preview.calls} model calls`));
    panel.append(el("p", preview.estimate === null ? "Rates unset" : `Estimated cost: ${preview.estimate.amount.toFixed(2)} ${preview.estimate.currency}`));
    const privateControl = privateBox(); privateControl.input.checked = preview.private; privateControl.input.disabled = preview.private; panel.append(privateControl.label);
    panel.append(el("p", "Private means not sent to Meta's hosted API, sent only to the endpoint you chose."));
    const status = el("p"); status.setAttribute("role", "status");
    const process = button("Process", () => {
      process.disabled = true; privateControl.input.disabled = true; locked(true);
      void sourcePlan({ ...target, private: preview.private, confirm: true, digest: preview.digest }).then(() => {
        if (!alive()) return;
        if (preview.source) open({ id: preview.source, kind: "video", title: "" });
        else { status.textContent = "Ready for processing. Your Library setup will show progress."; process.textContent = "Confirmed"; }
      }).catch(error => { if (alive()) { status.textContent = message(error); panel.append(button("Get a fresh preview", () => { void plan(target, preview.private); })); } }).finally(() => { if (alive()) locked(false); });
    });
    privateControl.input.addEventListener("change", () => {
      process.disabled = true; privateControl.input.disabled = true;
      void plan(target, true);
    });
    panel.append(process, status); result.replaceChildren(panel);
  }
  async function upload(list: File[], isFolder: boolean): Promise<void> {
    if (busy) return;
    const supported = list.filter(file => supportedFile(file.name));
    if (!supported.length) { note.textContent = "No supported files were found."; return; }
    locked(true); result.replaceChildren();
    const captured: CapturedSource[] = [];
    try {
      for (const file of supported) {
        note.textContent = `Saving ${captured.length + 1} of ${supported.length}: ${file.name}`;
        const source = await uploadSource(file, isFolder ? phase.value as BootstrapPhase : undefined, privacy.input.checked);
        captured.push(source); if (!alive()) return;
      }
      note.textContent = `Saved ${captured.length} ${captured.length === 1 ? "source" : "sources"}.${supported.length < list.length ? " Unsupported files were left out." : ""}`;
      if (isFolder) await plan({ phase: phase.value as BootstrapPhase }, privacy.input.checked);
      else if (captured.length === 1 && captured[0]) {
        if (captured[0].kind === "video") await plan({ id: captured[0].id }, privacy.input.checked); else open(captured[0]);
      } else {
        for (const source of captured) { const link = el("a", source.title); link.href = sourceHref(source.id); const p = el("p"); p.append(link); result.append(p); }
      }
    } catch (error) { if (alive()) note.textContent = `${captured.length} saved. ${message(error)}`; }
    finally { if (alive()) { locked(false); files.value = ""; folder.value = ""; } }
  }
  files.addEventListener("change", () => { void upload(Array.from(files.files ?? []), false); });
  folder.addEventListener("change", () => { void upload(Array.from(folder.files ?? []), true); });
  capture.addEventListener("dragover", event => { event.preventDefault(); });
  capture.addEventListener("drop", event => {
    event.preventDefault(); if (busy || !event.dataTransfer) return;
    // Reserve the UI while recursively reading a directory as well as while uploading.
    locked(true);
    void droppedFiles(event.dataTransfer).then(data => { if (alive()) { locked(false); void upload(data.files, data.folder); } }).catch(error => { if (alive()) { locked(false); note.textContent = message(error); } });
  });
  paste.addEventListener("submit", event => {
    event.preventDefault(); if (busy || !title.value.trim() || !text.value.trim()) return;
    locked(true); note.textContent = "Saving pasted text.";
    void capturePaste({ title: title.value.trim(), text: text.value, private: privacy.input.checked }).then(source => { if (alive()) open(source); }).catch(error => { if (alive()) { note.textContent = message(error); locked(false); } });
  });
  if (!selected) return;
  result.append(el("p", "Loading source."));
  try { const source = await getSource(selected); if (alive()) { heading.textContent = source.title; meta.textContent = metaLine(source); drawn.set(page, { id: source.id, apply: drawSource(source, result) }); } }
  catch (error) { if (alive()) result.replaceChildren(el("p", message(error))); }
}

const KINDS: Record<string, string> = { meeting: "Meeting", video: "Video", audio: "Recording", image: "Image", doc: "Document", conversation: "Conversation" };

/** Kind, length and turn count, from the source itself. */
function metaLine(source: SourceView): string {
  const parts = [KINDS[source.kind] ?? source.kind.charAt(0).toUpperCase() + source.kind.slice(1)];
  const length = sourceLength(source);
  if (length > 0) parts.push(`${Math.max(1, Math.ceil(length / 60))} min`);
  const turns = source.text.split("\n").filter(line => parseTurn(line) !== null).length;
  if (turns > 0) parts.push(`${turns} ${turns === 1 ? "turn" : "turns"}`);
  if (source.sensitivity === "private") parts.push("Private");
  return parts.join(" · ");
}

const MARK_TONES: Record<Mark["kind"], Tone> = { chapter: "neutral", aside: "neutral", gotcha: "attention" };

/** A panel whose heading is the small caps label, as in the mock. */
function box(title: string): { section: HTMLElement; body: HTMLElement } {
  const section = panel(title, { variant: "plain" }); const body = panelBody(); section.append(body); return { section, body };
}

function drawSource(source: SourceView, root: HTMLElement): () => void {
  const section = el("section"); section.className = "source-view"; root.replaceChildren(section);
  const notice = el("div"); notice.className = "source-notice"; section.append(notice);
  const marks = sourceMarks(source);

  // The player: the original, with a timeline of the chapters and gotchas.
  const player = el("div"); player.className = "source-player source-original";
  const mime = source.media_type ?? ""; let media: HTMLMediaElement | undefined;
  if (mime.startsWith("video/") || mime.startsWith("audio/")) {
    media = mime.startsWith("video/") ? el("video") : el("audio"); media.controls = true; media.preload = "metadata";
    media.src = `/api/sources/media?id=${encodeURIComponent(source.id)}`; player.append(media);
    media.addEventListener("error", () => { player.append(el("p", "The original cannot be played here. It may have expired or exceed the current 512 KiB media limit.")); }, { once: true });
  } else if (mime.startsWith("image/")) {
    const image = el("img"); image.alt = source.title; image.src = `/api/sources/media?id=${encodeURIComponent(source.id)}`;
    image.addEventListener("error", () => { image.replaceWith(el("p", "The original image is unavailable or exceeds the current media limit.")); }); player.append(image);
  } else player.append(el("p", "The captured text is shown below. No playable original is available."));
  let length = sourceLength(source);
  if (length > 0 || media) {
    const timeline = el("div"); timeline.className = "source-timeline";
    const readout = el("span", `00:00:00 / ${clock(length)}`); readout.className = "mono-meta source-readout";
    const track = el("div"); track.className = "source-track"; track.setAttribute("role", "group"); track.setAttribute("aria-label", "Timeline");
    const rail = el("div"); rail.className = "source-rail"; const fill = el("div"); fill.className = "source-fill"; track.append(rail, fill);
    const markNodes = marks.map(mark => {
      const node = el("a"); node.className = "source-mark"; node.dataset.kind = mark.kind;
      node.href = sourceHref(source.id, mark.anchor); node.title = `${clock(mark.time)} · ${mark.label}`; node.setAttribute("aria-label", node.title);
      track.append(node); return { node, mark };
    });
    const place = (): void => {
      const total = Math.max(length, 1);
      for (const { node, mark } of markNodes) node.style.left = `${Math.min(100, (mark.time / total) * 100)}%`;
      const now = media?.currentTime ?? 0; fill.style.width = `${Math.min(100, (now / total) * 100)}%`;
      readout.textContent = `${clock(now)} / ${clock(length)}`;
    };
    place();
    if (media) {
      const current = media;
      current.addEventListener("loadedmetadata", () => { if (Number.isFinite(current.duration) && current.duration > length) length = current.duration; place(); });
      current.addEventListener("timeupdate", place);
    }
    timeline.append(readout, track); player.append(timeline);
  }
  section.append(player);

  const columns = el("div"); columns.className = "source-columns";
  const main = el("div"); main.className = "source-main";
  const aside = el("aside"); aside.className = "source-aside"; aside.setAttribute("aria-label", "About this source");
  columns.append(main, aside); section.append(columns);

  // Transcript: one row per line, a stamped line as a speaker turn tagged with its marks.
  const lineList = source.text.split("\n");
  const timed = lineList.some(line => parseTurn(line) !== null);
  const transcript = box(timed ? "Transcript" : "Text");
  const tray = el("div"); tray.className = "source-tray";
  const count = el("span", "Shared with agent: unavailable"); count.className = "pill"; count.setAttribute("role", "status");
  let generation = 0;
  async function refresh(): Promise<void> {
    const current = ++generation;
    try { const data = await getSelectionCount(); if (section.isConnected && current === generation) count.textContent = `Shared with agent: ${data.count}`; }
    catch { if (section.isConnected && current === generation) count.textContent = "Shared with agent: unavailable"; }
  }
  const clear = button("Clear", () => { clear.disabled = true; void clearSelections().then(refresh).catch(() => { count.textContent = "Shared with agent: unavailable"; }).finally(() => { clear.disabled = false; }); });
  tray.append(count, clear); transcript.section.querySelector(".panel-head")?.append(tray); void refresh();
  const timer = window.setInterval(() => { if (!section.isConnected) window.clearInterval(timer); else void refresh(); }, 5000);
  if (source.content_withheld) transcript.body.append(el("p", "This source is private, so its text is withheld here. It is only processed on your chosen private endpoint."));
  const lines = el("div"); lines.className = "source-text"; lines.setAttribute("aria-label", "Source text");
  const tagged = turnMarks(lineList, marks);
  const lineNodes = lineList.map((line, index) => {
    const node = el("div"); node.className = "source-line"; node.id = `L${index + 1}`;
    const turn = parseTurn(line);
    if (turn) {
      node.classList.add("source-turn");
      const link = el("a", turn.stamp); link.className = "source-stamp"; link.href = sourceHref(source.id, `#t=${turn.stamp}`); link.setAttribute("aria-label", `Open ${turn.stamp}`);
      const body = el("div"); body.className = "source-turn-body";
      if (turn.who) { const who = el("span", turn.who); who.className = "source-who"; body.append(who); }
      const said = el("span", turn.text); said.className = "source-said"; body.append(said);
      const tags = tagged.get(index) ?? [];
      if (tags.length) { const row = el("span"); row.className = "source-tags"; for (const mark of tags) row.append(pill(mark.tag, MARK_TONES[mark.kind])); body.append(row); }
      node.append(link, body);
    } else if (line.trim() === "") node.classList.add("source-blank");
    else {
      const link = el("a", String(index + 1)); link.className = "source-stamp source-lineno"; link.href = sourceHref(source.id, `#L${index + 1}`); link.setAttribute("aria-label", `Open line ${index + 1}`);
      const said = el("span", line); said.className = "source-said"; node.append(link, said);
    }
    lines.append(node); return node;
  });
  const shareSelection = (): void => {
    const selection = window.getSelection();
    if (!selection || selection.isCollapsed || !selection.rangeCount) return;
    const range = selection.getRangeAt(0);
    if (!lines.contains(range.startContainer) || !lines.contains(range.endContainer)) return;
    const lineFor = (node: Node): number => lineNodes.findIndex(line => line.contains(node));
    const start = lineFor(range.startContainer); const end = lineFor(range.endContainer); const quote = selection.toString().trim();
    if (start < 0 || end < 0 || !quote) return;
    const anchor = `#L${start + 1}${end === start ? "" : `-${end + 1}`}`;
    void putSelection({ object: source.id, anchor, quote, view: "source", sensitivity: source.sensitivity }).then(refresh).catch(() => { count.textContent = "Shared with agent: unavailable"; });
  };
  lines.addEventListener("mouseup", shareSelection);
  lines.addEventListener("keyup", event => { if (event.key.startsWith("Arrow") || event.key === "Home" || event.key === "End") shareSelection(); });
  transcript.body.append(lines); main.append(transcript.section);

  // Chapters, with tangents folded and their gotchas shown, then keyframes.
  const chapterNodes = new Map<number, HTMLElement>();
  const shownGotchas = new Set<SourceView["gotchas"][number]>();
  const chapters = box("Chapters");
  for (let index = 0; index < source.chapters.length; index++) {
    const chapter = source.chapters[index]; if (!chapter) continue;
    const item = el("div"); item.className = "source-chapter"; const content = el("div");
    const link = el("a", `${clock(chapter.start)} · ${chapter.title}`); link.href = sourceHref(source.id, `#chapter=${chapter.number}`); content.append(link);
    if (chapter.label === "tangent") { const fold = el("details"); const summary = el("summary", `Aside: ${chapter.title}`); fold.append(summary, content); item.append(fold); }
    else item.append(content);
    const end = source.chapters[index + 1]?.start ?? Infinity;
    for (const gotcha of source.gotchas) {
      if (gotcha.cites.some(cite => { const time = seconds(/#(?:t|frame)=(.+)$/.exec(cite)?.[1] ?? ""); return time !== null && time >= chapter.start && time < end; })) { const warning = el("p", `Gotcha: ${gotcha.text}`); warning.className = "source-gotcha"; item.append(warning); shownGotchas.add(gotcha); }
    }
    chapterNodes.set(chapter.number, item); chapters.body.append(item);
  }
  for (const gotcha of source.gotchas) if (!shownGotchas.has(gotcha)) { const warning = el("p", `Gotcha: ${gotcha.text}`); warning.className = "source-gotcha"; chapters.body.append(warning); }
  if (source.chapters.length || source.gotchas.length) main.append(chapters.section);
  const frameNodes: HTMLElement[] = [];
  if (source.frames.length) {
    const frames = box("Keyframes"); const strip = el("div"); strip.className = "source-frames";
    for (const frame of source.frames) {
      const item = el("figure"); item.className = "source-frame";
      const link = el("a"); link.href = sourceHref(source.id, frame.anchor);
      const image = el("img"); image.alt = frame.text; image.loading = "lazy"; image.src = `/api/sources/media?${new URLSearchParams({ id: source.id, frame: frame.file })}`;
      link.append(image, el("span", clock(frame.time))); item.append(link, el("figcaption", frame.text)); strip.append(item); frameNodes.push(item);
    }
    frames.body.append(strip); main.append(frames.section);
  }

  // The aside. A "Since this meeting" digest (`GET /api/sources/digest`) would go here, before Cited by:
  //   if (source.kind === "meeting") aside.append(digestPanel(source.id));
  aside.append(citedBy(source, section));
  aside.append(whereItRan(source, section));

  // Highlight, scroll to and seek the hash's span. A later anchor on the same
  // source reapplies this to the drawn view, so the player is not reloaded.
  const applyAnchor = (): void => {
    for (const node of section.querySelectorAll<HTMLElement>("[data-highlight]")) delete node.dataset.highlight;
    notice.replaceChildren();
    const target = anchorTarget(source, window.location.hash);
    if (target) {
      const highlighted: HTMLElement[] = target.lines.map(index => lineNodes[index]).filter((node): node is HTMLDivElement => node !== undefined);
      if (target.frame !== undefined && frameNodes[target.frame]) highlighted.push(frameNodes[target.frame]!);
      if (target.chapter !== undefined) { const chapter = chapterNodes.get(target.chapter); if (chapter) { const fold = chapter.querySelector("details"); if (fold) fold.open = true; highlighted.push(chapter); } }
      for (const node of highlighted) node.dataset.highlight = "true";
      highlighted.at(-1)?.scrollIntoView({ block: "center" });
      if (media && target.time !== null) {
        const current = media; const time = target.time;
        const seek = (): void => { if (current.isConnected) current.currentTime = time; };
        if (current.readyState >= 1) seek(); else current.addEventListener("loadedmetadata", seek, { once: true });
      }
    } else if (/^#(?:t=|L|frame=|chapter=)/.test(window.location.hash)) notice.append(el("p", "This citation does not match a span in the current source."));
  };
  applyAnchor();
  const onHash = (): void => { if (!section.isConnected) window.removeEventListener("hashchange", onHash); else if (SAME_SOURCE.test(window.location.hash)) applyAnchor(); };
  window.addEventListener("hashchange", onHash);
  return applyAnchor;
}

/** Hashes that stay on the drawn source: its own screen, or a span in it. */
const SAME_SOURCE = /^#(?:source$|t=|L\d|frame=|chapter=)/;

/** The drawn source view of each page, so a new anchor reuses it rather than redrawing. */
const drawn = new WeakMap<HTMLElement, { id: string; apply: () => void }>();

const CITER_KINDS: Record<string, string> = { task: "Agent task", page: "Library page", runbook: "Runbook", source: "Source" };

/** Cited by: the pages, tasks and sources that cite this one; a page or task opens its text here. */
function citedBy(source: SourceView, section: HTMLElement): HTMLElement {
  const cited = box("Cited by");
  if (!source.cited_by.length) cited.body.append(el("p", "No linked pages or tasks yet."));
  if (!source.task_citers_available) cited.body.append(el("p", "Task links are unavailable at the moment."));
  if (source.cited_by_truncated) cited.body.append(el("p", "More tasks cite this source than are shown here."));
  const detail = el("div"); detail.className = "source-citation";
  let citationGeneration = 0;
  function citationDetail(citation: SourceView["cited_by"][number]): void {
    const generation = ++citationGeneration;
    detail.replaceChildren(el("p", "Loading linked work."));
    void getSourceCitation(citation.id, citation.kind).then(data => { if (section.isConnected && generation === citationGeneration) { const text = el("p", data.text); text.className = "source-citation-text"; detail.replaceChildren(el("h3", data.title), text); } }).catch(error => { if (section.isConnected && generation === citationGeneration) detail.replaceChildren(el("p", message(error))); });
  }
  if (source.cited_by.length) {
    const list = el("ul"); list.className = "rows source-citers";
    for (const citation of source.cited_by) {
      const item = el("li"); item.className = "source-citer";
      const kind = el("span", CITER_KINDS[citation.kind] ?? citation.kind); kind.className = "source-citer-kind";
      const link = el("a", citation.title); link.href = citation.kind === "source" ? sourceHref(citation.id) : `?source=${encodeURIComponent(source.id)}&citation=${encodeURIComponent(citation.id)}#source`;
      if (citation.kind !== "source") link.addEventListener("click", event => {
        event.preventDefault(); window.history.replaceState(null, "", link.href); citationDetail(citation);
      });
      item.append(kind, link); list.append(item);
    }
    cited.body.append(list);
  }
  cited.body.append(detail);
  const requestedCitation = new URLSearchParams(window.location.search).get("citation");
  const requested = source.cited_by.find(citation => citation.id === requestedCitation && citation.kind !== "source");
  if (requested) citationDetail(requested);
  return cited.section;
}

/** Where it ran: the recorded model routes, the sensitivity, Mark private and team sharing. */
function whereItRan(source: SourceView, section: HTMLElement): HTMLElement {
  const where = box("Where it ran");
  const routes = el("div"); routes.className = "source-routes";
  for (const label of source.route_labels) routes.append(pill(label, label.startsWith("Private") ? "private" : "neutral"));
  if (source.route_labels.length) where.body.append(routes); else where.body.append(el("p", "No model processing recorded."));
  const sharedLine = el("p"); sharedLine.className = "source-sharing"; const sharing = el("div");
  // Marking a source private also stops sharing it, so both redraw from `source`.
  const drawSharing = (): void => {
    const label = sharedLabel(source.shared_by, source.shareable); sharedLine.replaceChildren(...(label ? [label] : []));
    if (source.shared_by === null) sharing.replaceChildren(shareSwitch({ id: source.id, noun: "this source", shareable: source.shareable, private: source.sensitivity === "private", onChange: (next) => { source.shareable = next; drawSharing(); } }));
  };
  drawSharing();
  if (source.sensitivity === "private") where.body.append(el("p", "Marked private. Every future call stays on your chosen endpoint."));
  else {
    const note = el("p", "Normal sensitivity. Mark the source private to keep every future call on your chosen endpoint.");
    const status = el("p"); const mark = button("Mark private", () => {
      mark.disabled = true;
      void markSourcePrivate(source.id).then(() => { if (section.isConnected) { source.sensitivity = "private"; source.shareable = false; mark.textContent = "Marked private"; note.textContent = "Marked private."; status.textContent = "Future processing uses only your chosen endpoint."; drawSharing(); } }).catch(error => { if (section.isConnected) { status.textContent = message(error); mark.disabled = false; } });
    }); where.body.append(note, mark, status);
  }
  where.body.append(sharedLine, sharing);
  return where.section;
}
export const screen: Screen = { id: "source", title: "Source", status: "built", render };
