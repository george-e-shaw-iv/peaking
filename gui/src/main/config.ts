import { app } from 'electron'
import { readFile, writeFile, mkdir } from 'fs/promises'
import { join, dirname, basename } from 'path'
import { parse, stringify } from 'smol-toml'

interface AppConfig {
  display_name: string
  executable_name: string
  executable_path: string
  buffer_length_secs?: number
  hotkey?: string
}

interface GlobalConfig {
  buffer_length_secs: number
  hotkey: string
  clip_output_dir: string
}

interface Config {
  global: GlobalConfig
  applications: AppConfig[]
}

const DEFAULT_CONFIG: Config = {
  global: {
    buffer_length_secs: 15,
    hotkey: 'F8',
    clip_output_dir: '%USERPROFILE%\\Videos\\Peaking'
  },
  applications: []
}

function getConfigPath(): string {
  const appData = process.env.APPDATA ?? join(app.getPath('home'), '.config')
  return join(appData, 'Peaking', 'config.toml')
}

export async function readConfig(): Promise<Config> {
  try {
    const content = await readFile(getConfigPath(), 'utf-8')
    return parse(content) as unknown as Config
  } catch {
    return DEFAULT_CONFIG
  }
}

export async function writeConfig(config: Config): Promise<void> {
  const configPath = getConfigPath()
  await mkdir(dirname(configPath), { recursive: true })

  // Strip undefined optional fields so they're omitted from TOML output
  const clean: Config = {
    global: config.global,
    applications: config.applications.map((app) => {
      const entry: AppConfig = {
        display_name: app.display_name,
        executable_name: app.executable_name,
        executable_path: app.executable_path
      }
      if (app.buffer_length_secs !== undefined) entry.buffer_length_secs = app.buffer_length_secs
      if (app.hotkey !== undefined) entry.hotkey = app.hotkey
      return entry
    })
  }

  await writeFile(configPath, stringify(clean as unknown as Record<string, unknown>), 'utf-8')
}

export function executableToAppConfig(filePath: string): AppConfig {
  const executableName = basename(filePath)
  const displayName = executableName.replace(/\.exe$/i, '')
  return {
    display_name: displayName,
    executable_name: executableName,
    executable_path: filePath
  }
}
