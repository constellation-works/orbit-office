/** Capture originals, review processing costs, and open citations at their span. */
import type { BootstrapPhase, CapturedSource, SourcePlan, SourceView } from "@orbit-office/contracts";
import type { Screen } from "../../screen.js";
import { ApiError, capturePaste, getSource, getSourceCitation, markSourcePrivate, sourcePlan, uploadSource } from "../../api/client.js";
import { sourceHref } from "../../api/source-location.js";
import { anchorTarget, clock, droppedFiles, seconds, supportedFile } from "./model.js";
import { styles } from "./style.js";

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
  const page = el("section"); page.className = "source-page";
  const style = el("style", styles); const heading = el("h1", "Source");
  page.append(style, heading, el("p", "Keep the original beside the words, with every citation in place.")); root.replaceChildren(page);
  const alive = (): boolean => page.isConnected;
  const selected = new URLSearchParams(window.location.search).get("source");
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
  if (selected) { const disclosure = el("details"); disclosure.append(el("summary", "Add another source"), capture); page.append(disclosure); } else page.append(capture);
  const result = el("div"); page.append(result);
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
  try { const source = await getSource(selected); if (alive()) { heading.textContent = source.title; drawSource(source, result); } }
  catch (error) { if (alive()) result.replaceChildren(el("p", message(error))); }
}

