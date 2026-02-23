export type DaemonState = 'idle' | 'recording' | 'flushing'

export interface DaemonStatus {
  version: string
  state: DaemonState
  active_application?: string
  last_clip_path?: string
  last_clip_timestamp?: string
  error?: string
}

export interface StatusUpdate {
  status: DaemonStatus | null
  running: boolean
}
