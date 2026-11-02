export const styles = `
.routines { display:flex; flex-direction:column; gap:16px; min-width:0; }
.routines .screen-head { margin:0; }
.routines .panel + .panel { margin-top:0; }
.routines-hidden { margin:-4px 0 0; }
.routines-notice { margin:0; font-size:14px; color:var(--fg-dim); }
.routines-total { font-variant-numeric:tabular-nums; }
.routine-hour { padding:16px 20px 8px; }
.routine-hour-head { display:flex; flex-wrap:wrap; align-items:baseline; gap:6px 14px; font-size:12px; color:var(--fg-mute); }
.routine-hour-range { font-family:var(--font-mono); }
.routine-hour-legend { margin-left:auto; display:flex; gap:14px; }
.routine-hour-key { display:inline-flex; align-items:center; gap:6px; }
.routine-hour-key::before { content:""; box-sizing:border-box; width:10px; height:10px; border-radius:50%; border:2px solid var(--fg-mute); background:var(--bg-elev); }
.routine-hour-key--fires::before { background:var(--working); border-color:var(--working); }
.routine-hour-track { position:relative; height:68px; margin:8px 8px 0; }
.routine-hour-axis { position:absolute; left:0; right:0; top:19px; height:2px; background:var(--border); }
.routine-hour-mark { position:absolute; top:12px; transform:translateX(-50%); display:flex; flex-direction:column; align-items:center; gap:6px; max-width:40%; }
.routine-hour-tick { box-sizing:border-box; flex:none; width:16px; height:16px; border-radius:50%; border:2px solid var(--fg-mute); background:var(--bg-elev); }
.routine-hour-mark--fires .routine-hour-tick { background:var(--working); border-color:var(--working); }
.routine-hour-label { font-family:var(--font-mono); font-size:11px; color:var(--fg-mute); white-space:nowrap; overflow:hidden; text-overflow:ellipsis; max-width:100%; }
.routine-hour-mark--fires .routine-hour-label { color:var(--fg); }
.routine-hour-mark--low .routine-hour-label { margin-top:14px; }
.routine-group-head { padding:12px 18px; }
.panel-head .routine-group-title { display:flex; align-items:center; gap:8px; font-size:11px; font-weight:500; letter-spacing:0.08em; text-transform:uppercase; color:var(--fg-mute); }
.routine-group-dot--on { background:var(--done); }
.routine-group-count { font-family:var(--font-mono); letter-spacing:0; color:var(--fg-mute); }
.routine-columns, .routine-row { display:grid; grid-template-columns:56px minmax(0,2.2fr) minmax(0,1.3fr) minmax(0,1fr) minmax(0,1.2fr); gap:12px; align-items:center; padding:0 18px; }
.routine-columns { padding-top:8px; padding-bottom:8px; font-size:12px; color:var(--fg-mute); border-bottom:1px solid var(--hair); }
.routine-row { padding-top:12px; padding-bottom:12px; }
.routine-row + .routine-row { border-top:1px solid var(--hair); }
.routine-cell { display:flex; flex-direction:column; gap:2px; min-width:0; }
.routine-cell-main { font-size:13px; color:var(--fg); overflow-wrap:anywhere; }
.routine-cell-main.muted { color:var(--fg-dim); }
.routine-cell-sub { font-size:12px; color:var(--fg-mute); overflow-wrap:anywhere; }
.routine-cell-sub--mono { font-family:var(--font-mono); }
.routine-identity h3 { font-size:14px; font-weight:500; }
.routine-description { margin:0; font-size:12px; color:var(--fg-mute); }
.routine-note { margin:0; font-size:12px; color:var(--attention); }
.routine-note:empty { display:none; }
.routine-outcome { display:flex; align-items:center; gap:8px; }
.routine-dot { flex:none; width:8px; height:8px; border-radius:50%; background:var(--fg-mute); }
.routine-dot--done { background:var(--done); }
.routine-dot--working { background:var(--working); }
.routine-dot--attention { background:var(--attention); }
.routine-dot--danger { background:var(--danger); }
.routine-empty { margin:0; padding:14px 18px; font-size:13px; color:var(--fg-mute); }
.routine-switch { appearance:none; flex:none; width:40px; height:24px; min-height:0; margin:0; padding:0; border:0; border-radius:12px; background:var(--border-strong); position:relative; cursor:pointer; }
.routine-switch::after { content:""; position:absolute; top:3px; left:3px; width:18px; height:18px; border-radius:9px; background:var(--fg); transition:transform .15s ease; }
.routine-switch:checked { background:var(--done); }
.routine-switch:checked::after { transform:translateX(16px); }
.routine-switch:focus-visible { outline:2px solid var(--accent-hover); outline-offset:2px; }
.routine-switch:disabled { opacity:.6; cursor:default; }
@media (max-width: 760px) {
  .routine-columns { display:none; }
  .routine-row { grid-template-columns:48px minmax(0,1fr); align-items:start; gap:10px 12px; padding-left:16px; padding-right:16px; }
  .routine-row > .routine-cell:not(.routine-cell--control):not(.routine-identity) { grid-column:2; }
  .routine-row [data-label]:not(.routine-identity)::before { content:attr(data-label); font-size:11px; font-weight:500; letter-spacing:.08em; text-transform:uppercase; color:var(--fg-mute); }
  .routine-hour { padding-left:16px; padding-right:16px; }
  .routine-hour-legend { margin-left:0; }
  .routine-hour-label { display:none; }
}
`;
