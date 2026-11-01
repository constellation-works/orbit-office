import { expect, test } from 'claude-code/testing'

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

const FEED = {
  entries: [
    {
      source_id: 'src-fixture',
      anchor: '#L1',
      kind: 'decision',
      text: 'Decision: stay local',
      time: '2026-10-30T10:00:00Z',
    },
    {
      source_id: 'src-fixture',
      anchor: '#L3',
      kind: 'fact',
      text: 'Newer captured fact',
      time: '2026-10-30T11:00:00Z',
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
  ],
  refs: [],
  coverage: {},
}

function seed($: any, on: any, calls: any[], options: { fail?: boolean } = {}) {
  on('command.register', () => ({ value: { command: 'scribe-pane' } }))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('ui.close', () => ({ value: {} }))
  on('clock.every', () => ({ value: undefined }))
  on('session.id', () => ({ value: 'session-one' }))
  on('env.get', (_$: any, arg: any) => ({
    value: String(arg?.name ?? arg).includes('PLUGIN_ROOT') ? '/plugin' : '/workspace',
  }))
  on('process.run', (_$: any, arg: any) => {
    const argv: string[] = arg.argv
    calls.push(JSON.stringify(argv))
    if (options.fail) throw new Error('orbit unreachable')
    if (argv[0] === 'python3') {
      const args = JSON.parse(argv[3])
      if (args.action === 'status') {
        return { value: { on: true, last_queued_at: '2026-10-30T11:00:00Z' } }
      }
      return { value: { queued: true } }
    }
    const tool = argv[3]
    if (tool === 'office.recall') return { value: FEED }
    if (tool === 'office.capture') return { value: { appended: 1 } }
    return { value: {} }
  })
  on('agent.spawn', () => ({ model: 'haiku' }))
  on('session.start', (_$: any, e: any) => ({ cwd: e.cwd }))
  on('turn.complete', (_$: any, e: any) => e)
}

test('the pane shows the current session entries newest first', { options: { workspace: '/workspace' } }, async ($, on) => {
  const calls: any[] = []
  seed($, on, calls)
  await $.session.start({ cwd: '/work', surface: 'desktop', isInteractive: true })

  const ui = await $.ui.mount({ ...PANE, surface: 'desktop' })
  const newer = await ui.find({ type: 'Text', text: /Newer captured fact/ })
  const older = await ui.find({ type: 'Text', text: /Decision: stay local/ })
  expect(newer?.text).toContain('src-fixture#L3')
  expect(older?.text).toContain('src-fixture#L1')
  const status = await ui.find({ type: 'Text', text: /Scribe on/ })
  expect(status?.text).toContain('Local memory')
  await ui.unmount()
})

test("choosing 'Not a decision' issues one office.capture append_to correction", { options: { workspace: '/workspace' } }, async ($, on) => {
  const calls: any[] = []
  seed($, on, calls)
  await $.session.start({ cwd: '/work', surface: 'desktop', isInteractive: true })

  const ui = await $.ui.mount({ ...PANE, surface: 'desktop' })
  await ui.press({ key: 'not-decision-#L3' })

  const parsed = calls.map(raw => JSON.parse(raw))
  const captures = parsed.filter(argv => argv[0] === 'orbit' && argv[3] === 'office.capture')
  expect(captures.length).toBe(1)
  const input = JSON.parse(captures[0][captures[0].indexOf('--input') + 1])
  expect(input).toEqual({
    append_to: 'src-fixture',
    entries: [
      { kind: 'correction', correction: { anchor: '#L3', choice: 'not_decision' } },
    ],
  })
  await ui.unmount()
})

test('the ask box renders the recall subagent cited answer', { options: { workspace: '/workspace' } }, async ($, on) => {
  const calls: any[] = []
  seed($, on, calls)
  await $.session.start({ cwd: '/work', surface: 'desktop', isInteractive: true })

  const ui = await $.ui.mount({ ...PANE, surface: 'desktop' })
  await ui.input({ key: 'ask', text: 'what did we decide about hosting?' })

  const answer = await ui.find({ type: 'Text', text: /claude --resume session-one/ })
  expect(answer?.text).toContain('claude --resume session-one')
  await ui.unmount()
})

test('when orbit fails the pane shows one scribe unavailable line', { options: { workspace: '/workspace' } }, async ($, on) => {
  const calls: any[] = []
  seed($, on, calls, { fail: true })
  await $.session.start({ cwd: '/work', surface: 'desktop', isInteractive: true })

  const ui = await $.ui.mount({ ...PANE, surface: 'desktop' })
  const line = await ui.find({ type: 'Text', text: /scribe unavailable/ })
  expect(line?.text).toBe('scribe unavailable')
  await ui.unmount()
})
