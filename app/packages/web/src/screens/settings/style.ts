export const style = `
.settings-tabs, .settings-actions { display:flex; gap:12px; flex-wrap:wrap; align-items:center; }
.settings-tabs { border-bottom:1px solid var(--border); padding-bottom:16px; margin-bottom:28px; }
.settings button { font:inherit; font-size:14px; cursor:pointer; border:1px solid var(--border); background:transparent; color:var(--fg); border-radius:var(--radius-sm); min-height:40px; padding:0 14px; }
.settings button:hover { background:var(--bg-hover); }
.settings button[aria-pressed="true"] { background:var(--bg-hover); border-color:var(--border-strong); }
.settings button[type="submit"] { background:var(--accent); color:var(--on-accent); border-color:var(--accent); font-weight:600; }
.settings button:disabled { opacity:.65; cursor:default; }
.settings .settings-heading { display:flex; gap:16px; justify-content:space-between; align-items:center; margin-bottom:16px; }
.settings .settings-heading h2 { font-size:17px; }
.settings .card-grid { margin:20px 0; }
.settings .card p { margin:0; }
.settings .settings-meta { color:var(--fg-mute); font-size:13px; }
.settings .settings-url { overflow-wrap:anywhere; }
.settings-form { background:var(--bg-elev); border:1px solid var(--border); border-radius:var(--radius); padding:20px; margin:20px 0; }
.settings-fields { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:18px; margin:20px 0; }
.settings-field { display:flex; flex-direction:column; gap:6px; font-size:13px; color:var(--fg-dim); min-width:0; }
.settings-field input, .settings-field select { font:inherit; font-size:14px; color:var(--fg); background:var(--bg); width:100%; min-width:0; border:1px solid var(--border); border-radius:var(--radius-sm); padding:8px 10px; }
.settings input:focus-visible, .settings select:focus-visible { outline:2px solid var(--accent-hover); outline-offset:2px; }
.settings fieldset { border:1px solid var(--border); border-radius:8px; margin:0; padding:12px; min-width:0; }
.settings fieldset label { display:inline-flex; align-items:center; gap:6px; margin:4px 16px 4px 0; }
.settings .settings-wide { grid-column:1/-1; }
.settings .settings-field[hidden], .settings [hidden] { display:none; }
.settings .settings-hint { color:var(--fg-mute); font-size:12px; }
.settings .settings-feedback { margin:16px 0; }
.settings .settings-feedback:empty { display:none; }
.settings .api-calls { display:grid; gap:12px; padding:0; list-style:none; }
.settings .api-call { background:var(--bg-elev); border:1px solid var(--border); border-radius:var(--radius); padding:16px 18px; }
.settings .api-call header { display:flex; justify-content:space-between; gap:12px; flex-wrap:wrap; }
.settings .api-call h3 { font-size:14px; font-family:var(--font-mono); overflow-wrap:anywhere; margin:12px 0; }
.settings .api-call p { margin:6px 0 0; }
.settings .settings-code { font-family:var(--font-mono); font-size:22px; font-weight:600; letter-spacing:.08em; margin:12px 0; overflow-wrap:anywhere; }
.settings .settings-signin p { margin:8px 0; }
.settings .settings-attention { color:var(--fg); font-weight:600; }
.settings .settings-cli { margin-top:40px; padding-top:20px; border-top:1px solid var(--border); }
.settings .settings-honest, .settings .settings-destination, .settings .settings-preview { margin:12px 0; }
.settings .capability-notes { margin:16px 0; padding-left:1.2rem; }
.settings .capability-notes li { margin:8px 0; }
.settings .settings-attribution { color:var(--private); font-weight:500; margin-top:20px; }
.settings .settings-field input.settings-url { overflow-wrap:anywhere; }
@media(max-width:600px) { .settings-fields { grid-template-columns:1fr; } .settings-form { padding:20px; } .settings .settings-heading { align-items:flex-start; flex-direction:column; } }
`;
