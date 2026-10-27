export const styles = `
.source-page { display:grid; gap:24px; min-width:0 }
.source-page >*,.source-page section,.source-page form { min-width:0 }
.source-actions label { flex:1 1 240px; min-width:0 }
.source-actions input { display:block; width:100%; min-width:0 }
.source-page h2 { margin-bottom:12px }
.source-page button,.source-page input,.source-page textarea,.source-page select { font:inherit; max-width:100% }
.source-page button { border:1px solid #ccdcd3; border-radius:8px; padding:8px 12px; background:#fff; cursor:pointer; color:#225e55 }
.source-page button:disabled { opacity:.6; cursor:default }
.source-page input,.source-page textarea,.source-page select { border:1px solid #b8cac1; border-radius:6px; padding:8px; background:white; color:inherit }
.source-page label { display:block; margin:8px 0 }
.source-page label input[type=checkbox] { margin-right:8px }
.source-drop { border:2px dashed #8cae9e; border-radius:14px; padding:24px; background:#edf4ef }
.source-actions { display:flex; flex-wrap:wrap; gap:12px; margin:12px 0 }
.source-paste { display:grid; gap:8px; margin-top:16px }
.source-paste textarea { width:100%; min-height:100px }
.source-columns { display:grid; grid-template-columns: minmax(0,1fr) minmax(0,1fr); gap:24px; align-items:start }
.source-original video,.source-original audio,.source-original img { width:100%; max-height:420px; object-fit:contain; background:#e4ebe6; border-radius:10px }
.source-text { max-height:600px; overflow:auto; padding:16px; background:#fff; border:1px solid #dce5df; border-radius:12px }
.source-line { white-space:pre-wrap; margin:0; padding:6px; scroll-margin:100px }
.source-page [data-highlight=true] { background:#fff0b5; outline:2px solid #bc7b28; border-radius:4px }
.source-chapter { margin:12px 0; padding:12px; background:#fff; border:1px solid #dce5df; border-radius:10px }
.source-gotcha { color:#7a4119; background:#fff5df; padding:10px; border-radius:6px }
.source-frames { display:flex; gap:12px; overflow:auto; padding:8px 2px; margin-bottom:12px }
.source-frame { flex:0 0 145px; margin:0 }
.source-frame img { width:145px; height:85px; object-fit:cover; border-radius:6px }
.source-frame figcaption { font-size:.85rem }
.source-message { color:#526660; white-space:pre-wrap }
.source-plan { padding:24px; border-radius:14px; background:#fff; border:1px solid #b8cac1 }
@media(max-width:700px) { .source-columns { grid-template-columns:minmax(0,1fr) } .source-drop { padding:16px } }
`;
