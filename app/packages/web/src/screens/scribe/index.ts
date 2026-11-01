/**
 * Scribe: a compact window over scribe v1's SCR files (specs/scribe.md;
 * specs/web-app.md, "Screens"). The feed of entries, newest first, with the
 * most recent session first; the canned Pending, Preferences, Landed and
 * Why views; the read-only Handoffs view (Current, Global, History); Daniel's
 * own note; and an ask box with a cited answer. Polls;
 * v1 is commanded only, so there is no capture switch or heartbeat.
 */
import type {
  ScribeAnswer, ScribeClaim, ScribeEntry, ScribeFeed, ScribeNoteResult, ScribeQueryResult, ScribeSession, ScribeView,
} from "@orbit-office/contracts";

import { ApiError, getJson, postJson } from "../../api/client.js";
import type { Screen } from "../../screen.js";
import { pill } from "../../ui/index.js";
import {
  emptyView, leadingTag, messageFor, NOTE_TAGS, notCapturedText, sessionMeta, sessionTitle, suggestedTag, timeLabel, VIEW_TITLES,
} from "./model.js";
import { el, resumeRow } from "./dom.js";
import { createHandoffsView } from "./handoffs.js";
import { styles } from "./style.js";

/** How often the feed and the open view refresh. */
const POLL_MS = 10_000;

type Tab = "feed" | ScribeView | "handoffs";
const TABS: readonly Tab[] = ["feed", "pending", "pref", "landed", "why", "handoffs"];
const tabTitle = (tab: Tab): string => tab === "feed" ? "Feed" : tab === "handoffs" ? "Handoffs" : VIEW_TITLES[tab];

const codeOf = (error: unknown): string => error instanceof ApiError ? error.code : "unknown";

/** The citation anchor: `SCR file:line`, the message id, and a copyable resume command. */
function citation(fileLine: string | null, msg: string | null, resume: string | null): HTMLElement {
  const row = el("div", "", "scribe-cite");
  if (fileLine) { const code = el("code", fileLine); code.title = "SCR file:line"; row.append(code); }
  if (msg) row.append(el("span", msg));
  if (resume) row.append(...resumeRow(resume));
  return row;
}

function entryItem(entry: ScribeEntry): HTMLLIElement {
  const item = el("li", "", "scribe-entry");
  item.dataset.citation = entry.citation;
  const head = el("div", "", "scribe-entry-head");
  const time = el("time", timeLabel(entry.time));
  time.dateTime = entry.time;
  head.append(time);
  for (const tag of [entry.tag, ...entry.topics]) { const chip = pill(`#${tag}`); chip.classList.add("scribe-chip"); head.append(chip); }
  if (entry.stale) head.append(pill(`Stale · review by ${entry.review_by ?? "unset"}`, "attention"));
  item.append(head, el("blockquote", entry.quote, "scribe-quote"));
  const decision = entry.tag === "decision" || entry.tag === "agent-decision";
  const fields: Array<[string, string | null]> = [
    ["why", entry.why ?? (decision ? "no recorded reason" : null)],
    ["matter", entry.matter],
    ["review by", entry.stale ? null : entry.review_by],
    ["refs", entry.refs.length ? entry.refs.join(", ") : null],
    ["due", entry.due],
    ["by", entry.by],
  ];
  const shown = fields.filter((field): field is [string, string] => field[1] !== null);
  if (shown.length) {
    const list = el("dl", "", "scribe-fields");
    for (const [name, value] of shown) list.append(el("dt", name), el("dd", value));
    item.append(list);
  }
  item.append(citation(entry.file_line, entry.msg, entry.resume));
  return item;
}

function sessionBlock(session: ScribeSession, first: boolean): HTMLElement {
  const block = el("section", "", "scribe-session");
  block.setAttribute("aria-label", sessionTitle(session));
  const header = el("header");
  const title = el("h3", sessionTitle(session));
  header.append(title);
  const meta = el("p", sessionMeta(session), "scribe-meta");
  if (first) meta.prepend(pill("Most recent"), document.createTextNode(" "));
  header.append(meta);
  const list = el("ul", "", "scribe-list");
  for (const entry of session.entries) list.append(entryItem(entry));
  block.append(header, list);
  return block;
}

function claimItem(claim: ScribeClaim): HTMLElement {
  const item = el("li", "", "scribe-claim");
  if (claim.withheld) item.append(el("p", "A private line answers this; it is cited by reference only."));
  else {
    if (claim.text) item.append(el("p", claim.text));
    if (claim.quote) item.append(el("blockquote", claim.quote, "scribe-quote"));
  }
  item.append(citation(claim.file_line, claim.msg, claim.resume));
  return item;
}

