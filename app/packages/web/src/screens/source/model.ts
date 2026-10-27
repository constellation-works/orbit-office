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
