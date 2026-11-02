/**
 * Source screen styles, over the shared panels (static/style.css). The
 * transcript and the aside sit side by side and wrap to one column on a
 * phone; everything fits from 320 px with no horizontal scroll.
 */
export const styles = `
.source-page { display:flex; flex-direction:column; gap:20px; min-width:0; max-width:1120px }
.source-page >*,.source-page section,.source-page form,.source-page aside { min-width:0 }
.source-page .panel + .panel { margin-top:0 }
.source-head { display:flex; flex-direction:column; gap:6px }
.source-head h1 { margin:0; overflow-wrap:anywhere }
.source-head .intro { margin:0; font-size:14px; color:var(--fg-dim) }
.source-back { font-size:13px; text-decoration:none; align-self:flex-start }
.source-actions label { flex:1 1 240px; min-width:0 }
.source-actions input { display:block; width:100%; min-width:0 }
.source-page h2 { margin-bottom:12px }
.source-page .panel-head h2 { margin-bottom:0 }
.source-page button,.source-page input,.source-page textarea,.source-page select { font:inherit; max-width:100% }
.source-page button { border:1px solid var(--border); border-radius:var(--radius-sm); min-height:36px; padding:0 12px; background:transparent; cursor:pointer; color:var(--fg); font-size:14px; align-self:flex-start }
.source-page button:hover { background:var(--bg-hover) }
.source-page button:disabled { opacity:.6; cursor:default }
.source-page input,.source-page textarea,.source-page select { border:1px solid var(--border); border-radius:var(--radius-sm); padding:8px 10px; background:var(--bg-elev); color:inherit }
.source-page label { display:block; margin:8px 0 }
.source-page label input[type=checkbox] { margin-right:8px }
.source-drop { border:1.5px dashed var(--border-strong); border-radius:var(--radius); padding:24px; background:transparent; color:var(--fg-dim) }
.source-more > summary { cursor:pointer; color:var(--fg-dim); min-height:40px; display:flex; align-items:center }
.source-more[open] > .source-drop { margin-top:12px }
.source-actions { display:flex; flex-wrap:wrap; gap:12px; margin:12px 0 }
.source-paste { display:grid; gap:8px; margin-top:16px }
.source-paste textarea { width:100%; min-height:100px }
.source-message { color:var(--fg-mute); white-space:pre-wrap }
.source-plan { padding:20px; border-radius:var(--radius); background:var(--bg-elev); border:1px solid var(--border) }

.source-view { display:flex; flex-direction:column; gap:20px }
.source-notice:empty { display:none }
.source-notice p { margin:0; color:var(--attention-soft) }

.source-player { border:1px solid var(--border); border-radius:var(--radius); background:var(--bg-elev); padding:14px 18px; display:flex; flex-direction:column; gap:12px }
.source-player > p { margin:0; color:var(--fg-dim); font-size:14px }
.source-player video,.source-player audio,.source-player img { width:100%; max-height:420px; object-fit:contain; background:var(--bg); border-radius:var(--radius-sm) }
.source-player audio { background:transparent }
.source-timeline { display:flex; flex-wrap:wrap; align-items:center; gap:8px 16px }
.source-readout { flex:none; font-size:13px; color:var(--fg-dim) }
.source-track { flex:1 1 200px; height:28px; position:relative; display:flex; align-items:center }
.source-rail { width:100%; height:4px; border-radius:2px; background:var(--border) }
.source-fill { position:absolute; left:0; width:0; height:4px; border-radius:2px; background:var(--accent) }
.source-mark { position:absolute; top:6px; width:4px; height:16px; margin-left:-2px; border-radius:2px; background:var(--accent) }
.source-mark[data-kind=aside] { background:var(--fg-mute) }
.source-mark[data-kind=gotcha] { background:var(--attention) }
.source-mark:hover,.source-mark:focus-visible { outline:2px solid var(--accent-hover); outline-offset:2px }

.source-columns { display:flex; flex-wrap:wrap; gap:20px; align-items:flex-start }
.source-main { flex:999 1 520px; min-width:0; display:flex; flex-direction:column; gap:20px }
.source-aside { flex:1 1 280px; min-width:0; display:flex; flex-direction:column; gap:16px }
.source-page .panel-head > .source-tray { display:flex; flex-direction:row; flex-wrap:wrap; align-items:center; gap:8px }
.source-tray button { min-height:32px; font-size:13px }
.source-main .panel-body { padding-left:0; padding-right:0; padding-bottom:0 }
.source-main .panel-body > p { padding:0 20px }
.source-text { max-height:640px; overflow:auto; line-height:1.55 }
.source-line { display:flex; gap:14px; padding:10px 18px; scroll-margin:100px; border-top:1px solid var(--hair) }
.source-line:first-child { border-top:0 }
.source-blank { padding:0; border-top:0 }
.source-stamp { flex:0 0 64px; font-family:var(--font-mono); font-size:12px; text-decoration:none; padding-top:2px }
.source-lineno { color:var(--fg-mute) }
.source-turn-body { display:flex; flex-direction:column; gap:4px; flex:1; min-width:0 }
.source-who { font-size:12px; color:var(--fg-mute) }
.source-said { font-size:14px; white-space:pre-wrap; overflow-wrap:anywhere; flex:1; min-width:0 }
.source-tags { display:flex; flex-wrap:wrap; gap:6px }
.source-page .source-line[data-highlight=true] { background:var(--highlight-bg); box-shadow:inset 3px 0 0 var(--highlight-line) }
.source-page [data-highlight=true]:not(.source-line) { background:var(--highlight-bg); outline:1px solid var(--highlight-line); border-radius:var(--radius-sm) }
.source-main .panel-body > .source-chapter, .source-main .panel-body > .source-gotcha, .source-main .panel-body > .source-frames { margin-left:18px; margin-right:18px }
.source-chapter { margin:0 0 8px; padding:10px 12px; border:1px solid var(--border); border-radius:var(--radius-sm) }
.source-chapter a { text-decoration:none }
.source-gotcha { margin:8px 0 0; color:var(--attention-fg); background:var(--attention-bg); padding:10px 12px; border-radius:var(--radius-sm) }
.source-main .panel-body > :last-child { margin-bottom:18px }
.source-frames { display:flex; gap:12px; overflow:auto; padding:0 0 8px }
.source-frame { flex:0 0 145px; margin:0 }
.source-frame img { width:145px; height:85px; object-fit:cover; border-radius:6px }
.source-frame figcaption { font-size:12px; font-family:var(--font-mono); color:var(--fg-mute) }

.source-aside .panel-body > p { margin:0; font-size:13px; line-height:1.5; color:var(--fg-dim) }
.source-citers { gap:0 }
.source-citer { display:flex; flex-direction:column; gap:2px; padding:8px 0 }
.source-citer + .source-citer { border-top:1px solid var(--hair) }
.source-citer-kind { font-size:12px; color:var(--fg-mute) }
.source-citer a { font-size:14px; color:var(--fg); text-decoration:none; overflow-wrap:anywhere }
.source-citer a:hover { color:var(--accent-hover) }
.source-citation:empty { display:none }
.source-citation { padding-top:10px; border-top:1px solid var(--hair) }
.source-citation h3 { margin:0 0 6px; font-size:14px }
.source-citation-text { margin:0; font-size:13px; white-space:pre-wrap; overflow-wrap:anywhere; color:var(--fg-dim) }
.source-routes { display:flex; flex-wrap:wrap; gap:6px }
.source-sharing:empty { display:none }

@media(max-width:640px) {
  .source-drop { padding:16px }
  .source-player { padding:12px }
  .source-line { gap:10px; padding:10px 16px }
  .source-stamp { flex-basis:58px }
}
`;
