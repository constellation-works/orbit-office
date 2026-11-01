import { atom, read, update } from 'claude-code'
import type { EngineInterface, PluginOptions, ProcessRunResult, Register } from 'claude-code'

import type { PaneState, ScribeEntry } from '../contract'

// The Claude desktop mod pane for the scribe (docs/design/office/specs/
// recall.md, "The Scribe window"): the current session's captured entries,
// its status and controls, inline corrections and an ask box, beside the
// chat. It reaches Office only through `orbit tool run office.*` run with
// `$.process`, plus the plugin's own structured pane control
// (scripts/pane.py) for the switch, Capture now, title and the
// window-open heartbeat. It holds no Library path or credentials, and it
// fails quietly: when Orbit is unreachable it shows one line and never
// loops errors into the session.

const PANE = 'scribe'
const REFRESH_MS = 30_000
const ASK_TIMEOUT_MS = 180_000
const PAGE = 15
const KINDS = ['decision', 'direction', 'fact', 'action', 'open', 'resolved', 'handoff'] as const
const MISSED_KINDS = ['direction', 'decision', 'fact'] as const

const EMPTY: PaneState = {
  entries: [],
  title: null,
  on: null,
  lastPass: null,
  answer: '',
  asking: false,
  missedKind: 'direction',
  unavailable: false,
  notice: '',
  expanded: null,
  kindFilter: 'all',
  shown: PAGE,
}

// Change the tag when PaneState's fields change: it declines what an earlier
// version stored, which outlives a reload.
const paneState = atom({ plugin: 'office-scribe', key: 'scribe-pane' } as const, EMPTY, {
  shape: 'scribe-pane-3',
})

type Config = { workspace: string; defaultOn: boolean; excluded: string; webUrl: string }
let config: Config = { workspace: '', defaultOn: false, excluded: '[]', webUrl: '' }

// Subagent answers arrive on that agent's own turn.complete.
const asked = new Map<string, (answer: string) => void>()
let refreshing: Promise<void> | undefined

function parseReply(ran: ProcessRunResult): Record<string, unknown> {
  if (ran.exitCode !== 0) throw new Error('process_failed')
  const value: unknown = JSON.parse(ran.stdout)
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new Error('invalid_reply')
  }
  const record = value as Record<string, unknown>
  if (record.ok === false || record.error) throw new Error('tool_refused')
  // Tolerate the exec response envelope, like scripts/process.py does.
  if ('schema_version' in record && typeof record.result === 'object' && record.result !== null) {
    return record.result as Record<string, unknown>
  }
  return record
}

async function runOrbit($: EngineInterface, tool: string, input: unknown): Promise<Record<string, unknown>> {
  if (!config.workspace) throw new Error('configure_local_office_workspace')
  const argv = ['orbit', 'tool', 'run', tool, '--json', '--full', '--workspace', config.workspace]
  argv.push('--input', JSON.stringify(input))
  return parseReply(await $.process.run(argv, { cwd: config.workspace, timeoutMs: 20_000 }))
}

// The same structured arguments the `scribe` MCP tool takes. The pane
// control reads the plugin's options from the environment the way the
// command hooks receive them.
async function runPaneControl($: EngineInterface, args: Record<string, unknown>): Promise<Record<string, unknown>> {
  if (!config.workspace) throw new Error('configure_local_office_workspace')
  const argv = ['python3', '-I', `${$.plugin.root}/scripts/pane.py`, JSON.stringify(args)]
  return parseReply(
    await $.process.run(argv, {
      cwd: config.workspace,
      timeoutMs: 40_000,
      env: {
        CLAUDE_PLUGIN_OPTION_WORKSPACE: config.workspace,
        CLAUDE_PLUGIN_OPTION_DEFAULT_ON: String(config.defaultOn),
        CLAUDE_PLUGIN_OPTION_EXCLUDED_PROJECTS: config.excluded,
      },
    }),
  )
}

// Window-open heartbeat, kept while the pane is shown. Stub: until
// office.scribe-control (ORB-20755, ORB-20756) carries the heartbeat and the
// shared switch, this refreshes the plugin's local
// `.orbit/tmp/scribe/<session-id>.window-open` file, which the Stop hook reads.
async function heartbeat($: EngineInterface, sessionId: string): Promise<void> {
  const shown = (await $.ui.panes()).some(pane => pane.id === PANE && pane.isPlaced)
  if (shown) await runPaneControl($, { session_id: sessionId, action: 'heartbeat' })
}

