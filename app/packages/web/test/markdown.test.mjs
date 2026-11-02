import assert from "node:assert/strict";
import { test } from "node:test";
import { isSafeHref, renderMarkdown } from "../dist/ui/markdown.js";

// A minimal DOM: elements, text nodes and attributes are all the renderer touches.
class Text { constructor(data) { this.data = data; } get textContent() { return this.data; } }
class Element {
  constructor(tag) { this.tagName = tag.toUpperCase(); this.children = []; this.attrs = {}; this.className = ""; this._text = null; }
  set textContent(value) { this._text = String(value); this.children = []; }
  get textContent() { return this._text ?? this.children.map((c) => c.textContent).join(""); }
  appendChild(child) { this.children.push(child); return child; }
  append(...children) { for (const child of children) this.children.push(typeof child === "string" ? new Text(child) : child); }
  setAttribute(name, value) { this.attrs[name] = String(value); }
  getAttribute(name) { return this.attrs[name] ?? null; }
}
globalThis.document = { createElement: (tag) => new Element(tag), createTextNode: (data) => new Text(data) };

const all = (node) => node instanceof Element ? [node, ...node.children.flatMap(all)] : [];
const tags = (node, tag) => all(node).filter((n) => n.tagName === tag.toUpperCase());
const outline = (node) => node instanceof Element ? `<${node.tagName.toLowerCase()}>${node._text ?? node.children.map(outline).join("")}</${node.tagName.toLowerCase()}>` : node.data;
const render = (source, options) => outline(renderMarkdown(source, options));

test("headings, paragraphs and emphasis become elements; the heading level starts under the screen's own", () => {
  assert.equal(render("## Plan\n\nA **bold** and *slanted* and ~~old~~ word, `code`."), "<div><h4>Plan</h4><p>A <strong>bold</strong> and <em>slanted</em> and <del>old</del> word, <code>code</code>.</p></div>");
  assert.equal(render("# Top", { headingLevel: 4 }), "<div><h4>Top</h4></div>");
  assert.equal(render("###### Deep", { headingLevel: 5 }), "<div><h6>Deep</h6></div>");
  assert.equal(render("snake_case_name and 2 * 3 * 4"), "<div><p>snake_case_name and 2 * 3 * 4</p></div>");
  assert.equal(render("window.__x = 1 then window.__y"), "<div><p>window.__x = 1 then window.__y</p></div>");
  assert.equal(render("__bold__ and _em_"), "<div><p><strong>bold</strong> and <em>em</em></p></div>");
});

test("lists: bullets, numbers, nesting, tight and loose, task boxes", () => {
  assert.equal(render("- one\n- two\n  - nested\n- three"), "<div><ul><li>one</li><li>two<ul><li>nested</li></ul></li><li>three</li></ul></div>");
  assert.equal(render("3. c\n4. d"), "<div><ol><li>c</li><li>d</li></ol></div>");
  assert.equal(tags(renderMarkdown("3. c\n4. d"), "ol")[0].getAttribute("start"), "3");
  assert.equal(render("- a\n\n- b"), "<div><ul><li><p>a</p></li><li><p>b</p></li></ul></div>");
  assert.equal(render("- [x] done\n- [ ] todo"), "<div><ul><li>☑ done</li><li>☐ todo</li></ul></div>");
});

test("code blocks keep their text verbatim, and inline code keeps markup characters", () => {
  const source = "```js\nconst a = '<b>' ** 2;\n- not a list\n```\nafter";
  const root = renderMarkdown(source);
  assert.equal(tags(root, "pre").length, 1);
  assert.equal(tags(root, "code")[0].textContent, "const a = '<b>' ** 2;\n- not a list");
  assert.equal(tags(root, "code")[0].className, "language-js");
  assert.equal(render("Use `**x**` here"), "<div><p>Use <code>**x**</code> here</p></div>");
});

