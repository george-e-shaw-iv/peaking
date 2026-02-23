import { contextBridge, ipcRenderer } from 'electron'

// Expose IPC bridges to the renderer via window.electronAPI.
// Types for these methods are declared in src/renderer/src/types/electron.d.ts.
contextBridge.exposeInMainWorld('electronAPI', {
  // Config
  readConfig: () => ipcRenderer.invoke('config:read'),
  writeConfig: (config: unknown) => ipcRenderer.invoke('config:write', config),

  // Dialogs
  openDirectoryDialog: () => ipcRenderer.invoke('dialog:open-directory'),
  openExecutableDialog: () => ipcRenderer.invoke('dialog:open-executable'),

  // Status push — returns an unsubscribe function
  onStatusUpdate: (callback: (data: unknown) => void) => {
    const listener = (_: Electron.IpcRendererEvent, data: unknown) => callback(data)
    ipcRenderer.on('status:update', listener)
    return () => ipcRenderer.removeListener('status:update', listener)
  },

  // Daemon control
  daemonStart: () => ipcRenderer.invoke('daemon:start'),
  daemonStop: () => ipcRenderer.invoke('daemon:stop'),
  daemonRestart: () => ipcRenderer.invoke('daemon:restart'),

  // Clips
  discoverClips: () => ipcRenderer.invoke('clips:discover'),
  deleteClip: (filePath: string) => ipcRenderer.invoke('clips:delete', filePath),
  showInExplorer: (filePath: string) => ipcRenderer.invoke('clips:show-in-explorer', filePath)
})
