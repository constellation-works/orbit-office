import { atom, read, update } from 'claude-code'
import type { Register } from 'claude-code'

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

export type ScribeEntry = {
  source_id: string
  anchor: string
  kind: string
  text: string
  time: string
  corrected?: boolean
  by?: string
}

export type PaneState = {
  entries: ScribeEntry[]
  title: string | null
  on: boolean | null
  lastPass: string | null
  answer: string
  missedKind: string
  unavailable: boolean
  notice: string
}

const EMPTY: PaneState = {
  entries: [],
  title: null,
  on: null,
  lastPass: null,
  answer: '',
  missedKind: 'fact',
  unavailable: false,
  notice: '',
}

const paneState = atom(
  { plugin: 'office-scribe', key: 'scribe-pane' } as const,
  EMPTY,
  { shape: 'scribe-pane-1' },
)

function parseProcessJson(result: unknown): Record<string, unknown> {
  let value: unknown = result
  if (typeof value === 'object' && value !== null) {
    const record = value as Record<string, unknown>
    if (typeof record.code === 'number' && record.code !== 0) {
      throw new Error('process_failed')
    }
    if (typeof record.stdout === 'string') value = record.stdout
    else if ('value' in record) value = record.value
  }
  if (typeof value === 'string') value = JSON.parse(value)
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

async function runOrbit($: any, tool: string, input: unknown): Promise<Record<string, unknown>> {
  const workspace = await $.env.get('CLAUDE_PLUGIN_OPTION_WORKSPACE')
  const argv = ['orbit', 'tool', 'run', tool, '--json', '--full']
  if (typeof workspace === 'string' && workspace) argv.push('--workspace', workspace)
  argv.push('--input', JSON.stringify(input))
  return parseProcessJson(await $.process.run(argv))
}

async function runPaneControl($: any, args: Record<string, unknown>): Promise<Record<string, unknown>> {
  const root = await $.env.get('CLAUDE_PLUGIN_ROOT')
  const base = typeof root === 'string' && root ? root : '.'
  return parseProcessJson(
    await $.process.run(['python3', '-I', `${base}/scripts/pane.py`, JSON.stringify(args)]),
  )
}

function newestFirst(entries: ScribeEntry[]): ScribeEntry[] {
  return [...entries].sort((a, b) => String(b.time).localeCompare(String(a.time)))
}

async function refresh($: any): Promise<void> {
  try {
    const sessionId = await $.session.id()
    const data = await runOrbit($, 'office.recall', { question: String(sessionId), limit: 50 })
    const entries = newestFirst(Array.isArray(data.entries) ? (data.entries as ScribeEntry[]) : [])
    const sessions = Array.isArray(data.sessions) ? (data.sessions as Record<string, unknown>[]) : []
    const mine = sessions.find(s => s.session_id === sessionId) ?? sessions[0]
    let status: Record<string, unknown> = {}
    try {
      status = await runPaneControl($, { session_id: String(sessionId), action: 'status' })
    } catch {
      status = {}
    }
    try {
      await runPaneControl($, { session_id: String(sessionId), action: 'heartbeat' })
    } catch {
      // The heartbeat is best-effort; the pane still works without it.
    }
    await update($, paneState, (state: PaneState) => ({
      ...state,
      entries,
      title: typeof mine?.title === 'string' ? mine.title : state.title,
      on: typeof status.on === 'boolean' ? status.on : state.on,
      lastPass: typeof status.last_queued_at === 'string' ? status.last_queued_at : state.lastPass,
      unavailable: false,
    }))
  } catch {
    // Fail quietly: one line, no error loop, the session stays responsive.
    await update($, paneState, (state: PaneState) => ({ ...state, unavailable: true }))
  }
  try {
    await $.ui.invalidate('ui.render')
  } catch {
    // No surface attached yet; the next render reads current state.
  }
}

async function correct($: any, entry: ScribeEntry, choice: string, extra: Record<string, unknown> = {}): Promise<void> {
  await runOrbit($, 'office.capture', {
    append_to: entry.source_id,
    entries: [{ kind: 'correction', correction: { anchor: entry.anchor, choice, ...extra } }],
  })
  await refresh($)
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    try {
      await $.command.register({
        name: 'scribe-pane',
        description: 'Open the scribe pane for this session',
      })
    } catch {
      // The pane command is a convenience; opening below still works.
    }
    void $.ui.open({ id: PANE, title: 'Scribe' })
    try {
      await $.clock.every(30000, () => refresh($))
    } catch {
      // No timer on this surface; turn-end refresh below still applies.
    }
    await refresh($)
    return next(e)
  })

  on('command.run', { command: 'scribe-pane' }, async $ => {
    await $.ui.open({ id: PANE, title: 'Scribe' })
    await refresh($)
    return { text: 'Scribe pane opened.' }
  })

  on('turn.complete', async ($, e, next) => {
    await refresh($)
    return next(e)
  })

  on('session.end', async ($, e, next) => {
    try {
      await $.ui.close({ id: PANE })
    } catch {
      // The session is ending either way.
    }
    return next(e)
  })

  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e) => {
    const { Box, Text, Button, Input, Select, Link } = $.ui.resolve(e)
    const state = await read($, paneState)

    if (state.unavailable) {
      return (
        <Box flexDirection="column">
          <Text>scribe unavailable</Text>
          <Button
            key="retry"
            label="Retry"
            onPress={async () => {
              await refresh($)
            }}
          />
        </Box>
      )
    }

    const sourceId = state.entries[0]?.source_id
    const statusLine =
      `Scribe ${state.on === null ? '' : state.on ? 'on' : 'off'}` +
      (state.title ? ` — ${state.title}` : '') +
      (state.lastPass ? ` — last pass ${state.lastPass}` : '') +
      ` — ${state.entries.length} entries`

    return (
      <Box flexDirection="column">
        <Text>{statusLine}</Text>
        <Box flexDirection="row">
          <Button
            key="toggle"
            label={state.on ? 'Turn off' : 'Turn on'}
            onPress={async () => {
              const sessionId = await $.session.id()
              await runPaneControl($, {
                session_id: String(sessionId),
                action: state.on ? 'off' : 'on',
              })
              await refresh($)
            }}
          />
          <Button
            key="capture-now"
            label="Capture now"
            onPress={async () => {
              const sessionId = await $.session.id()
              await runPaneControl($, { session_id: String(sessionId), action: 'now' })
              await refresh($)
            }}
          />
          <Button
            key="refresh"
            label="Refresh"
            onPress={async () => {
              await refresh($)
            }}
          />
        </Box>
        <Input
          key="title"
          label="Session title"
          placeholder="Set the session title"
          submitLabel="Set title"
          onSubmit={async (text: string) => {
            const sessionId = await $.session.id()
            await runPaneControl($, {
              session_id: String(sessionId),
              action: 'title',
              title: text,
            })
            await refresh($)
          }}
        />
        {state.entries.length === 0 && <Text dimColor>No captured entries yet.</Text>}
        {state.entries.map(entry => (
          <Box key={`${entry.source_id}${entry.anchor}`} flexDirection="column">
            <Text>
              {entry.kind} {entry.time} {entry.source_id}
              {entry.anchor}
              {entry.corrected ? ' (corrected)' : ''}
              {entry.by ? ` (by: ${entry.by})` : ''}: {entry.text}
            </Text>
            <Box flexDirection="row">
              <Button
                key={`not-decision-${entry.anchor}`}
                label="Not a decision"
                onPress={async () => {
                  await correct($, entry, 'not_decision')
                }}
              />
              <Button
                key={`remove-${entry.anchor}`}
                label="Remove"
                onPress={async () => {
                  await correct($, entry, 'remove')
                }}
              />
            </Box>
            <Input
              key={`wording-${entry.anchor}`}
              label="Wrong wording"
              placeholder="Replacement wording"
              submitLabel="Correct wording"
              onSubmit={async (text: string) => {
                await correct($, entry, 'wording', { text })
              }}
            />
            <Select
              key={`kind-${entry.anchor}`}
              label="Wrong kind"
              options={[
                { value: 'decision' },
                { value: 'direction' },
                { value: 'fact' },
                { value: 'action' },
                { value: 'open' },
                { value: 'resolved' },
                { value: 'handoff' },
              ]}
              onSelect={async (kind: string) => {
                await correct($, entry, 'kind', { kind })
              }}
            />
          </Box>
        ))}
        <Select
          key="missed-kind"
          label="Missed kind"
          options={[{ value: 'direction' }, { value: 'decision' }, { value: 'fact' }]}
          value={state.missedKind}
          onSelect={async (kind: string) => {
            await update($, paneState, (s: PaneState) => ({ ...s, missedKind: kind }))
            await $.ui.invalidate('ui.render')
          }}
        />
        <Input
          key="missed"
          label="Missed something…"
          placeholder="What the scribe missed"
          submitLabel="Add entry"
          onSubmit={async (text: string) => {
            if (!sourceId) return
            await runOrbit($, 'office.capture', {
              append_to: sourceId,
              entries: [{ kind: state.missedKind, text, by: 'user' }],
            })
            await refresh($)
          }}
        />
        <Input
          key="ask"
          label="Ask the scribe"
          placeholder="Ask about captured conversations"
          submitLabel="Ask"
          onSubmit={async (text: string) => {
            let answer = ''
            try {
              const spawned: any = await $.agent.spawn({
                subagentType: 'office-scribe:recall',
                description: 'Scribe recall',
                prompt: text,
              })
              const value = spawned?.value ?? spawned
              answer =
                typeof value === 'string'
                  ? value
                  : String(value?.text ?? value?.result ?? value?.answer ?? '')
            } catch {
              answer = ''
            }
            if (!answer) {
              // No subagent answer (for example a surface without one):
              // render the same cited evidence the recall agent quotes,
              // through office.recall directly.
              try {
                const data = await runOrbit($, 'office.recall', { question: text, limit: 20 })
                const entries = Array.isArray(data.entries) ? data.entries : []
                const sessions = Array.isArray(data.sessions)
                  ? (data.sessions as Record<string, unknown>[])
                  : []
                if (entries.length === 0) {
                  answer =
                    'not captured; coverage: ' +
                    JSON.stringify(data.coverage ?? {}, Object.keys((data.coverage ?? {}) as object).sort())
                } else {
                  answer = (entries as ScribeEntry[])
                    .map(entry => {
                      const session = sessions.find(s => s.source_id === entry.source_id)
                      const resume = (session?.resume as Record<string, unknown> | undefined)
                        ?.command
                      return (
                        `${entry.text} [${entry.source_id}${entry.anchor}]` +
                        (session?.title ? ` — ${String(session.title)}` : '') +
                        (resume ? `, ${String(resume)}` : '')
                      )
                    })
                    .join('\n')
                }
              } catch {
                answer = 'recall unavailable'
              }
            }
            await update($, paneState, (s: PaneState) => ({ ...s, answer }))
            await $.ui.invalidate('ui.render')
          }}
        />
        {state.answer !== '' && <Text>{state.answer}</Text>}
        {state.notice !== '' && <Text dimColor>{state.notice}</Text>}
        <Link href="http://localhost:4317/scribe">
          Scribe screen for other conversations
        </Link>
      </Box>
    )
  })
}