test("blockquotes, rules and tables", () => {
  assert.equal(render("> quoted\n> more\n\n---"), "<div><blockquote><p>quoted\nmore</p></blockquote><hr></hr></div>");
  const table = renderMarkdown("| Name | Qty |\n|:-----|----:|\n| apples | 3 |\n| pears | 4 |");
  assert.deepEqual(tags(table, "th").map((n) => n.textContent), ["Name", "Qty"]);
  assert.deepEqual(tags(table, "td").map((n) => n.textContent), ["apples", "3", "pears", "4"]);
  assert.equal(tags(table, "th")[1].className, "md-right");
  assert.equal(tags(table, "tr").length, 3);
  assert.equal(tags(renderMarkdown("a | b\nnot a divider"), "table").length, 0);
});

test("links keep http(s), relative and in-app # targets", () => {
  const root = renderMarkdown("[a](https://example.org/x) [b](http://example.org) [c](docs/page.md) [d](#library) [e](?source=src-1#source) see https://example.org/y.");
  assert.deepEqual(tags(root, "a").map((a) => a.getAttribute("href")), ["https://example.org/x", "http://example.org", "docs/page.md", "#library", "?source=src-1#source", "https://example.org/y"]);
  assert.equal(tags(root, "a")[0].getAttribute("rel"), "noopener noreferrer");
  assert.equal(tags(root, "a")[2].getAttribute("rel"), null);
});

test("a script, raw HTML and javascript: links are inert", () => {
  const hostile = [
    "<script>alert(1)</script>",
    "<img src=x onerror=alert(1)>",
    "[click](javascript:alert(1))",
    "[tab](java\tscript:alert(1))",
    "[caps](JaVaScRiPt:alert(1))",
    "[space]( javascript:alert(1))",
    "[data](data:text/html,<script>alert(1)</script>)",
    "[vb](vbscript:msgbox)",
    "[proto](//evil.example/x)",
    "![pixel](https://tracker.example/p.png)",
    "<a href=\"javascript:alert(1)\">raw</a>",
    "```\n<script>alert(2)</script>\n```",
  ].join("\n\n");
  const root = renderMarkdown(hostile);
  const elements = all(root).map((n) => n.tagName);
  for (const banned of ["SCRIPT", "IMG", "IFRAME", "STYLE"]) assert.ok(!elements.includes(banned), `${banned} must not be rendered`);
  assert.deepEqual(tags(root, "a"), [], "no unsafe target becomes a link");
  for (const node of all(root)) for (const name of Object.keys(node.attrs)) assert.ok(!name.startsWith("on"), `${name} must not be set`);
  const shown = root.textContent;
  assert.ok(shown.includes("<script>alert(1)</script>"), "the script is shown as text");
  assert.ok(shown.includes("<img src=x onerror=alert(1)>"));
  assert.ok(shown.includes("click") && shown.includes("pixel"), "link text stays readable");
  for (const target of ["javascript:alert(1)", "JaVaScRiPt:x", " javascript:x", "java\tscript:x", "data:text/html,x", "vbscript:x", "//evil.example", "file:///etc/passwd", "\\\\host\\share", ""]) assert.equal(isSafeHref(target), false, target);
  for (const target of ["https://a.example", "HTTP://a.example", "a/b.md", "./b", "../b", "#today", "?x=1", "/api/x"]) assert.equal(isSafeHref(target), true, target);
});

test("citations call the hook; without a hook they stay plain text", () => {
  const cite = (id) => { const b = new Element("button"); b.textContent = "View source"; b.attrs["data-doc"] = id; return b; };
  const root = renderMarkdown("Claim [src-20261001-call#L1] and both [dom-a; rb-b] but not [ORB-1] or [x](src-1)", { cite });
  assert.deepEqual(tags(root, "button").map((b) => b.getAttribute("data-doc")), ["src-20261001-call#L1", "dom-a", "rb-b"]);
  assert.ok(root.textContent.includes("[ORB-1]"));
  const extra = renderMarkdown("See [ORB-7]", { cite, citePrefixes: ["ORB"] });
  assert.deepEqual(tags(extra, "button").map((b) => b.getAttribute("data-doc")), ["ORB-7"]);
  assert.equal(tags(renderMarkdown("Claim [src-1]"), "button").length, 0);
});
