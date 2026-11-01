import { expect, test } from 'claude-code/testing'

const OPTIONS = { options: { workspace: '/workspace', scribe_url: 'http://localhost:4317/scribe' } }

const PANE = {
  plugin: 'office-scribe',
  component: 'Pane',
  requestId: 'scribe',
  props: {
    title: 'Scribe',
    isFocused: false,
    bodyColumns: 60,
    placement: 'dock',
    scroll: { offset: 0, bodyRows: 30 },
    view: {},
  },
} as const

// office.recall's reply as `orbit tool run --json --full` prints it.
const FEED = {
  entries: [
    {
      source_id: 'src-fixture',
      anchor: '#L1',
      kind: 'decision',
      refs: [],
      text: 'Decision: stay local',
      time: '2026-10-30T10:00:00Z',
    },
    {
      source_id: 'src-fixture',
      anchor: '#L3',
      kind: 'fact',
      refs: [],
      text: 'Newer captured fact',
      time: '2026-10-30T11:00:00Z',
    },
    {
      source_id: 'src-other',
      anchor: '#L9',
      kind: 'decision',
      refs: [],
      text: 'Another session mentioned session-one',
      time: '2026-10-30T12:00:00Z',
    },
  ],
  sessions: [
    {
      source_id: 'src-fixture',
      session_id: 'session-one',
      title: 'Local memory',
      project: '/work',
      resume: { provider: 'claude', command: 'claude --resume session-one' },
    },
    { source_id: 'src-other', session_id: 'session-two', title: 'Other', resume: null },
  ],
  refs: [],
  coverage: {},
}

function ran(value: unknown, exitCode = 0) {
  return {
    value: {
      exitCode,
      stdout: JSON.stringify(value),
      stderr: '',
      isStdoutTruncated: false,
      isStderrTruncated: false,
    },
  }
}

type Seed = { fail?: boolean; feed?: unknown }

function seed(on: any, calls: { argv: string[]; env?: Record<string, string> }[], options: Seed = {}) {
  on('command.register', () => ({ value: { command: 'scribe-pane' } }))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('ui.panes', () => ({
    value: [{ id: 'scribe', title: 'Scribe', isShown: true, isFocused: false, isPlaced: true }],
  }))
  on('clock.every', () => ({ value: undefined }))
  // The ask timeout never comes due within a test.
  on('clock.after', () => ({ deny: 'held' }))
  on('clock.now', () => ({ value: Date.parse('2026-10-30T11:05:00Z') }))
  on('session.id', () => ({ value: 'session-one' }))
  on('process.run', (_$: any, e: any) => {
    calls.push({ argv: e.argv, env: e.init?.env ?? e.env })
    if (options.fail) return ran({ error: 'orbit unreachable', code: 'unreachable' }, 1)
    if (e.argv[0] === 'python3') {
      const args = JSON.parse(e.argv[3])
      if (args.action === 'status') return ran({ on: true, last_queued_at: '2026-10-30T11:00:00Z' })
      return ran({ queued: true })
    }
    const tool = e.argv[3]
    if (tool === 'office.recall') return ran(options.feed ?? FEED)
    if (tool === 'office.capture') return ran({ appended: 1 })
    return ran({})
  })
  on('session.start', (_$: any, e: any) => ({ cwd: e.cwd }))
  on('turn.complete', () => ({}))
}

async function open($: any) {
  await $.session.start({ cwd: '/work', surface: 'desktop', isInteractive: true })
  const ui = await $.ui.mount({ ...PANE, surface: 'desktop' })
  await ui.press({ key: 'refresh' })
  return ui
}

