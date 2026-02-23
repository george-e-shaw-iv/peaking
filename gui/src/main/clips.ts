import { shell } from 'electron'
import { readdir, unlink } from 'fs/promises'
import { join } from 'path'
import { readConfig } from './config'

interface Clip {
  name: string
  path: string
  timestamp: string
}

interface ClipGroup {
  game: string
  clips: Clip[]
}

function resolveEnvVars(p: string): string {
  return p.replace(/%([^%]+)%/g, (_, key) => process.env[key] ?? `%${key}%`)
}

function parseTimestamp(filename: string): Date {
  const match = filename.match(/^(\d{4}-\d{2}-\d{2})_(\d{2}-\d{2}-\d{2})/)
  if (!match) return new Date(0)
  return new Date(`${match[1]}T${match[2].replace(/-/g, ':')}`)
}

export async function discoverClips(): Promise<ClipGroup[]> {
  const config = await readConfig()
  const clipDir = resolveEnvVars(config.global.clip_output_dir)

  let entries: Awaited<ReturnType<typeof readdir>>
  try {
    entries = await readdir(clipDir, { withFileTypes: true })
  } catch {
    return []
  }

  const groups: ClipGroup[] = []

  for (const entry of entries) {
    if (!entry.isDirectory()) continue
    const gameDir = join(clipDir, entry.name)

    let files: string[]
    try {
      files = await readdir(gameDir)
    } catch {
      continue
    }

    const clips: Clip[] = files
      .filter((f) => f.toLowerCase().endsWith('.mp4'))
      .map((f) => ({
        name: f.replace(/\.mp4$/i, ''),
        path: join(gameDir, f),
        timestamp: parseTimestamp(f).toISOString()
      }))
      .sort((a, b) => new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime())

    if (clips.length > 0) {
      groups.push({ game: entry.name, clips })
    }
  }

  return groups.sort(
    (a, b) =>
      new Date(b.clips[0].timestamp).getTime() - new Date(a.clips[0].timestamp).getTime()
  )
}

export async function deleteClip(filePath: string): Promise<void> {
  await unlink(filePath)
}

export function showClipInFolder(filePath: string): void {
  shell.showItemInFolder(filePath)
}
