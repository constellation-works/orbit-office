/** The Library screen's own rules, on the shared tokens and the panel and row patterns (ui/panel.ts). */
export const styles = `
.library { display:flex; flex-direction:column; gap:20px; min-width:0; }
.library .screen-head { margin:0; }
.library-kicker:empty { display:none; }
.library input:not([type="checkbox"]), .library select { min-width:0; max-width:100%; min-height:40px; }
.library form { display:flex; flex-wrap:wrap; gap:8px; margin:0; min-width:0; }
.library-search { flex:0 1 380px; min-width:0; }
.library-search input { flex:1 1 200px; }
.library-back { min-height:40px; padding:0 14px; border:1px solid var(--border-strong); border-radius:var(--radius-sm); background:transparent; color:var(--fg); font:inherit; font-size:14px; cursor:pointer; }
.library-back:hover { background:var(--bg-hover); }
.library-layout { display:flex; flex-wrap:wrap; gap:20px; align-items:flex-start; min-width:0; }
.library-aside { flex:1 1 260px; min-width:0; display:flex; flex-direction:column; gap:16px; }
.library-main { flex:999 1 520px; min-width:0; display:flex; flex-direction:column; gap:20px; }
.library-main > .panel + .panel, .library-aside > .panel + .panel { margin-top:0; }
.library-heading { margin:0; }
.library-group .panel-head { padding:12px 16px 0; }
.library-group-body { padding:4px 16px 8px; }
.library-group-body > p { font-size:13px; color:var(--fg-mute); }
.library-link { padding:0; border:0; background:none; color:var(--fg); font:inherit; font-size:14px; text-align:left; cursor:pointer; overflow-wrap:anywhere; }
.library-link:hover, .library-link:focus-visible { color:var(--accent-hover); }
.library-cite { padding:0; margin:0 4px; border:0; background:none; color:var(--accent); font-family:var(--font-mono); font-size:12px; cursor:pointer; }
.library-cite:hover { color:var(--accent-hover); }
.library h3 { margin:6px 0 0; font-size:12px; font-weight:500; text-transform:capitalize; color:var(--fg-mute); }
.library-results { display:flex; flex-direction:column; gap:6px; min-width:0; }
.library-results > p { margin:0; font-size:14px; color:var(--fg-dim); }
.library-setup .rows .rows { margin-top:4px; }
.library-setup .row .row { min-height:0; padding:6px 0; }
.library-setup .row .row .row-title { font-size:13px; color:var(--fg-dim); }
.library-sharing { margin:0; }
.library-sharing:empty { display:none; }
.library-connect { padding:14px 16px; border:1px solid var(--border-strong); border-radius:var(--radius); background:var(--bg); display:flex; flex-direction:column; align-items:flex-start; gap:6px; }
.library-connect h3 { margin:0; font-size:15px; text-transform:none; color:var(--fg); }
.library-connect p { margin:0; font-size:14px; color:var(--fg-dim); }
.library-connect-link { display:inline-flex; align-items:center; min-height:40px; margin-top:6px; padding:0 14px; border-radius:var(--radius-sm); background:var(--accent); color:var(--on-accent); font-weight:600; text-decoration:none; }
.library-connect-link:hover { background:var(--accent-hover); color:var(--on-accent); }
.library-document-body { padding:24px 28px; gap:16px; }
.library-summary { margin:0; font-size:14px; color:var(--fg-dim); }
.library-body { margin:0; white-space:pre-wrap; overflow-wrap:anywhere; font-size:15px; line-height:1.65; }
.library-part { display:flex; flex-direction:column; gap:8px; padding-top:14px; border-top:1px solid var(--hair); }
.library-part h2 { margin:0; font-size:11px; font-weight:500; letter-spacing:0.08em; text-transform:uppercase; color:var(--fg-mute); }
.library-part p { margin:0; font-size:14px; color:var(--fg-dim); }
.library-inline { display:flex; flex-wrap:wrap; gap:6px 14px; align-items:baseline; }
.library-inline .library-cite { margin:0; }
.library-inline .library-link { color:var(--accent); }
.library-part .library-revision { font-family:var(--font-mono); font-size:12px; color:var(--fg-mute); }
.library-snippet { margin:0; font-size:13px; color:var(--fg-dim); }
.library .share { margin:0; }
@media (max-width: 640px) {
  .library-search { flex-basis:100%; }
  .library-document-body { padding:16px; }
}
`;
