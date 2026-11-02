/** Scribe screen styles: one column from 320 px, so it fits a small installed window. */
export const styles = `
.scribe-page { display:grid; gap:20px; min-width:0; max-width:880px }
.scribe-page *, .scribe-page *::before { min-width:0 }
.scribe-page [hidden] { display:none !important }
.scribe-page h2 { margin:0 0 8px; font-size:16px }
.scribe-page h3 { margin:0; font-size:15px; overflow-wrap:anywhere }
.scribe-page h4 { margin:0 0 4px; font-size:13px; color:var(--fg-dim) }
.scribe-lede { color:var(--fg-mute); margin:0 }
.scribe-page button, .scribe-page input, .scribe-page textarea, .scribe-page select { font:inherit; font-size:14px; max-width:100% }
.scribe-page button { border:1px solid var(--border); border-radius:var(--radius-sm); min-height:36px; padding:0 12px; background:transparent; color:var(--fg); cursor:pointer }
.scribe-page button:hover { background:var(--bg-hover) }
.scribe-page button:disabled { opacity:.6; cursor:default }
.scribe-page input, .scribe-page textarea, .scribe-page select { border:1px solid var(--border); border-radius:var(--radius-sm); padding:8px 10px; background:var(--bg-elev); color:inherit; width:100% }
.scribe-page textarea { min-height:64px; resize:vertical }
.scribe-panel { padding:14px; background:var(--bg-elev); border:1px solid var(--border); border-radius:var(--radius); display:grid; gap:10px }
.scribe-row { display:flex; flex-wrap:wrap; gap:8px; align-items:center }
.scribe-row > input, .scribe-row > select { flex:1 1 180px; width:auto }
.scribe-status { margin:0; color:var(--fg-mute); white-space:pre-wrap; overflow-wrap:anywhere }
.scribe-tabs { display:flex; flex-wrap:wrap; gap:6px }
.scribe-tabs button[aria-selected=true] { background:var(--accent); color:var(--on-accent); border-color:var(--accent) }
.scribe-stated { margin:0; font-size:13px; color:var(--fg-mute) }
.scribe-results { display:grid; gap:8px }
.scribe-list { list-style:none; margin:0; padding:0; display:grid; gap:8px }
.scribe-entry { padding:10px 12px; border:1px solid var(--border); border-radius:var(--radius-sm); background:var(--bg); display:grid; gap:6px }
.scribe-entry + .scribe-entry { margin-top:8px }
.scribe-entry-head { display:flex; flex-wrap:wrap; gap:6px; align-items:center }
.scribe-entry time { font-family:var(--font-mono); font-size:12px; color:var(--fg-mute) }
.scribe-chip { font-family:var(--font-mono); font-size:12px }
.scribe-quote { margin:0; padding-left:10px; border-left:2px solid var(--border-strong); white-space:pre-wrap; overflow-wrap:anywhere }
.scribe-fields { margin:0; display:grid; grid-template-columns:auto minmax(0,1fr); gap:2px 10px; font-size:13px }
.scribe-fields dt { color:var(--fg-mute) }
.scribe-fields dd { margin:0; overflow-wrap:anywhere }
.scribe-cite { display:flex; flex-wrap:wrap; gap:4px 10px; align-items:center; font-family:var(--font-mono); font-size:12px; color:var(--fg-dim) }
.scribe-cite code { overflow-wrap:anywhere; word-break:break-all }
.scribe-cite button { min-height:28px; padding:0 8px; font-size:12px; font-family:var(--font-sans) }
.scribe-answer { display:grid; gap:8px }
.scribe-claim { padding:10px 12px; border:1px solid var(--border); border-radius:var(--radius-sm); display:grid; gap:6px }
.scribe-claim p { margin:0; overflow-wrap:anywhere }
.scribe-page .scribe-card-title { text-align:left; font-size:15px; font-weight:600; border:none; padding:0; min-height:0; background:transparent; overflow-wrap:anywhere }
.scribe-page .scribe-card-title:hover { background:transparent; text-decoration:underline }
.scribe-card-summary { margin:0; white-space:pre-wrap; overflow-wrap:anywhere }
.scribe-card-section { display:grid; gap:2px }
.scribe-card-items { margin:0; padding-left:18px; display:grid; gap:2px; overflow-wrap:anywhere }
.scribe-card-counts { margin:0; font-size:13px; color:var(--fg-dim) }
.scribe-tag-entry { display:grid; gap:4px }
.scribe-tag-list { display:grid; gap:8px }
.scribe-tag-form label { display:grid; gap:4px; font-size:13px; color:var(--fg-dim) }
.scribe-tag-row strong { font-family:var(--font-mono); font-size:13px }
.scribe-agreement { display:grid; gap:8px }
.scribe-agreement h3 { margin:0; font-size:15px }
.scribe-agreement-row { overflow-wrap:anywhere }
`;
