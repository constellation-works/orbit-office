export const style = `
.settings-tabs, .settings-actions { display:flex; gap:12px; flex-wrap:wrap; align-items:center; }
.settings-tabs { border-bottom:1px solid #dce5df; padding-bottom:16px; margin-bottom:28px; }
.settings button { font:inherit; cursor:pointer; border:1px solid #bdcfc5; background:white; color:#225e55; border-radius:8px; padding:9px 15px; font-weight:650; }
.settings button[aria-pressed="true"], .settings button[type="submit"] { background:#225e55; color:white; border-color:#225e55; }
.settings button:disabled { opacity:.65; cursor:default; }
.settings .settings-heading { display:flex; gap:16px; justify-content:space-between; align-items:center; margin-bottom:16px; }
.settings .settings-heading h2 { font-size:1.4rem; }
.settings .card-grid { margin:20px 0; }
.settings .card p { margin:0; }
.settings .settings-meta { color:#526660; font-size:.9rem; }
.settings .settings-url { overflow-wrap:anywhere; }
.settings-form { background:#fff; border:1px solid #dce5df; border-radius:14px; padding:24px; margin:20px 0; }
.settings-fields { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:18px; margin:20px 0; }
.settings-field { display:flex; flex-direction:column; gap:6px; font-weight:600; min-width:0; }
.settings-field input, .settings-field select { font:inherit; font-weight:400; color:#203035; background:#fff; width:100%; min-width:0; border:1px solid #9cb4a8; border-radius:7px; padding:10px; }
.settings input:focus-visible, .settings select:focus-visible { outline:3px solid #b9712e; outline-offset:2px; }
.settings fieldset { border:1px solid #dce5df; border-radius:8px; margin:0; padding:12px; min-width:0; }
.settings fieldset label { display:inline-flex; align-items:center; gap:6px; margin:4px 16px 4px 0; }
.settings .settings-wide { grid-column:1/-1; }
.settings .settings-field[hidden], .settings [hidden] { display:none; }
.settings .settings-hint { color:#526660; font-weight:400; font-size:.9rem; }
.settings .settings-feedback { margin:16px 0; }
.settings .settings-feedback:empty { display:none; }
.settings .api-calls { display:grid; gap:12px; padding:0; list-style:none; }
.settings .api-call { background:white; border:1px solid #dce5df; border-radius:12px; padding:20px; }
.settings .api-call header { display:flex; justify-content:space-between; gap:12px; flex-wrap:wrap; }
.settings .api-call h3 { font-size:1rem; overflow-wrap:anywhere; margin:12px 0; }
.settings .api-call p { margin:6px 0 0; }
.settings .settings-cli { margin-top:40px; padding-top:20px; border-top:1px solid #dce5df; }
.settings .settings-honest, .settings .settings-destination, .settings .settings-preview { margin:12px 0; }
.settings .capability-notes { margin:16px 0; padding-left:1.2rem; }
.settings .capability-notes li { margin:8px 0; }
.settings .settings-attribution { font-weight:650; margin-top:20px; }
.settings .settings-field input.settings-url { overflow-wrap:anywhere; }
@media(max-width:600px) { .settings-fields { grid-template-columns:1fr; } .settings-form { padding:20px; } .settings .settings-heading { align-items:flex-start; flex-direction:column; } }
`;
