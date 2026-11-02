/**
 * The one Markdown renderer for stored prose: Library pages, task text, results, reviews and briefs.
 *
 * It parses a small, forgiving subset (headings, paragraphs, lists, emphasis, inline code, code blocks,
 * links, blockquotes, tables, rules) straight into DOM nodes. Nothing is ever parsed as HTML: every piece of
 * text goes in through `createTextNode` or `textContent`, so `<script>` and `<img onerror=...>` are shown as
 * the characters they are. A link keeps its target only when it is http(s), relative or an in-app `#` route;
 * any other scheme (`javascript:`, `data:`) leaves just the link's text. Images are never loaded.
 */

/** What a caller can adjust. */
export interface MarkdownOptions {
  /** Turns a bracketed document id such as `src-20261001-call#L1` into its link or button; without it the id stays plain text. */
  cite?: (id: string) => Node;
  /** Id prefixes that count as citations (`[src-…]`); default `src`, `dom`, `rb` and `art`. */
  citePrefixes?: readonly string[];
  /** The heading level a Markdown `#` becomes, so a body sits under its screen's own headings; default 3. */
  headingLevel?: number;
  /** Extra class on the wrapper. */
  className?: string;
}

const DEFAULT_CITE_PREFIXES = ["src", "dom", "rb", "art"] as const;

/** Whether a link target may become an `href`: http(s), a relative path or an in-app `#` route. */
export function isSafeHref(target: string): boolean {
  // Browsers drop tabs and newlines inside a URL and ignore leading control characters, so refuse them outright.
  if (target === "" || /[\u0000-\u0020\u007f-\u009f\u2028\u2029]/u.test(target)) return false;
  if (target.startsWith("//") || target.startsWith("\\")) return false;
  const scheme = /^([a-z][a-z0-9+.-]*):/i.exec(target);
  if (scheme !== null) return scheme[1]?.toLowerCase() === "http" || scheme[1]?.toLowerCase() === "https";
  return true;
}

interface Context {
  options: MarkdownOptions;
  citation: RegExp | null;
}

function node(tag: string, className?: string): HTMLElement {
  const element = document.createElement(tag);
  if (className !== undefined) element.className = className;
  return element;
}

function text(value: string): Text {
  return document.createTextNode(value);
}

/* ---------- inline ---------- */

const PUNCTUATION = /[!-/:-@[-`{-~]/;
const isSpace = (ch: string | undefined): boolean => ch === undefined || /\s/.test(ch);
const isWord = (ch: string | undefined): boolean => ch !== undefined && /[\p{L}\p{N}]/u.test(ch);

/** The index of the `]` closing the `[` at `open`, skipping code spans and nested brackets; -1 when unclosed. */
function closeBracket(source: string, open: number): number {
  let depth = 0;
  for (let i = open; i < source.length; i++) {
    const ch = source[i];
    if (ch === "\\") { i++; continue; }
    if (ch === "`") {
      const run = /^`+/.exec(source.slice(i))?.[0] ?? "`";
      const end = source.indexOf(run, i + run.length);
      if (end >= 0) { i = end + run.length - 1; continue; }
      i += run.length - 1;
      continue;
    }
    if (ch === "[") depth++;
    else if (ch === "]" && --depth === 0) return i;
  }
  return -1;
}

