import type { Config, AppConfig } from './config'
import type { StatusUpdate } from './status'
import type { ClipGroup } from './clips'

declare global {
  interface Window {
    electronAPI: {
      readConfig(): Promise<Config>
      writeConfig(config: Config): Promise<void>
      openDirectoryDialog(): Promise<string | null>
      openExecutableDialog(): Promise<AppConfig | null>
      onStatusUpdate(callback: (data: StatusUpdate) => void): () => void
      daemonStart(): Promise<void>
      daemonStop(): Promise<void>
      daemonRestart(): Promise<void>
      discoverClips(): Promise<ClipGroup[]>
      deleteClip(filePath: string): Promise<void>
      showInExplorer(filePath: string): Promise<void>
    }
  }
}
