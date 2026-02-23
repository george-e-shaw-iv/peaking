import { app } from 'electron'
import { readFile } from 'fs/promises'
import { join } from 'path'
import { parse } from 'smol-toml'

interface DaemonStatus {
  version: string
  state: 'idle' | 'recording' | 'flushing'
  active_application?: string
  last_clip_path?: string
  last_clip_timestamp?: string
  error?: string
}

function getStatusPath(): string {
  const appData = process.env.APPDATA ?? join(app.getPath('home'), '.config')
  return join(appData, 'Peaking', 'status.toml')
}

export async function readStatus(): Promise<DaemonStatus | null> {
  try {
    const content = await readFile(getStatusPath(), 'utf-8')
    return parse(content) as unknown as DaemonStatus
  } catch {
    return null
  }
}