/** The `(target "title")` after a link's text: the target and the index after the `)`, or null. */
function linkTarget(source: string, open: number): { target: string; end: number } | null {
  if (source[open] !== "(") return null;
  let depth = 0;
  for (let i = open; i < source.length; i++) {
    const ch = source[i];
    if (ch === "\\") { i++; continue; }
    if (ch === "\n") return null;
    if (ch === "(") depth++;
    else if (ch === ")" && --depth === 0) {
      let inner = source.slice(open + 1, i).trim();
      if (inner.startsWith("<") && inner.includes(">")) inner = inner.slice(1, inner.indexOf(">"));
      else inner = inner.replace(/\s+(?:"[^"]*"|'[^']*')$/, "");
      return { target: inner, end: i + 1 };
    }
  }
  return null;
}

/** The index of the delimiter closing an emphasis run opened before `from`, or -1. */
function closeEmphasis(source: string, delimiter: string, from: number): number {
  for (let i = from; i < source.length; i++) {
    const ch = source[i];
    if (ch === "\\") { i++; continue; }
    if (ch === "`") {
      const run = /^`+/.exec(source.slice(i))?.[0] ?? "`";
      const end = source.indexOf(run, i + run.length);
      if (end >= 0) i = end + run.length - 1; else i += run.length - 1;
      continue;
    }
    if (!source.startsWith(delimiter, i)) continue;
    if (delimiter.length === 1) {
      // A single * or _ does not close on half of a doubled one.
      if (source[i + 1] === delimiter || source[i - 1] === delimiter) { while (source[i + 1] === delimiter) i++; continue; }
    }
    // An underscore run closes only at a word edge, so `window.__x` and snake_case stay as written.
    if (delimiter[0] === "_" && isWord(source[i + delimiter.length])) continue;
    if (i > from && !isSpace(source[i - 1])) return i;
  }
  return -1;
}

function link(target: string, children: (parent: HTMLElement) => void): Node {
  if (!isSafeHref(target)) {
    const inert = node("span");
    children(inert);
    return inert;
  }
  const anchor = document.createElement("a");
  anchor.setAttribute("href", target);
  if (/^https?:/i.test(target)) { anchor.setAttribute("rel", "noopener noreferrer"); anchor.setAttribute("target", "_blank"); }
  children(anchor);
  return anchor;
}

function inline(parent: Node, source: string, context: Context, inLink = false): void {
  let plain = "";
  const flush = (): void => { if (plain !== "") { parent.appendChild(text(plain)); plain = ""; } };
  let i = 0;
  while (i < source.length) {
    const ch = source[i] ?? "";
    const rest = (): string => source.slice(i);
    if (ch === "\\" && i + 1 < source.length && PUNCTUATION.test(source[i + 1] ?? "")) { plain += source[i + 1]; i += 2; continue; }
    if (ch === "`") {
      const run = /^`+/.exec(rest())?.[0] ?? "`";
      const end = source.indexOf(run, i + run.length);
      if (end > i + run.length - 1 && source[end + run.length] !== "`") {
        flush();
        const code = node("code");
        code.textContent = source.slice(i + run.length, end).replace(/\n/g, " ").replace(/^ (.*\S.*) $/, "$1");
        parent.appendChild(code);
        i = end + run.length;
        continue;
      }
      plain += run; i += run.length; continue;
    }
    if (ch === "[" && context.citation !== null && context.options.cite !== undefined) {
      context.citation.lastIndex = i;
      const cited = context.citation.exec(source);
      if (cited !== null && cited.index === i) {
        flush();
        for (const id of (cited[1] ?? "").split(/\s*[,;]\s*/)) parent.appendChild(context.options.cite(id));
        i += cited[0].length;
        continue;
      }
    }
    if ((ch === "[" || (ch === "!" && source[i + 1] === "[")) && !(inLink && ch === "[")) {
      const image = ch === "!";
      const open = image ? i + 1 : i;
      const close = closeBracket(source, open);
      const target = close < 0 ? null : linkTarget(source, close + 1);
      if (close >= 0 && target !== null) {
        flush();
        const label = source.slice(open + 1, close);
        if (image) inline(parent, label, context, true);
        else parent.appendChild(link(target.target, (anchor) => { inline(anchor, label, context, true); }));
        i = target.end;
        continue;
      }
    }
    if (!inLink && (ch === "h" || ch === "H")) {
      const url = /^https?:\/\/[^\s<>]+/i.exec(rest());
      if (url !== null && !isWord(source[i - 1])) {
        let address = url[0].replace(/[.,;:!?'"]+$/, "");
        // Drop a trailing ) that has no opening ( inside the address.
        while (address.endsWith(")") && (address.match(/\(/g)?.length ?? 0) < (address.match(/\)/g)?.length ?? 0)) address = address.slice(0, -1);
        flush();
        parent.appendChild(link(address, (anchor) => { anchor.appendChild(text(address)); }));
        i += address.length;
        continue;
      }
    }
    if (ch === "*" || ch === "_" || ch === "~") {
      const double = source[i + 1] === ch;
      const delimiters = ch === "~" ? (double ? ["~~"] : []) : double ? [ch + ch, ch] : [ch];
      let matched = false;
      for (const delimiter of delimiters) {
        const after = source[i + delimiter.length];
        if (isSpace(after)) continue;
        if (ch === "_" && isWord(source[i - 1])) continue;
        const end = closeEmphasis(source, delimiter, i + delimiter.length);
        if (end < 0) continue;
        flush();
        const wrapper = node(delimiter === "~~" ? "del" : delimiter.length === 2 ? "strong" : "em");
        inline(wrapper, source.slice(i + delimiter.length, end), context, inLink);
        parent.appendChild(wrapper);
        i = end + delimiter.length;
        matched = true;
        break;
      }
      if (matched) continue;
      plain += ch; i += 1; continue;
    }
    plain += ch; i += 1;
  }
  flush();
}

/* ---------- blocks ---------- */

const FENCE = /^ {0,3}(`{3,}|~{3,})[ \t]*([^`\s]*)[^`]*$/;
const HEADING = /^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$/;
const RULE = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const QUOTE = /^ {0,3}>[ ]?/;
const ITEM = /^( *)([-*+]|\d{1,9}[.)])(?:[ \t]+(.*))?$/;
const DIVIDER = /^[ \t]*\|?[ \t]*:?-+:?[ \t]*(?:\|[ \t]*:?-+:?[ \t]*)*\|?[ \t]*$/;

/** The cells of a table line, with the outer pipes removed. */
function cells(line: string): string[] {
  let body = line.trim();
  if (body.startsWith("|")) body = body.slice(1);
  if (body.endsWith("|") && !body.endsWith("\\|")) body = body.slice(0, -1);
  const out: string[] = [];
  let current = "";
  for (let i = 0; i < body.length; i++) {
    const ch = body[i];
    if (ch === "\\" && body[i + 1] === "|") { current += "|"; i++; continue; }
    if (ch === "`") {
      const end = body.indexOf("`", i + 1);
      if (end > 0) { current += body.slice(i, end + 1); i = end; continue; }
    }
    if (ch === "|") { out.push(current.trim()); current = ""; continue; }
    current += ch;
  }
  out.push(current.trim());
  return out;
}