function line(entry: ScribeEntry): number {
  return Number(/^#L(\d+)$/.exec(entry.anchor)?.[1] ?? 0)
}

function newestFirst(entries: ScribeEntry[]): ScribeEntry[] {
  return [...entries].sort(
    (a, b) => String(b.time).localeCompare(String(a.time)) || line(b) - line(a),
  )
}

function clock(iso: string): string {
  const at = new Date(iso)
  if (Number.isNaN(at.getTime())) return iso
  return `${String(at.getHours()).padStart(2, '0')}:${String(at.getMinutes()).padStart(2, '0')}`
}

function ago(iso: string | null, now: number): string {
  if (!iso) return 'no pass yet'
  const minutes = Math.round((now - Date.parse(iso)) / 60_000)
  if (!Number.isFinite(minutes)) return 'no pass yet'
  if (minutes < 1) return 'last pass just now'
  if (minutes < 120) return `last pass ${minutes} min ago`
  return `last pass ${Math.round(minutes / 60)} h ago`
}

// One refresh at a time; a caller during one waits for it.
function refresh($: EngineInterface): Promise<void> {
  refreshing ??= load($).finally(() => {
    refreshing = undefined
  })
  return refreshing
}

async function load($: EngineInterface): Promise<void> {
  try {
    const sessionId = await $.session.id()
    const data = await runOrbit($, 'office.recall', { question: sessionId, limit: 50 })
    const sessions = Array.isArray(data.sessions) ? (data.sessions as Record<string, unknown>[]) : []
    // Recall matches the id as a search term; keep only this session's sources.
    const mine = sessions.filter(s => s.session_id === sessionId)
    const sources = new Set(mine.map(s => s.source_id))
    const all = Array.isArray(data.entries) ? (data.entries as ScribeEntry[]) : []
    const entries = newestFirst(all.filter(entry => sources.has(entry.source_id)))
    let status: Record<string, unknown> = {}
    try {
      status = await runPaneControl($, { session_id: sessionId, action: 'status' })
    } catch {
      // Status is best-effort; the feed still stands.
    }
    try {
      await heartbeat($, sessionId)
    } catch {
      // The heartbeat is best-effort; the pane still works without it.
    }
    await update($, paneState, state => ({
      ...state,
      entries,
      title: typeof mine[0]?.title === 'string' ? mine[0].title : state.title,
      on: typeof status.on === 'boolean' ? status.on : state.on,
      lastPass: typeof status.last_queued_at === 'string' ? status.last_queued_at : state.lastPass,
      unavailable: false,
    }))
  } catch {
    // Fail quietly: one line, no error loop, the session stays responsive.
    await update($, paneState, state => ({ ...state, unavailable: true }))
  }
}

// A control press: refreshes on success, one dim notice on failure.
async function act($: EngineInterface, failure: string, run: () => Promise<unknown>): Promise<void> {
  try {
    await run()
    await update($, paneState, state => ({ ...state, notice: '' }))
  } catch {
    await update($, paneState, state => ({ ...state, notice: failure }))
  }
  await refresh($)
}

function correct($: EngineInterface, entry: ScribeEntry, choice: string, extra: Record<string, unknown> = {}) {
  return act($, 'correction not saved', () =>
    runOrbit($, 'office.capture', {
      append_to: entry.source_id,
      entries: [{ kind: 'correction', correction: { anchor: entry.anchor, choice, ...extra } }],
    }),
  )
}

// The same cited evidence the recall agent quotes, for when no subagent
// answer comes back.
async function recallEvidence($: EngineInterface, question: string): Promise<string> {
  const data = await runOrbit($, 'office.recall', { question, limit: 20 })
  const entries = Array.isArray(data.entries) ? (data.entries as ScribeEntry[]) : []
  const sessions = Array.isArray(data.sessions) ? (data.sessions as Record<string, unknown>[]) : []
  if (entries.length === 0) return `not captured; coverage: ${JSON.stringify(data.coverage ?? {})}`
  return entries
    .map(entry => {
      const session = sessions.find(s => s.source_id === entry.source_id)
      const resume = (session?.resume as Record<string, unknown> | null | undefined)?.command
      return (
        `${entry.text} [${entry.source_id}${entry.anchor}]` +
        (session?.title ? ` — ${String(session.title)}` : '') +
        (resume ? `, ${String(resume)}` : '')
      )
    })
    .join('\n')
}

async function ask($: EngineInterface, question: string): Promise<void> {
  if (!question.trim()) return
  await update($, paneState, state => ({ ...state, answer: '', asking: true }))
  const settle = async (answer: string) => {
    await update($, paneState, state => ({ ...state, answer, asking: false }))
  }
  const fallback = async () => {
    try {
      await settle(await recallEvidence($, question))
    } catch {
      await settle('recall unavailable')
    }
  }
  let spawned: Awaited<ReturnType<EngineInterface['agent']['spawn']>>
  try {
    spawned = await $.agent.spawn({
      subagentType: 'office-scribe:recall',
      description: 'Scribe recall',
      prompt: question,
    })
  } catch {
    return fallback()
  }
  const agentId = spawned.agentId
  if (!agentId) return fallback()
  const timer = $.clock.after(ASK_TIMEOUT_MS, () => {
    if (asked.delete(agentId)) void fallback()
  })
  asked.set(agentId, answer => {
    timer.cancel()
    void (answer.trim() ? settle(answer) : fallback())
  })
}

export const register: Register = (on, options: PluginOptions) => {
  config = {
    workspace: typeof options.workspace === 'string' ? options.workspace : '',
    defaultOn: options.default_on === true,
    excluded: typeof options.excluded_projects === 'string' ? options.excluded_projects : '[]',
    webUrl: typeof options.scribe_url === 'string' ? options.scribe_url : '',
  }

  on('session.start', async ($, e, next) => {
    const started = await next(e)
    await $.command.register({
      name: 'scribe-pane',
      description: 'Open the scribe pane for this session',
    })
    void $.ui.open({ id: PANE, title: 'Scribe' })
    $.clock.every(REFRESH_MS, () => void refresh($))
    void refresh($)
    return started
  })

  on('command.run', { command: 'scribe-pane' }, async $ => {
    await $.ui.open({ id: PANE, title: 'Scribe' })
    void refresh($)
    return { text: 'Scribe pane opened.' }
  })

  on('turn.complete', async ($, e, next) => {
    const done = await next(e)
    if (e.agentId) {
      const resolve = asked.get(e.agentId)
      if (resolve) {
        asked.delete(e.agentId)
        resolve(e.answer)
      }
    } else {
      // After each main-loop turn, without holding the session.
      void refresh($)
    }
    return done
  })

  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e) => {
    const table = $.ui.resolve(e)
    const { Box, Text, Button, Link } = table
    // Mobile draws no field yet: its pane shows the feed and the buttons.
    const Input = 'Input' in table ? table.Input : undefined
    const Select = 'Select' in table ? table.Select : undefined
    const state = await read($, paneState)

    if (state.unavailable) {
      return (
        <Box flexDirection="column">
          <Text>scribe unavailable</Text>
          <Button key="retry" label="Retry" onPress={() => refresh($)} />
        </Box>
      )
    }

    const sourceId = state.entries[0]?.source_id
    const now = await $.clock.now()
    const counts = new Map<string, number>()
    for (const entry of state.entries) counts.set(entry.kind, (counts.get(entry.kind) ?? 0) + 1)
    const filtered =
      state.kindFilter === 'all' ? state.entries : state.entries.filter(entry => entry.kind === state.kindFilter)
    const visible = filtered.slice(0, state.shown)
    const switchText = state.on === null ? 'Scribe' : state.on ? 'Scribe on' : 'Scribe off'
    const statusLine =
      switchText +
      (state.title ? ` · ${state.title}` : '') +
      ` · ${ago(state.lastPass, now)}` +
      ` · ${state.entries.length} ${state.entries.length === 1 ? 'entry' : 'entries'}`
    const pick = (patch: Partial<PaneState>) => update($, paneState, s => ({ ...s, ...patch }))

    return (
      <Box flexDirection="column" gap={1}>
        <Box flexDirection="column">
          <Text weight="semibold">{statusLine}</Text>
          {sourceId && <Text dimColor size="caption">{sourceId}</Text>}
        </Box>
        <Box flexDirection="row" gap={1}>
          <Button
            key="toggle"
            label={state.on ? 'Turn off' : 'Turn on'}
            onPress={async () => {
              const sessionId = await $.session.id()
              await act($, 'switch not changed', () =>
                runPaneControl($, { session_id: sessionId, action: state.on ? 'off' : 'on' }),
              )
            }}
          />
          <Button
            key="capture-now"
            label="Capture now"
            onPress={async () => {
              const sessionId = await $.session.id()
              await act($, 'capture not queued', () =>
                runPaneControl($, { session_id: sessionId, action: 'now' }),
              )
            }}
          />
          <Button key="refresh" label="Refresh" onPress={() => refresh($)} />
        </Box>
        {Input && (
          <Input
            key="ask"
            placeholder="Ask: where did we discuss…, what was decided…"
            submitLabel="ask"
            onSubmit={text => ask($, text)}
          />
        )}
        {state.asking && <Text dimColor>Asking the recall agent…</Text>}
        {state.answer !== '' && (
          <Box key="answer" flexDirection="column">
            <Text>{state.answer}</Text>
            <Button key="clear-answer" label="Clear" onPress={() => pick({ answer: '' })} />
          </Box>
        )}
        {state.notice !== '' && <Text dimColor>{state.notice}</Text>}
        {state.entries.length === 0 && <Text dimColor>Nothing captured for this session yet.</Text>}
        {Select && state.entries.length > 0 && (
          <Select
            key="kind-filter"
            label="Show"
            value={state.kindFilter}
            options={[
              { value: 'all', label: `all (${state.entries.length})` },
              ...[...counts].map(([value, count]) => ({ value, label: `${value} (${count})` })),
            ]}
            onSelect={kind => pick({ kindFilter: kind, shown: PAGE, expanded: null })}
          />
        )}
        {visible.map(entry => {
          const isOpen = state.expanded === entry.anchor
          const marks = (entry.corrected ? ' · corrected' : '') + (entry.by ? ` · by ${entry.by}` : '')
          return (
            <Box key={`entry-${entry.anchor}`} flexDirection="column">
              <Box flexDirection="row" gap={1}>
                <Text dimColor size="caption">{`${entry.kind} · ${clock(entry.time)} · ${entry.anchor}${marks}`}</Text>
                <Button
                  key={`edit-${entry.anchor}`}
                  label={isOpen ? 'Done' : 'Correct…'}
                  onPress={() => pick({ expanded: isOpen ? null : entry.anchor })}
                />
              </Box>
              <Text>{entry.text}</Text>
              {isOpen && (
                <Box flexDirection="column">
                  <Box flexDirection="row" gap={1}>
                    <Button
                      key={`not-decision-${entry.anchor}`}
                      label="Not a decision"
                      onPress={() => correct($, entry, 'not_decision')}
                    />
                    <Button
                      key={`remove-${entry.anchor}`}
                      label="Remove"
                      onPress={() => correct($, entry, 'remove')}
                    />
                    {Select && (
                      <Select
                        key={`kind-${entry.anchor}`}
                        label="Kind"
                        value={entry.kind}
                        options={KINDS.map(value => ({ value }))}
                        onSelect={kind => {
                          if (kind !== entry.kind) void correct($, entry, 'kind', { kind })
                        }}
                      />
                    )}
                  </Box>
                  {Input && (
                    <Input
                      key={`wording-${entry.anchor}`}
                      value={entry.text}
                      submitLabel="correct wording"
                      onSubmit={text => {
                        if (text.trim() && text !== entry.text) void correct($, entry, 'wording', { text })
                      }}
                    />
                  )}
                </Box>
              )}
            </Box>
          )
        })}
        {filtered.length > visible.length && (
          <Button
            key="more"
            label={`Show ${Math.min(PAGE, filtered.length - visible.length)} more of ${filtered.length - visible.length}`}
            onPress={() => pick({ shown: state.shown + PAGE })}
          />
        )}
        {Select && Input && sourceId && (
          <Box key="missed" flexDirection="row" gap={1}>
            <Select
              key="missed-kind"
              label="Missed"
              value={state.missedKind}
              options={MISSED_KINDS.map(value => ({ value }))}
              onSelect={kind => pick({ missedKind: kind })}
            />
            <Input
              key="missed"
              placeholder="What the scribe missed, verbatim"
              submitLabel="add"
              onSubmit={text => {
                if (!text.trim()) return
                void act($, 'entry not added', () =>
                  runOrbit($, 'office.capture', {
                    append_to: sourceId,
                    entries: [{ kind: state.missedKind, text, by: 'user' }],
                  }),
                )
              }}
            />
          </Box>
        )}
        {Input && (
          <Input
            key="title"
            label="Title"
            placeholder={state.title ?? 'Set the session title'}
            submitLabel="set title"
            onSubmit={async text => {
              if (!text.trim()) return
              const sessionId = await $.session.id()
              await act($, 'title not set', () =>
                runPaneControl($, { session_id: sessionId, action: 'title', title: text }),
              )
            }}
          />
        )}
        {config.webUrl !== '' && <Link href={config.webUrl} label="Other conversations: Scribe screen" />}
      </Box>
    )
  })
}
