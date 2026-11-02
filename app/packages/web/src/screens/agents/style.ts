/** Agent work: the header controls, workspace chips and the grouped task rows. */
export const styles = `
.agents-header-meta { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; font-size: 13px; color: var(--fg-mute); }
.agents-header-meta a { display: inline-flex; align-items: center; min-height: 40px; padding: 0 14px; border: 1px solid var(--border-strong); border-radius: var(--radius-sm); color: var(--fg); text-decoration: none; font-size: 14px; }
.agents-header-meta a:hover { background: var(--bg-hover); }
.agents .tile-row { margin-bottom: 20px; }
.agents-summary-link { color: inherit; text-decoration: none; display: flex; }
.agents-summary-link .tile { flex: 1; }
.agents-summary-link:hover .tile { border-color: var(--border-strong); }
.agents-filter { display: flex; flex-wrap: wrap; gap: 8px; margin: 0 0 20px; }
.agents-chip { min-height: 36px; padding: 0 12px; border: 1px solid var(--border); border-radius: 999px; background: transparent; color: var(--fg-dim); font-size: 13px; cursor: pointer; }
.agents-chip:hover { background: var(--bg-hover); color: var(--fg); }
.agents-chip[aria-pressed="true"] { background: var(--bg-hover); border-color: var(--border-strong); color: var(--fg); font-weight: 500; }
.agents-group { margin: 0 0 20px; }
.agents-group-head { display: flex; align-items: center; gap: 10px; padding: 14px 20px 6px; }
.agents-group-head h2 { font-size: 11px; font-weight: 500; letter-spacing: 0.08em; text-transform: uppercase; color: var(--fg-mute); }
.agents-list { display: flex; flex-direction: column; padding: 0 20px 6px; }
.agent-row { display: flex; align-items: center; gap: 12px; min-height: 48px; padding: 12px 0; min-width: 0; }
.agent-row + .agent-row { border-top: 1px solid var(--hair); }
.agent-main { flex: 1 1 240px; min-width: 0; display: flex; flex-direction: column; gap: 4px; }
.agent-main h3 { font-size: 14px; font-weight: 500; }
.agent-meta { margin: 0; }
.agent-detail { display: flex; flex-wrap: wrap; gap: 6px; }
.agent-detail:empty { display: none; }
.agent-trail { flex: none; display: flex; flex-wrap: wrap; align-items: center; justify-content: flex-end; gap: 12px; }
.agents-show-more { margin: 0 20px 14px; min-height: 36px; padding: 0 12px; border: 1px solid var(--border); border-radius: var(--radius-sm); background: transparent; color: var(--fg); font-size: 13px; cursor: pointer; }
.agents-show-more:hover { background: var(--bg-hover); }
@media (max-width: 600px) {
  .agent-row { flex-wrap: wrap; align-items: flex-start; }
  .agent-row > .dot { margin-top: 6px; }
  .agent-trail { justify-content: flex-start; padding-left: 20px; }
  .agents-group-head, .agents-list { padding-left: 16px; padding-right: 16px; }
}
`;
