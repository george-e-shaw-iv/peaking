export interface AppConfig {
  display_name: string
  executable_name: string
  executable_path: string
  buffer_length_secs?: number
  hotkey?: string
}

export interface GlobalConfig {
  buffer_length_secs: number
  hotkey: string
  clip_output_dir: string
}

export interface Config {
  global: GlobalConfig
  applications: AppConfig[]
}

export const DEFAULT_CONFIG: Config = {
  global: {
    buffer_length_secs: 15,
    hotkey: 'F8',
    clip_output_dir: '%USERPROFILE%\\Videos\\Peaking'
  },
  applications: []
}

export const HOTKEY_OPTIONS = [
  'F1', 'F2', 'F3', 'F4', 'F5', 'F6',
  'F7', 'F8', 'F9', 'F10', 'F11', 'F12',
  'Insert', 'Home', 'PageUp',
  'Delete', 'End', 'PageDown',
  'Pause', 'ScrollLock'
]

export const BUFFER_MIN = 5
export const BUFFER_MAX = 120
