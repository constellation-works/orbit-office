import type { SourceView } from "@orbit-office/contracts";

export function seconds(stamp: string): number | null {
  if (!/^\d{2,}:[0-5]\d:[0-5]\d$/.test(stamp)) return null;
  const [h = 0, m = 0, s = 0] = stamp.split(":").map(Number);
  const value = h * 3600 + m * 60 + s;
  return Number.isSafeInteger(value) ? value : null;
}
export function clock(time: number): string {
  return [Math.floor(time / 3600), Math.floor(time / 60) % 60, Math.floor(time % 60)].map(n => String(n).padStart(2, "0")).join(":");
}
export function anchorTarget(view: SourceView, hash: string): { time: number | null; lines: number[]; frame?: number; chapter?: number } | null {
  const range = /^#L([1-9]\d*)(?:-([1-9]\d*))?$/.exec(hash);
  if (range) {
    const first = Number(range[1]); const last = Number(range[2] ?? range[1]);
    if (last < first || last > view.text.split("\n").length) return null;
    return { time: null, lines: Array.from({ length: last - first + 1 }, (_, i) => first - 1 + i) };
  }
  const chapter = /^#chapter=([1-9]\d*)$/.exec(hash);
  const found = chapter ? view.chapters.find(c => c.number === Number(chapter[1])) : undefined;
  const match = /^#(t|frame)=(.+)$/.exec(hash);
  const time = found?.start ?? (match ? seconds(match[2] ?? "") : null);
  if (time === null) return null;
  const lines = view.text.split("\n"); let line = -1; let end = lines.length;
  lines.forEach((text, i) => {
    const stamp = /^\[(\d{2,}:[0-5]\d:[0-5]\d)\]/.exec(text);
    const start = stamp ? seconds(stamp[1] ?? "") : null;
    if (start !== null && start <= time) line = i;
    else if (start !== null && line >= 0 && end === lines.length) end = i;
  });
  const span = line < 0 ? [] : lines.map((text, i) => ({ text, i })).filter(row => row.i >= line && row.i < end && row.text.trim() !== "").map(row => row.i);
  const result: { time: number; lines: number[]; frame?: number; chapter?: number } = { time, lines: span };
  if (found) result.chapter = found.number;
  if (match?.[1] === "frame") { const index = view.frames.findIndex(frame => frame.time === time); if (index < 0) return null; result.frame = index; }
  return result;
}
/** A transcript turn: `[hh:mm:ss] Speaker: text`, or a stamped line with no speaker. */
export interface Turn { stamp: string; time: number; who: string | null; text: string }

export function parseTurn(line: string): Turn | null {
  const stamp = /^\[(\d{2,}:[0-5]\d:[0-5]\d)\]\s?(.*)$/.exec(line);
  if (stamp === null || stamp[1] === undefined) return null;
  const time = seconds(stamp[1]);
  if (time === null) return null;
  const rest = stamp[2] ?? "";
  const speaker = /^([^:\[\]]{1,60}):\s+(.*)$/.exec(rest);
  return speaker && speaker[1] !== undefined
    ? { stamp: stamp[1], time, who: speaker[1].trim(), text: speaker[2] ?? "" }
    : { stamp: stamp[1], time, who: null, text: rest };
}

/** A mark on the timeline or a tag on a turn, from what the source view records. */
export interface Mark {
  time: number; kind: "chapter" | "aside" | "gotcha";
  /** The whole mark in words, for the timeline. */
  label: string;
  /** The short tag on its transcript turn. */
  tag: string;
  anchor: string;
}

/** Chapters (tangents as asides) and gotchas with a time cite, in time order. */
export function sourceMarks(view: Pick<SourceView, "chapters" | "gotchas">): Mark[] {
  const marks: Mark[] = view.chapters.map(chapter => ({
    time: chapter.start, kind: chapter.label === "tangent" ? "aside" : "chapter",
    label: `${chapter.label === "tangent" ? "Aside" : "Chapter"}: ${chapter.title}`,
    tag: `${chapter.label === "tangent" ? "Aside" : "Chapter"} · ${chapter.title}`, anchor: `#chapter=${chapter.number}`,
  }));
  for (const gotcha of view.gotchas) {
    for (const cite of gotcha.cites) {
      const stamp = /#(?:t|frame)=(.+)$/.exec(cite)?.[1] ?? "";
      const time = seconds(stamp);
      if (time !== null) { marks.push({ time, kind: "gotcha", label: `Gotcha: ${gotcha.text}`, tag: "Gotcha", anchor: `#t=${stamp}` }); break; }
    }
  }
  return marks.sort((a, b) => a.time - b.time);
}

/** The latest time the source knows of: its last turn, chapter, frame or mark. */
export function sourceLength(view: Pick<SourceView, "text" | "chapters" | "frames" | "gotchas">): number {
  let last = 0;
  for (const line of view.text.split("\n")) { const turn = parseTurn(line); if (turn && turn.time > last) last = turn.time; }
  for (const chapter of view.chapters) last = Math.max(last, chapter.start);
  for (const frame of view.frames) last = Math.max(last, frame.time);
  for (const mark of sourceMarks({ chapters: [], gotchas: view.gotchas })) last = Math.max(last, mark.time);
  return last;
}

/**
 * The marks that fall in each turn, keyed by line index: a mark belongs to
 * the last turn that starts at or before it. Lines without a stamp get none.
 */
export function turnMarks(lines: readonly string[], marks: readonly Mark[]): Map<number, Mark[]> {
  const starts = lines.flatMap((line, index) => { const turn = parseTurn(line); return turn ? [{ index, time: turn.time }] : []; });
  const result = new Map<number, Mark[]>();
  for (const mark of marks) {
    let owner: number | null = null;
    for (const start of starts) if (start.time <= mark.time) owner = start.index;
    if (owner === null) continue;
    result.set(owner, [...(result.get(owner) ?? []), mark]);
  }
  return result;
}

export const supportedFile = (name: string): boolean => /\.(txt|md|vtt|html?|pdf|docx|pptx|png|jpe?g|wav|mp3|m4a|mp4|webm|mov)$/i.test(name);

/** Drain every directory-reader page; the browser may return only 100 entries per read. */
export async function droppedFiles(transfer: DataTransfer): Promise<{ files: File[]; folder: boolean }> {
  type Entry = { isFile: boolean; isDirectory: boolean; file: (ok: (file: File) => void, fail: (error: DOMException) => void) => void; createReader: () => { readEntries: (ok: (entries: Entry[]) => void, fail: (error: DOMException) => void) => void } };
  const entries = Array.from(transfer.items ?? []).map(item => (item as unknown as { webkitGetAsEntry?: () => Entry | null }).webkitGetAsEntry?.()).filter((e): e is Entry => e != null);
  if (!entries.length) return { files: Array.from(transfer.files), folder: false };
  async function walk(entry: Entry): Promise<File[]> {
    if (entry.isFile) return [await new Promise<File>((ok, fail) => entry.file(ok, fail))];
    if (!entry.isDirectory) return [];
    const reader = entry.createReader(); const files: File[] = [];
    for (;;) {
      const page = await new Promise<Entry[]>((ok, fail) => reader.readEntries(ok, fail));
      if (!page.length) return files;
      for (const child of page) files.push(...await walk(child));
    }
  }
  const files: File[] = [];
  for (const entry of entries) files.push(...await walk(entry));
  return { files, folder: entries.some(e => e.isDirectory) };
}