test('the pane shows the current session entries newest first', OPTIONS, async ($, on) => {
  const calls: { argv: string[] }[] = []
  seed(on, calls)
  const ui = await open($)

  const texts = (await ui.findAll({ type: 'Text' })).map((t: any) => t.text)
  const newer = texts.findIndex((t: string) => t.includes('Newer captured fact'))
  const older = texts.findIndex((t: string) => t.includes('Decision: stay local'))
  expect(newer).toBeGreaterThan(-1)
  expect(older).toBeGreaterThan(newer)
  expect(texts[newer - 1]).toMatch(/^fact · \d\d:\d\d · #L3$/)
  // Another session's source matching the id as a search term stays out.
  expect(texts.some((t: string) => t.includes('Another session'))).toBe(false)
  const status = await ui.find({ type: 'Text', text: /^Scribe on/ })
  expect(status?.text).toBe('Scribe on · Local memory · last pass 5 min ago · 2 entries')
  expect(await ui.find({ type: 'Text', text: 'src-fixture' })).toBeDefined()
  // Corrections stay folded until the entry's Correct… is pressed.
  expect(await ui.find({ key: 'remove-#L1' })).toBeUndefined()
  await ui.press({ key: 'edit-#L1' })
  expect(await ui.find({ key: 'kind-#L1' })).toBeDefined()
  expect(await ui.find({ key: 'wording-#L1' })).toBeDefined()
  expect(await ui.find({ key: 'remove-#L1' })).toBeDefined()
  expect(await ui.find({ type: 'Link' })).toBeDefined()
  await ui.unmount()
})

test("choosing 'Not a decision' issues one office.capture append_to correction", OPTIONS, async ($, on) => {
  const calls: { argv: string[] }[] = []
  seed(on, calls)
  const ui = await open($)
  await ui.press({ key: 'edit-#L1' })
  await ui.press({ key: 'not-decision-#L1' })

  const captures = calls.filter(c => c.argv[0] === 'orbit' && c.argv[3] === 'office.capture')
  expect(captures.length).toBe(1)
  const argv = captures[0]!.argv
  expect(argv.slice(0, 8)).toEqual(['orbit', 'tool', 'run', 'office.capture', '--json', '--full', '--workspace', '/workspace'])
  expect(JSON.parse(argv[argv.indexOf('--input') + 1]!)).toEqual({
    append_to: 'src-fixture',
    entries: [{ kind: 'correction', correction: { anchor: '#L1', choice: 'not_decision' } }],
  })
  await ui.unmount()
})

test('the switch runs the pane control with the plugin options', OPTIONS, async ($, on) => {
  const calls: { argv: string[]; env?: Record<string, string> }[] = []
  seed(on, calls)
  const ui = await open($)
  await ui.press({ key: 'toggle' })

  const off = calls.find(c => c.argv[0] === 'python3' && JSON.parse(c.argv[3]!).action === 'off')
  expect(off?.argv[2]).toMatch(/\/scripts\/pane\.py$/)
  expect(JSON.parse(off!.argv[3]!)).toEqual({ session_id: 'session-one', action: 'off' })
  expect(off?.env?.CLAUDE_PLUGIN_OPTION_WORKSPACE).toBe('/workspace')
  expect(calls.some(c => c.argv[0] === 'python3' && JSON.parse(c.argv[3]!).action === 'heartbeat')).toBe(true)
  await ui.unmount()
})

test('the ask box sends the question to the recall subagent', OPTIONS, async ($, on) => {
  const calls: { argv: string[] }[] = []
  const spawned: any[] = []
  seed(on, calls)
  on('agent.spawn', (_$: any, e: any) => (spawned.push(e), { model: 'haiku' }))
  const ui = await open($)
  await ui.input({ key: 'ask', text: 'what did we decide about hosting?' })

  expect(spawned.length).toBe(1)
  expect(spawned[0].subagent_type).toBe('office-scribe:recall')
  expect(spawned[0].prompt).toBe('what did we decide about hosting?')
  // The kit's spawn starts no agent (core alone sets agentId), so the pane
  // renders the same cited evidence the agent quotes.
  const answer = await ui.find({ type: 'Text', text: /Decision: stay local \[src-fixture#L1\]/ })
  expect(answer?.text).toContain('Local memory, claude --resume session-one')
  await ui.unmount()
})

test('when the subagent is refused the ask box renders office.recall cited evidence', OPTIONS, async ($, on) => {
  const calls: { argv: string[] }[] = []
  seed(on, calls)
  on('agent.spawn', () => ({ deny: 'no agents here' }))
  const ui = await open($)
  await ui.input({ key: 'ask', text: 'hosting' })

  const answer = await ui.find({ type: 'Text', text: /Decision: stay local \[src-fixture#L1\]/ })
  expect(answer?.text).toContain('Local memory, claude --resume session-one')
  await ui.unmount()
})

test('when orbit fails the pane shows one scribe unavailable line', OPTIONS, async ($, on) => {
  const calls: { argv: string[] }[] = []
  seed(on, calls, { fail: true })
  await $.session.start({ cwd: '/work', surface: 'desktop', isInteractive: true })
  const ui = await $.ui.mount({ ...PANE, surface: 'desktop' })
  await ui.press({ key: 'retry' })

  const texts = await ui.findAll({ type: 'Text' })
  expect(texts.map((t: any) => t.text)).toEqual(['scribe unavailable'])
  await ui.unmount()
})

test('mobile draws the feed and buttons without fields', OPTIONS, async ($, on) => {
  const calls: { argv: string[] }[] = []
  seed(on, calls)
  await $.session.start({ cwd: '/work', surface: 'mobile', isInteractive: true })
  const ui = await $.ui.mount({ ...PANE, surface: 'mobile' })
  await ui.press({ key: 'refresh' })
  expect(await ui.find({ type: 'Text', text: /Newer captured fact/ })).toBeDefined()
  expect(await ui.find({ key: 'capture-now' })).toBeDefined()
  await ui.unmount()
})

test('the kind filter and paging narrow a long feed', OPTIONS, async ($, on) => {
  const calls: { argv: string[] }[] = []
  const many = Array.from({ length: 20 }, (_, i) => ({
    source_id: 'src-fixture',
    anchor: `#L${i + 1}`,
    kind: i === 0 ? 'decision' : 'open',
    refs: [],
    text: `line ${i + 1}`,
    time: '2026-10-30T10:00:00Z',
  }))
  seed(on, calls, { feed: { ...FEED, entries: many } })
  const ui = await open($)
  expect((await ui.findAll({ type: 'Text', text: /^line / })).length).toBe(15)
  expect(await ui.find({ key: 'more' })).toBeDefined()
  await ui.press({ key: 'more' })
  expect((await ui.findAll({ type: 'Text', text: /^line / })).length).toBe(20)
  await ui.select({ key: 'kind-filter', value: 'decision' })
  const shown = await ui.findAll({ type: 'Text', text: /^line / })
  expect(shown.map((t: any) => t.text)).toEqual(['line 1'])
  await ui.unmount()
})