function drawAnswer(target: HTMLElement, answer: ScribeAnswer): void {
  const parts: HTMLElement[] = [];
  if (answer.status === "answered") {
    const list = el("ul", "", "scribe-list");
    for (const claim of answer.claims) list.append(claimItem(claim));
    parts.push(list);
  } else if (answer.status === "reference_only") {
    parts.push(el("p", "Only private lines match. Without a private model route they are cited by reference only:", "scribe-status"));
    const list = el("ul", "", "scribe-list");
    for (const reference of answer.references) { const li = el("li", "", "scribe-cite"); li.append(el("code", reference)); list.append(li); }
    parts.push(list);
  } else parts.push(el("p", notCapturedText(answer), "scribe-status"));
  const notes: string[] = [];
  if (answer.dropped > 0) notes.push(`${answer.dropped} ${answer.dropped === 1 ? "quote was" : "quotes were"} dropped: not found on the cited line.`);
  if (answer.model) notes.push(`Answered by ${answer.model}.`);
  if (notes.length) parts.push(el("p", notes.join(" "), "scribe-stated"));
  target.replaceChildren(...parts);
}

async function render(root: HTMLElement): Promise<void> {
  const page = el("section", "", "scribe-page");
  page.append(el("style", styles), el("h1", "Scribe"),
    el("p", "What the scribe wrote, newest first. Run /scribe in a session to add more; nothing is captured automatically.", "scribe-lede"));
  root.replaceChildren(page);
  const alive = (): boolean => page.isConnected;

  // Ask box.
  const askForm = el("form", "", "scribe-panel");
  askForm.setAttribute("aria-label", "Ask the scribe");
  const question = el("input");
  question.placeholder = "Did Daniel decide on the release?";
  question.setAttribute("aria-label", "Question");
  question.maxLength = 4096;
  const askButton = el("button", "Ask");
  askButton.type = "submit";
  const askRow = el("div", "", "scribe-row");
  askRow.append(question, askButton);
  const answerBox = el("div", "", "scribe-answer");
  answerBox.setAttribute("aria-live", "polite");
  askForm.append(el("h2", "Ask"), askRow, answerBox);
  askForm.addEventListener("submit", (event) => {
    event.preventDefault();
    if (!question.value.trim()) { answerBox.replaceChildren(el("p", messageFor("question_required"), "scribe-status")); return; }
    askButton.disabled = true;
    answerBox.replaceChildren(el("p", "Searching the scribe, then asking the model about the shortlist.", "scribe-status"));
    void postJson<ScribeAnswer>("/api/scribe/ask", { question: question.value.trim() })
      .then((answer) => { if (alive()) drawAnswer(answerBox, answer); })
      .catch((error) => { if (alive()) answerBox.replaceChildren(el("p", messageFor(codeOf(error)), "scribe-status")); })
      .finally(() => { askButton.disabled = false; });
  });

  // Add note.
  const noteForm = el("form", "", "scribe-panel scribe-note");
  noteForm.setAttribute("aria-label", "Add a note");
  const sessionPick = el("select");
  const sessionLabel = el("label", "Session");
  sessionLabel.append(sessionPick);
  const noteText = el("textarea");
  noteText.placeholder = "#todo call the venue at 3pm";
  const noteLabel = el("label", "Note, recorded verbatim");
  noteLabel.append(noteText);
  const tagPick = el("select");
  for (const tag of NOTE_TAGS) { const option = el("option", `#${tag}`); option.value = tag; tagPick.append(option); }
  const tagCaption = el("span", "Tag");
  const tagLabel = el("label");
  tagLabel.append(tagCaption, tagPick);
  let tagChosen = false;
  const syncTag = (): void => {
    const leading = leadingTag(noteText.value);
    tagPick.disabled = leading !== null;
    tagCaption.textContent = leading !== null ? `Tag: #${leading}, from the text` : "Tag";
    if (!tagChosen && leading === null) tagPick.value = suggestedTag(noteText.value);
  };
  noteText.addEventListener("input", syncTag);
  tagPick.addEventListener("change", () => { tagChosen = true; });
  const noteButton = el("button", "Add note");
  noteButton.type = "submit";
  const noteStatus = el("p", "", "scribe-status");
  noteStatus.setAttribute("role", "status");
  noteForm.append(el("h2", "Add a note"), sessionLabel, noteLabel, tagLabel, noteButton, noteStatus);
  noteForm.addEventListener("submit", (event) => {
    event.preventDefault();
    const text = noteText.value.trim();
    if (!text) { noteStatus.textContent = messageFor("note_text_required"); return; }
    if (!sessionPick.value) { noteStatus.textContent = messageFor("no_cursor"); return; }
    noteButton.disabled = true;
    noteStatus.textContent = "Recording the note.";
    const body = leadingTag(text) !== null ? { session_id: sessionPick.value, text } : { session_id: sessionPick.value, text, tag: tagPick.value };
    void postJson<ScribeNoteResult>("/api/scribe/note", body)
      .then((result) => {
        if (!alive()) return;
        noteText.value = ""; tagChosen = false; syncTag();
        noteStatus.textContent = `Recorded at ${result.added.map((line) => line.file_line).join(", ")}. The feed shows it on its next refresh.`;
      })
      .catch((error) => { if (alive()) noteStatus.textContent = messageFor(codeOf(error)); })
      .finally(() => { noteButton.disabled = sessionPick.options.length === 0; });
  });

  // Tabs over the feed and the canned views.
  const tabs = el("div", "", "scribe-tabs");
  tabs.setAttribute("role", "tablist");
  tabs.setAttribute("aria-label", "Scribe views");
  const about = el("input");
  about.placeholder = "Matter or topic (optional)";
  about.setAttribute("aria-label", "Matter or topic");
  const aboutButton = el("button", "Show");
  aboutButton.type = "submit";
  const aboutForm = el("form", "", "scribe-about");
  aboutForm.append(about, aboutButton);
  const stated = el("p", "", "scribe-stated");
  const panel = el("div", "", "scribe-results");
  panel.setAttribute("role", "tabpanel");
  const handoffs = createHandoffsView(POLL_MS);
  // Today links here with `?scribe=handoffs`.
  let tab: Tab = new URLSearchParams(window.location.search).get("scribe") === "handoffs" ? "handoffs" : "feed";
  const buttons = new Map<Tab, HTMLButtonElement>();
  for (const name of TABS) {
    const button = el("button", tabTitle(name));
    button.type = "button";
    button.setAttribute("role", "tab");
    button.addEventListener("click", () => { tab = name; drawTabs(); void refresh(true); });
    buttons.set(name, button);
    tabs.append(button);
  }
  const drawTabs = (): void => {
    for (const [name, button] of buttons) button.setAttribute("aria-selected", String(name === tab));
    aboutForm.hidden = tab !== "pref" && tab !== "why";
    handoffs.bar.hidden = tab !== "handoffs";
  };
  aboutForm.addEventListener("submit", (event) => { event.preventDefault(); void refresh(true); });
  handoffs.onChange(() => { if (tab === "handoffs") void refresh(true); });

  page.append(askForm, noteForm, tabs, aboutForm, handoffs.bar, stated, panel);
  drawTabs();

  const fillSessions = (feed: ScribeFeed): void => {
    const keep = sessionPick.value;
    const options = feed.sessions.filter((session) => session.scribed_through !== null).map((session) => {
      const option = el("option", [sessionTitle(session), session.project].filter(Boolean).join(" · "));
      option.value = session.session_id;
      return option;
    });
    sessionPick.replaceChildren(...options);
    if (options.some((option) => option.value === keep)) sessionPick.value = keep;
    noteButton.disabled = options.length === 0;
    if (options.length === 0) noteStatus.textContent = "No scribed session yet. Run /scribe in a session first.";
  };

  let generation = 0;
  async function refresh(loading: boolean): Promise<void> {
    const current = ++generation; const showing = tab;
    if (loading) { panel.replaceChildren(el("p", "Loading.", "scribe-status")); stated.textContent = ""; }
    try {
      if (showing === "handoffs") {
        const draw = await handoffs.load();
        if (!alive() || current !== generation) return;
        draw(panel, stated);
        return;
      }
      if (showing === "feed") {
        const feed = await getJson<ScribeFeed>("/api/scribe/feed");
        if (!alive() || current !== generation) return;
        fillSessions(feed);
        const more = feed.total > feed.shown ? ` ${feed.total - feed.shown} older lines are not shown.` : "";
        stated.textContent = `${feed.shown} ${feed.shown === 1 ? "line" : "lines"} since ${feed.since} (${feed.time_zone}), refreshed every ${POLL_MS / 1000} seconds.${more}`;
        panel.replaceChildren(...(feed.sessions.length ? feed.sessions.map((session, index) => sessionBlock(session, index === 0))
          : [el("p", "Nothing scribed yet. Run /scribe in a session.", "scribe-status")]));
        return;
      }
      const topic = about.value.trim().replace(/^#/, "");
      const query = new URLSearchParams({ view: showing });
      if (topic && (showing === "pref" || showing === "why")) query.set("about", topic);
      const result = await getJson<ScribeQueryResult>(`/api/scribe/query?${query.toString()}`);
      if (!alive() || current !== generation) return;
      stated.textContent = showing === "landed"
        ? `Today, ${result.today} in ${result.time_zone}.`
        : showing === "pending" ? "Open items, oldest first." : showing === "pref" ? "Current preferences; superseded ones are hidden." : "Decisions with their reason.";
      const list = el("ul", "", "scribe-list");
      for (const entry of result.entries) list.append(entryItem(entry));
      panel.replaceChildren(result.entries.length ? list : el("p", emptyView(showing, topic || null, result.time_zone, result.today), "scribe-status"));
    } catch (error) {
      if (alive() && current === generation) panel.replaceChildren(el("p", messageFor(codeOf(error)), "scribe-status"));
    }
  }

  // The note form needs the session list even while a canned view is open.
  const loadSessions = async (): Promise<void> => {
    try { const feed = await getJson<ScribeFeed>("/api/scribe/feed"); if (alive()) fillSessions(feed); } catch { /* the feed tab reports it */ }
  };
  const timer = window.setInterval(() => {
    if (!alive()) { window.clearInterval(timer); return; }
    void refresh(false);
    if (tab !== "feed") void loadSessions();
  }, POLL_MS);
  await refresh(true);
}

export const screen: Screen = { id: "scribe", title: "Scribe", status: "built", render };
