/**
 * Scribe screen styles, over the shared panels and rows (static/style.css).
 * Panels sit side by side from 680 px and stack below; everything fits
 * from 320 px with no horizontal scroll.
 */
export const styles = `
.scribe-page { display:flex; flex-direction:column; gap:20px; min-width:0; max-width:1120px }
.scribe-page *, .scribe-page *::before { min-width:0 }
.scribe-page [hidden] { display:none !important }
.scribe-page .screen-head { margin:0 }
.scribe-page .panel + .panel, .scribe-page .panel-grid + .panel, .scribe-page .panel + .panel-grid { margin-top:0 }
.scribe-page .panel-grid { grid-template-columns:repeat(auto-fit, minmax(min(100%, 340px), 1fr)); align-items:start }
.scribe-page h3 { margin:0; font-size:14px; overflow-wrap:anywhere }
.scribe-page h4 { margin:0 0 4px; font-size:13px; color:var(--fg-dim) }
.scribe-page button, .scribe-page input, .scribe-page textarea, .scribe-page select { font:inherit; font-size:14px; max-width:100% }
.scribe-page button:not(.primary-action):not(.choice):not(.scribe-card-title) { border:1px solid var(--border); border-radius:var(--radius-sm); min-height:36px; padding:0 12px; background:transparent; color:var(--fg); cursor:pointer }
.scribe-page button:not(.primary-action):not(.choice):not(.scribe-card-title):hover { background:var(--bg-hover) }
.scribe-page button:disabled { opacity:.6; cursor:default }
.scribe-page input, .scribe-page textarea, .scribe-page select { border:1px solid var(--border); border-radius:var(--radius-sm); padding:8px 10px; background:var(--bg-elev); color:inherit; width:100% }
.scribe-page textarea { min-height:64px; resize:vertical }

.scribe-ask { display:flex; flex-direction:column; gap:10px }
.scribe-ask-row { display:flex; flex-wrap:wrap; gap:8px }
.scribe-ask-row input { flex:1 1 260px; width:auto; min-height:44px; padding:0 14px; border-color:var(--border-strong) }
.scribe-ask-row .primary-action { min-height:44px; padding:0 18px; align-self:auto }

.scribe-conflicts { display:flex; flex-direction:column; gap:16px }
.scribe-conflicts:empty { display:none }
.scribe-conflict { border:1px solid var(--attention-line); border-radius:var(--radius); background:var(--attention-wash); padding:18px 20px; display:flex; flex-direction:column; gap:14px }
.scribe-conflict-head { display:flex; flex-wrap:wrap; align-items:center; justify-content:space-between; gap:8px }
.scribe-conflict-head h2 { margin:0; font-size:16px; font-weight:600; color:var(--attention-soft); overflow-wrap:anywhere }
.scribe-conflict-sides { display:grid; grid-template-columns:repeat(auto-fit, minmax(min(100%, 260px), 1fr)); gap:12px }
.scribe-conflict-side { border:1px solid var(--border); border-radius:8px; background:var(--bg-elev); padding:14px; display:flex; flex-direction:column; gap:6px }
.scribe-conflict-when { font-size:12px; color:var(--fg-mute) }
.scribe-conflict-text { margin:0; font-size:15px; overflow-wrap:anywhere }
.scribe-conflict-why { font-size:13px; color:var(--fg-dim); overflow-wrap:anywhere }
.scribe-conflict .choice:first-child { background:var(--attention); border-color:var(--attention); color:var(--on-accent) }
.scribe-conflict .choice:first-child:hover { background:var(--attention-soft) }
@media (max-width:640px) { .scribe-conflict { padding:16px } }

.scribe-stated { margin:0; font-size:13px; color:var(--fg-mute) }
.scribe-stated:empty { display:none }
.scribe-status { margin:0; color:var(--fg-mute); white-space:pre-wrap; overflow-wrap:anywhere }
.scribe-results { display:flex; flex-direction:column; gap:0 }
.scribe-results > .scribe-card + .scribe-card { border-top:1px solid var(--hair) }
.scribe-row-item { align-items:flex-start }
.scribe-row-item .row-title { overflow-wrap:anywhere }
.scribe-line { font-size:12px; color:var(--fg-mute); overflow-wrap:anywhere }
.scribe-line-label { color:var(--fg-mute) }
.scribe-row-item .scribe-line:not(:first-of-type) { color:var(--fg-dim) }
.scribe-line time { font-family:var(--font-mono) }
.scribe-chip { font-family:var(--font-mono); font-size:12px }
.scribe-quote { margin:0; padding-left:10px; border-left:2px solid var(--border-strong); white-space:pre-wrap; overflow-wrap:anywhere; font-size:13px; color:var(--fg-dim) }
.scribe-cite { display:flex; flex-wrap:wrap; gap:4px 10px; align-items:center; font-family:var(--font-mono); font-size:12px; color:var(--fg-dim) }
.scribe-cite code { overflow-wrap:anywhere; word-break:break-all }
.scribe-page .scribe-cite button { min-height:28px; padding:0 8px; font-size:12px; font-family:var(--font-sans) }
.scribe-actions { display:flex; flex-wrap:wrap; gap:8px; align-items:center }

.scribe-answer { display:flex; flex-direction:column; gap:8px }
.scribe-answer:empty { display:none }
.scribe-list { list-style:none; margin:0; padding:0; display:flex; flex-direction:column; gap:8px }
.scribe-claim { padding:12px 14px; border:1px solid var(--border); border-radius:var(--radius-sm); background:var(--bg-elev); display:flex; flex-direction:column; gap:6px }
.scribe-claim p { margin:0; overflow-wrap:anywhere }

.scribe-card { display:flex; flex-direction:column; gap:6px; padding:12px 0 }
.scribe-card:first-child { padding-top:0 }
.scribe-card-top { display:flex; justify-content:space-between; align-items:baseline; gap:8px }
.scribe-card-age { flex:none; font-size:12px; color:var(--fg-mute) }
.scribe-entry-head { display:flex; flex-wrap:wrap; gap:6px; align-items:center }
.scribe-page .scribe-card-title { text-align:left; font-size:14px; font-weight:600; border:none; padding:0; min-height:0; background:transparent; overflow-wrap:anywhere }
.scribe-page .scribe-card-title:hover { background:transparent; text-decoration:underline }
.scribe-detail h3 { font-size:15px }
.scribe-card-summary { margin:0; font-size:13px; line-height:1.5; color:var(--fg-dim); white-space:pre-wrap; overflow-wrap:anywhere }
.scribe-card-summary.md { white-space:normal; gap:6px }
.scribe-card-section { display:flex; flex-direction:column; gap:2px }
.scribe-card-items { margin:0; padding-left:18px; display:flex; flex-direction:column; gap:2px; font-size:13px; overflow-wrap:anywhere }
.scribe-card-counts { margin:0; font-size:12px; color:var(--fg-mute) }
.scribe-tag-entry { display:flex; flex-direction:column; gap:4px }

.scribe-agreement { display:flex; flex-direction:column; gap:8px }
.scribe-agreement-row { overflow-wrap:anywhere }
.scribe-entry { padding:10px 12px; border:1px solid var(--border); border-radius:var(--radius-sm); background:var(--bg); display:flex; flex-direction:column; gap:6px }
.scribe-fields { margin:0; display:grid; grid-template-columns:auto minmax(0,1fr); gap:2px 10px; font-size:13px }
.scribe-fields dt { color:var(--fg-mute) }
.scribe-fields dd { margin:0; overflow-wrap:anywhere }

.scribe-tag-list { display:flex; flex-direction:column; gap:8px; margin-bottom:16px }
.scribe-tag-row strong { font-family:var(--font-mono); font-size:13px }
.scribe-tag-form { display:grid; grid-template-columns:repeat(auto-fit, minmax(min(100%, 240px), 1fr)); gap:10px 16px; padding-top:16px; border-top:1px solid var(--hair) }
.scribe-tag-form > h3, .scribe-tag-form > .scribe-actions, .scribe-tag-form > p, .scribe-tag-form > [role=status] { grid-column:1 / -1 }
.scribe-tag-form label { display:flex; flex-direction:column; gap:4px; font-size:13px; color:var(--fg-dim) }
.scribe-tag-form label:has(textarea) { grid-column:1 / -1 }
`;