const startsTable = (lines: string[], i: number): boolean => {
  const head = lines[i];
  const divider = lines[i + 1];
  if (head === undefined || divider === undefined || !head.includes("|") || !divider.includes("-") || !DIVIDER.test(divider)) return false;
  return cells(head).length === cells(divider).length;
};

/** Whether `line` begins a block that ends a running paragraph. */
function interrupts(lines: string[], i: number): boolean {
  const line = lines[i] ?? "";
  if (FENCE.test(line) || HEADING.test(line) || RULE.test(line) || QUOTE.test(line) || startsTable(lines, i)) return true;
  const item = ITEM.exec(line);
  return item !== null && item[3] !== undefined && item[3] !== "" && (!/^\d/.test(item[2] ?? "") || /^1[.)]$/.test(item[2] ?? ""));
}

function blocks(parent: HTMLElement, lines: string[], context: Context, tight = false): void {
  const level = Math.min(6, Math.max(1, context.options.headingLevel ?? 3));
  let i = 0;
  while (i < lines.length) {
    const line = lines[i] ?? "";
    if (line.trim() === "") { i++; continue; }

    const fence = FENCE.exec(line);
    if (fence !== null) {
      const marker = fence[1] ?? "```";
      const body: string[] = [];
      i++;
      while (i < lines.length && !new RegExp(`^ {0,3}${marker[0] === "`" ? "`" : "~"}{${marker.length},}[ \\t]*$`).test(lines[i] ?? "")) { body.push(lines[i] ?? ""); i++; }
      i++;
      const pre = node("pre");
      const code = node("code");
      const language = fence[2] ?? "";
      if (/^[\w+#.-]{1,24}$/.test(language)) { code.className = `language-${language}`; pre.setAttribute("data-language", language); }
      code.textContent = body.join("\n");
      pre.appendChild(code);
      parent.appendChild(pre);
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading !== null) {
      const element = node(`h${Math.min(6, level + (heading[1]?.length ?? 1) - 1)}`);
      inline(element, heading[2] ?? "", context);
      parent.appendChild(element);
      i++;
      continue;
    }

    if (RULE.test(line)) { parent.appendChild(node("hr")); i++; continue; }

    if (QUOTE.test(line)) {
      const inner: string[] = [];
      while (i < lines.length && QUOTE.test(lines[i] ?? "")) { inner.push((lines[i] ?? "").replace(QUOTE, "")); i++; }
      const quote = node("blockquote");
      blocks(quote, inner, context);
      parent.appendChild(quote);
      continue;
    }

    if (startsTable(lines, i)) {
      const head = cells(line);
      const align = cells(lines[i + 1] ?? "").map((cell) => (cell.startsWith(":") && cell.endsWith(":") ? "center" : cell.endsWith(":") ? "right" : ""));
      const table = node("table");
      const headRow = node("tr");
      head.forEach((cell, column) => {
        const th = node("th", align[column] ? `md-${align[column]}` : undefined);
        inline(th, cell, context);
        headRow.appendChild(th);
      });
      const thead = node("thead"); thead.appendChild(headRow);
      const tbody = node("tbody");
      i += 2;
      while (i < lines.length && (lines[i] ?? "").trim() !== "" && (lines[i] ?? "").includes("|")) {
        const row = node("tr");
        const values = cells(lines[i] ?? "");
        head.forEach((_, column) => {
          const td = node("td", align[column] ? `md-${align[column]}` : undefined);
          inline(td, values[column] ?? "", context);
          row.appendChild(td);
        });
        tbody.appendChild(row);
        i++;
      }
      table.append(thead, tbody);
      const wrap = node("div", "md-table");
      wrap.appendChild(table);
      parent.appendChild(wrap);
      continue;
    }

    const item = ITEM.exec(line);
    if (item !== null) {
      i = list(parent, lines, i, context);
      continue;
    }

    const paragraph: string[] = [line.trim()];
    i++;
    while (i < lines.length && (lines[i] ?? "").trim() !== "" && !interrupts(lines, i)) { paragraph.push((lines[i] ?? "").trim()); i++; }
    if (tight) {
      inline(parent, paragraph.join("\n"), context);
    } else {
      const element = node("p");
      inline(element, paragraph.join("\n"), context);
      parent.appendChild(element);
    }
  }
}

/** One list starting at `lines[start]`; returns the index after it. */
function list(parent: HTMLElement, lines: string[], start: number, context: Context): number {
  const first = ITEM.exec(lines[start] ?? "");
  const baseIndent = first?.[1]?.length ?? 0;
  const ordered = /^\d/.test(first?.[2] ?? "");
  const element = node(ordered ? "ol" : "ul");
  if (ordered && Number.parseInt(first?.[2] ?? "1", 10) !== 1) element.setAttribute("start", String(Number.parseInt(first?.[2] ?? "1", 10)));
  const items: string[][] = [];
  let loose = false;
  let i = start;
  while (i < lines.length) {
    const match = ITEM.exec(lines[i] ?? "");
    // Deeper items were taken into the previous item below; a shallower one belongs to an outer list.
    if (match === null || match[1]?.length !== baseIndent || /^\d/.test(match[2] ?? "") !== ordered) break;
    const indent = baseIndent;
    const content = match[3] ?? "";
    const marker = (match[2] ?? "").length + 1;
    const body: string[] = [content];
    i++;
    while (i < lines.length) {
      const next = lines[i] ?? "";
      if (next.trim() === "") {
        // A blank line stays in the item only when indented content follows it.
        let ahead = i;
        while (ahead < lines.length && (lines[ahead] ?? "").trim() === "") ahead++;
        const following = lines[ahead] ?? "";
        if (ahead < lines.length && (/^ */.exec(following)?.[0].length ?? 0) > indent) { for (; i < ahead; i++) body.push(""); loose = loose || ITEM.exec(following) === null; continue; }
        break;
      }
      const spaces = /^ */.exec(next)?.[0].length ?? 0;
      if (spaces > indent) { body.push(next.slice(Math.min(spaces, indent + marker))); i++; continue; }
      // Lazy continuation of the item's paragraph.
      if (!ITEM.test(next) && !interrupts(lines, i) && (body[body.length - 1] ?? "").trim() !== "") { body.push(next.trim()); i++; continue; }
      break;
    }
    items.push(body);
    // A blank line between items makes the list loose.
    let ahead = i;
    while (ahead < lines.length && (lines[ahead] ?? "").trim() === "") ahead++;
    const following = ahead < lines.length ? ITEM.exec(lines[ahead] ?? "") : null;
    if (following !== null && ahead > i && (following[1]?.length ?? 99) === baseIndent && /^\d/.test(following[2] ?? "") === ordered) { loose = true; i = ahead; } else if (ahead > i) break;
  }
  for (const body of items) {
    const li = node("li");
    const task = /^\[([ xX])\][ \t]+/.exec(body[0] ?? "");
    if (task !== null) { body[0] = (body[0] ?? "").slice(task[0].length); li.className = "md-task"; li.appendChild(text(task[1] === " " ? "☐ " : "☑ ")); }
    blocks(li, body, context, !loose);
    element.appendChild(li);
  }
  parent.appendChild(element);
  return i;
}

/** Renders `source` as Markdown into a new `div.md`; the element holds only text nodes and the elements above. */
export function renderMarkdown(source: string, options: MarkdownOptions = {}): HTMLElement {
  const prefixes = (options.citePrefixes ?? DEFAULT_CITE_PREFIXES).map((prefix) => prefix.replace(/[^\w-]/g, ""));
  const id = `(?:${prefixes.join("|")})-[^\\]\\s,;]+`;
  const context: Context = {
    options,
    citation: options.cite === undefined || prefixes.length === 0 ? null : new RegExp(`\\[(${id}(?:\\s*[,;]\\s*${id})*)\\]`, "y"),
  };
  const root = node("div", options.className === undefined ? "md" : `md ${options.className}`);
  blocks(root, source.replace(/\r\n?/g, "\n").replace(/\t/g, "    ").split("\n"), context);
  return root;
}