function drawSource(source: SourceView, root: HTMLElement): void {
  const section = el("section"); root.replaceChildren(section);
  for (const label of source.route_labels) { const badge = el("span", label); badge.className = "pill"; section.append(badge); }
  if (!source.route_labels.length) section.append(el("p", "No model processing recorded."));
  if (source.sensitivity === "private") section.append(el("p", "Marked private"));
  else {
    const status = el("p"); const mark = button("Mark private", () => {
      mark.disabled = true;
      void markSourcePrivate(source.id).then(() => { if (section.isConnected) { mark.textContent = "Marked private"; status.textContent = "Future processing uses only your chosen endpoint."; } }).catch(error => { if (section.isConnected) { status.textContent = message(error); mark.disabled = false; } });
    }); section.append(mark, status);
  }
  const columns = el("div"); columns.className = "source-columns";
  const original = el("div"); original.className = "source-original"; original.append(el("h2", "Original"));
  const mime = source.media_type ?? ""; let player: HTMLMediaElement | undefined;
  if (mime.startsWith("video/") || mime.startsWith("audio/")) {
    player = mime.startsWith("video/") ? el("video") : el("audio"); player.controls = true; player.preload = "metadata";
    player.src = `/api/sources/media?id=${encodeURIComponent(source.id)}`; original.append(player);
    player.addEventListener("error", () => { original.append(el("p", "The original cannot be played here. It may have expired or exceed the current 512 KiB media limit.")); }, { once: true });
  } else if (mime.startsWith("image/")) {
    const image = el("img"); image.alt = source.title; image.src = `/api/sources/media?id=${encodeURIComponent(source.id)}`;
    image.addEventListener("error", () => { image.replaceWith(el("p", "The original image is unavailable or exceeds the current media limit.")); }); original.append(image);
  } else original.append(el("p", "The captured text is shown alongside. No playable original is available."));
  const transcript = el("div"); transcript.append(el("h2", "Text"));
  const lines = el("div"); lines.className = "source-text"; lines.setAttribute("aria-label", "Source text");
  const lineNodes = source.text.split("\n").map((line, index) => {
    const node = el("p"); node.className = "source-line"; node.id = `L${index + 1}`;
    const stamp = /^\[(\d{2,}:[0-5]\d:[0-5]\d)\]/.exec(line);
    const link = el("a", stamp?.[0] ?? String(index + 1)); link.href = sourceHref(source.id, stamp ? `#t=${stamp[1]}` : `#L${index + 1}`); link.setAttribute("aria-label", `Open ${stamp?.[1] ?? `line ${index + 1}`}`);
    node.append(link, document.createTextNode(stamp ? line.slice(stamp[0].length) : ` ${line}`)); lines.append(node); return node;
  });
  transcript.append(lines); columns.append(original, transcript); section.append(columns);
  const chapterNodes = new Map<number, HTMLElement>();
  const shownGotchas = new Set<SourceView["gotchas"][number]>();
  if (source.chapters.length) {
    section.append(el("h2", "Chapters"));
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
      chapterNodes.set(chapter.number, item); section.append(item);
    }
  }
  for (const gotcha of source.gotchas) if (!shownGotchas.has(gotcha)) { const warning = el("p", `Gotcha: ${gotcha.text}`); warning.className = "source-gotcha"; section.append(warning); }
  const frameNodes: HTMLElement[] = [];
  if (source.frames.length) {
    section.append(el("h2", "Keyframes")); const timeline = el("div"); timeline.className = "source-frames";
    for (const frame of source.frames) {
      const item = el("figure"); item.className = "source-frame";
      const link = el("a"); link.href = sourceHref(source.id, frame.anchor);
      const image = el("img"); image.alt = frame.text; image.loading = "lazy"; image.src = `/api/sources/media?${new URLSearchParams({ id: source.id, frame: frame.file })}`;
      link.append(image, el("span", clock(frame.time))); item.append(link, el("figcaption", frame.text)); timeline.append(item); frameNodes.push(item);
    } section.append(timeline);
  }
  section.append(el("h2", "Cited by"));
  if (!source.cited_by.length) section.append(el("p", "No linked pages or tasks yet."));
  if (!source.task_citers_available) section.append(el("p", "Task links are unavailable at the moment."));
  if (source.cited_by_truncated) section.append(el("p", "More tasks cite this source than are shown here."));
  const detail = el("section");
  let citationGeneration = 0;
  function citationDetail(citation: SourceView["cited_by"][number]): void {
    const generation = ++citationGeneration;
    detail.replaceChildren(el("p", "Loading linked work."));
    void getSourceCitation(citation.id, citation.kind).then(data => { if (section.isConnected && generation === citationGeneration) { const text = el("p", data.text); text.className = "source-line"; detail.replaceChildren(el("h3", data.title), text); } }).catch(error => { if (section.isConnected && generation === citationGeneration) detail.replaceChildren(el("p", message(error))); });
  }
  for (const citation of source.cited_by) {
    const link = el("a", citation.title); link.href = citation.kind === "source" ? sourceHref(citation.id) : `?source=${encodeURIComponent(source.id)}&citation=${encodeURIComponent(citation.id)}#source`;
    if (citation.kind !== "source") link.addEventListener("click", event => {
      event.preventDefault(); window.history.replaceState(null, "", link.href); citationDetail(citation);
    }); const p = el("p"); p.append(link); section.append(p);
  }
  section.append(detail);
  const requestedCitation = new URLSearchParams(window.location.search).get("citation");
  const cited = source.cited_by.find(citation => citation.id === requestedCitation && citation.kind !== "source");
  if (cited) citationDetail(cited);
  const target = anchorTarget(source, window.location.hash);
  if (target) {
    const highlighted: HTMLElement[] = target.lines.map(index => lineNodes[index]).filter((node): node is HTMLParagraphElement => node !== undefined);
    if (target.frame !== undefined && frameNodes[target.frame]) highlighted.push(frameNodes[target.frame]!);
    if (target.chapter !== undefined) { const chapter = chapterNodes.get(target.chapter); if (chapter) { const fold = chapter.querySelector("details"); if (fold) fold.open = true; highlighted.push(chapter); } }
    for (const node of highlighted) node.dataset.highlight = "true";
    highlighted.at(-1)?.scrollIntoView({ block: "center" });
    if (player && target.time !== null) {
      const media = player; const time = target.time;
      const seek = (): void => { if (media.isConnected) media.currentTime = time; };
      media.addEventListener("loadedmetadata", seek, { once: true });
      if (media.readyState >= 1) seek();
    }
  } else if (/^#(?:t=|L|frame=|chapter=)/.test(window.location.hash)) section.append(el("p", "This citation does not match a span in the current source."));
}
export const screen: Screen = { id: "source", title: "Source", status: "built", render };
