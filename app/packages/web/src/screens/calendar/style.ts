/** The Calendar screen's own rules, on the shared tokens. One hour is `--hour` tall. */
export const styles = `
.calendar { --hour:72px; --gutter:56px; }
.calendar-header { display:flex; flex-wrap:wrap; justify-content:space-between; align-items:flex-end; gap:12px; margin-bottom:16px; }
.calendar-header .intro { margin:0; }
.calendar-sync { display:flex; flex-wrap:wrap; align-items:center; gap:8px 12px; font-size:13px; color:var(--fg-mute); }
.calendar-sync button, .calendar-nav button, .calendar-days button { background:var(--bg-elev); border:1px solid var(--border); border-radius:var(--radius-sm); padding:6px 12px; min-height:36px; cursor:pointer; }
.calendar-sync span:empty { display:none; }
.calendar-sync button:hover, .calendar-nav button:hover, .calendar-days button:hover { background:var(--bg-hover); }
.calendar-notice { margin:0 0 14px; padding:10px 14px; border:1px solid var(--border); border-radius:var(--radius); background:var(--bg-elev); font-size:14px; color:var(--fg-dim); }
.calendar-notice[data-tone="attention"] { border-color:var(--attention); color:var(--attention-fg); }
.calendar-connect { margin-bottom:16px; }
.calendar-nav { display:flex; flex-wrap:wrap; align-items:center; gap:8px; margin-bottom:12px; }
.calendar-nav-label { font-weight:600; margin-left:4px; }
.calendar-days { display:none; gap:4px; margin-bottom:12px; }
.calendar-days button { flex:1 1 0; min-width:0; padding:6px 2px; font-size:12px; line-height:1.3; }
.calendar-days button[aria-pressed="true"] { border-color:var(--accent); color:var(--fg); background:var(--highlight-bg); }
.calendar-days button[data-today="true"] { font-weight:600; }
.calendar-grid { display:grid; grid-template-columns:var(--gutter) repeat(7, minmax(0,1fr)); background:var(--bg-elev); border:1px solid var(--border); border-radius:var(--radius); overflow:hidden; min-width:0; }
.calendar-col { display:flex; flex-direction:column; min-width:0; border-left:1px solid var(--hair); }
.calendar-col--gutter { border-left:0; }
.calendar-head { height:48px; padding:6px 8px; border-bottom:1px solid var(--hair); font-size:12px; color:var(--fg-mute); display:flex; flex-direction:column; justify-content:center; }
.calendar-head strong { font-size:15px; color:var(--fg); font-weight:600; }
.calendar-col[data-today="true"] .calendar-head { background:var(--highlight-bg); }
.calendar-col[data-today="true"] .calendar-head strong { color:var(--accent-hover); }
.calendar-allday { min-height:var(--allday, 30px); padding:3px 4px; border-bottom:1px solid var(--border); display:flex; flex-direction:column; gap:3px; }
.calendar-col--gutter .calendar-allday { font-size:11px; color:var(--fg-mute); justify-content:center; padding-left:8px; }
.calendar-body { position:relative; height:calc(var(--hours) * var(--hour)); background-image:repeating-linear-gradient(to bottom, transparent 0, transparent calc(var(--hour) - 1px), var(--hair) calc(var(--hour) - 1px), var(--hair) var(--hour)); }
.calendar-hour { position:absolute; right:8px; font-family:var(--font-mono); font-size:11px; color:var(--fg-mute); transform:translateY(-50%); }
.calendar-hour:first-child { transform:none; }
.calendar-margin { position:absolute; top:0; bottom:0; left:0; width:10px; }
.calendar-events { position:absolute; top:0; bottom:0; left:12px; right:4px; }
.calendar-meeting { position:absolute; display:block; overflow:hidden; text-align:left; padding:0; border:1px solid var(--border-strong); border-left:3px solid var(--fg-mute); border-radius:var(--radius-sm); background:var(--bg-hover); font-size:12px; line-height:15px; cursor:pointer; min-height:30px; }
.calendar-meeting:hover, .calendar-meeting:focus-visible { z-index:2; border-color:var(--accent); }
.calendar-meeting[data-tone="attention"] { border-left-color:var(--attention); }
.calendar-meeting[data-tone="working"] { border-left-color:var(--working); }
.calendar-meeting[data-tone="done"] { border-left-color:var(--done); }
.calendar-meeting[data-private="true"] { border-left-style:double; border-left-width:4px; border-left-color:var(--private); }
.calendar-meeting[data-cancelled="true"] { background:transparent; border-style:dashed; color:var(--fg-mute); }
.calendar-meeting-title { font-weight:600; color:var(--fg); max-width:100%; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.calendar-meeting-inner { position:absolute; inset:0; display:flex; flex-direction:column; align-items:flex-start; gap:1px; padding:2px 6px; overflow:hidden; }
.calendar-meeting-inner > * { flex:none; }
.calendar-allday .calendar-meeting-inner { position:static; }
.calendar-meeting-state { color:var(--fg); max-width:100%; overflow-wrap:normal; }
.calendar-meeting[data-narrow="true"] { font-size:11px; }
.calendar-meeting[data-narrow="true"] .calendar-meeting-inner { padding:2px 4px; }
.calendar-meeting-meta { color:var(--fg-mute); max-width:100%; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-family:var(--font-mono); font-size:11px; }
.calendar-struck, .calendar-meeting[data-cancelled="true"] .calendar-meeting-title { text-decoration:line-through; color:var(--fg-mute); }
.calendar-allday .calendar-meeting { position:static; width:100%; min-height:0; }
.calendar-now { position:absolute; left:0; right:0; height:0; border-top:2px solid var(--danger); z-index:3; pointer-events:none; }
.calendar-now::before { content:""; position:absolute; left:-4px; top:-5px; width:8px; height:8px; border-radius:4px; background:var(--danger); }
.calendar-mark { position:absolute; z-index:1; left:2px; width:6px; height:4px; border-radius:1px; background:var(--accent); transform:translateY(-50%); }
.calendar-mark--paused { background:transparent; border:1px solid var(--fg-mute); }
.calendar-band { position:absolute; left:3px; top:0; bottom:0; width:4px; border-radius:2px; background:repeating-linear-gradient(to bottom, var(--accent) 0 3px, transparent 3px 9px); opacity:.7; }
.calendar-band--paused { background:repeating-linear-gradient(to bottom, var(--fg-mute) 0 1px, transparent 1px 9px); border:1px solid var(--fg-mute); opacity:1; }
.calendar-legend { display:flex; flex-wrap:wrap; align-items:center; gap:8px 16px; margin:10px 0 0; font-size:12px; color:var(--fg-mute); }
.calendar-key { display:inline-flex; align-items:center; gap:6px; }
.calendar-key::before { content:""; width:10px; height:4px; border-radius:1px; background:var(--accent); }
.calendar-key--paused::before { background:transparent; border:1px solid var(--fg-mute); }
.calendar-panel { position:fixed; top:0; right:0; bottom:0; width:min(440px, 100vw); overflow-y:auto; z-index:20; background:var(--bg-elev); border-left:1px solid var(--border); padding:20px 22px 40px; display:flex; flex-direction:column; gap:10px; box-shadow:-12px 0 32px rgba(0,0,0,.35); }
.calendar-panel h3 { margin-top:10px; font-size:13px; font-weight:600; letter-spacing:.04em; text-transform:uppercase; color:var(--fg-mute); }
.calendar-panel h4 { margin-top:6px; font-size:14px; }
.calendar-panel ul { margin:0; padding-left:18px; }
.calendar-panel p { margin:0; }
.calendar-panel-head { display:grid; grid-template-columns:minmax(0,1fr) auto; gap:8px; align-items:start; }
.calendar-panel-head h2 { grid-column:1; font-size:20px; }
.calendar-panel-states { grid-column:1; display:flex; flex-wrap:wrap; gap:6px; }
.calendar-close { grid-column:2; grid-row:1 / span 2; background:var(--bg); border:1px solid var(--border); border-radius:var(--radius-sm); padding:6px 12px; cursor:pointer; }
.calendar-facts { display:grid; grid-template-columns:auto minmax(0,1fr); gap:4px 14px; margin:4px 0 0; font-size:14px; }
.calendar-facts dt { color:var(--fg-mute); }
.calendar-facts dd { margin:0; }
.calendar-attendees { list-style:none; padding:0 !important; display:flex; flex-direction:column; gap:4px; }
.calendar-attendees li { display:flex; flex-wrap:wrap; gap:4px 10px; align-items:baseline; }
.calendar-person { display:inline-flex; flex-direction:column; }
.calendar-person-page { font-size:13px; color:var(--fg-dim); margin-top:2px !important; }
.calendar-link { background:none; border:0; padding:0; color:var(--accent); text-decoration:underline; cursor:pointer; text-align:left; }
.calendar-muted { color:var(--fg-mute); font-size:13px; }
.calendar-agenda { margin:0; padding:8px 12px; border-left:3px solid var(--border-strong); background:var(--bg); white-space:pre-wrap; font-size:13px; color:var(--fg-dim); }
.calendar-cite { font-size:12px; margin-left:4px; color:var(--fg-mute); }
a.calendar-cite { color:var(--accent); }
.calendar-brief { display:flex; flex-direction:column; gap:6px; font-size:14px; }
.calendar-summary-text { white-space:pre-wrap; }
.calendar-capture { display:flex; flex-direction:column; gap:8px; }
.calendar-field { display:flex; flex-direction:column; gap:4px; font-size:13px; color:var(--fg-mute); }
.calendar-field textarea { width:100%; resize:vertical; }
.calendar-capture .primary-action { align-self:flex-start; }
.calendar-drop { display:block; padding:14px; border:1px dashed var(--border-strong); border-radius:var(--radius); font-size:13px; color:var(--fg-dim); cursor:pointer; }
.calendar-drop[data-over="true"] { border-color:var(--accent); background:var(--highlight-bg); }
.calendar-drop input { position:absolute; width:1px; height:1px; opacity:0; }
.calendar-drop-choose { color:var(--accent); text-decoration:underline; }
.calendar-status:empty { display:none; }
.calendar-task { border-style:dashed; border-left-style:solid; background:var(--bg-elev); }
.calendar-task[data-tone="neutral"] { border-left-color:var(--accent); }
.calendar-task-actions { display:flex; flex-direction:column; gap:8px; margin-top:6px; }
.calendar-task-buttons { display:flex; flex-wrap:wrap; gap:8px; }
.calendar-open-todos { align-self:flex-start; margin-top:6px; }
@media (max-width: 820px) {
  .calendar { --gutter:48px; }
  .calendar-days { display:flex; }
  .calendar-grid { grid-template-columns:var(--gutter) minmax(0,1fr); }
  .calendar-col[data-selected="false"] { display:none; }
  .calendar-panel { width:100vw; border-left:0; padding:16px; }
}
`;
