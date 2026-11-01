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
  asking: boolean
  missedKind: string
  unavailable: boolean
  notice: string
}

declare module 'claude-code' {
  interface PluginState {
    'office-scribe': { 'scribe-pane': Shaped<PaneState> }
  }
}
