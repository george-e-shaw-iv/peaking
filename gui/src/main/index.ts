import { app, BrowserWindow, ipcMain, dialog, protocol, net } from 'electron'
import { join, sep } from 'path'
import { pathToFileURL } from 'url'
import { readConfig, writeConfig, executableToAppConfig } from './config'
import { readStatus } from './status'
import { spawnDaemon, stopDaemon, restartDaemon, isDaemonRunning } from './daemon'
import { discoverClips, deleteClip, showClipInFolder } from './clips'

// Must be called before app.ready
protocol.registerSchemesAsPrivileged([
  { scheme: 'local-file', privileges: { secure: true, supportFetchAPI: true, stream: true } }
])

function createWindow(): void {
  const win = new BrowserWindow({
    width: 1200,
    height: 800,
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      sandbox: false
    }
  })

  if (process.env.NODE_ENV === 'development') {
    win.loadURL(process.env['ELECTRON_RENDERER_URL']!)
  } else {
    win.loadFile(join(__dirname, '../renderer/index.html'))
  }

  startStatusPolling(win)
}

function startStatusPolling(win: BrowserWindow): void {
  async function push(): Promise<void> {
    if (win.isDestroyed()) return
    const [status, running] = await Promise.all([readStatus(), isDaemonRunning()])
    win.webContents.send('status:update', { status, running })
  }

  win.webContents.once('did-finish-load', () => push())
  const interval = setInterval(push, 2000)
  win.on('closed', () => clearInterval(interval))
}

// Config IPC
ipcMain.handle('config:read', () => readConfig())
ipcMain.handle('config:write', (_event, config) => writeConfig(config))

// Dialog IPC
ipcMain.handle('dialog:open-directory', async () => {
  const result = await dialog.showOpenDialog({ properties: ['openDirectory'] })
  if (result.canceled || result.filePaths.length === 0) return null
  return result.filePaths[0]
})

ipcMain.handle('dialog:open-executable', async () => {
  const result = await dialog.showOpenDialog({
    properties: ['openFile'],
    filters: [{ name: 'Executable', extensions: ['exe'] }]
  })
  if (result.canceled || result.filePaths.length === 0) return null
  return executableToAppConfig(result.filePaths[0])
})

// Daemon control IPC
ipcMain.handle('daemon:start', () => spawnDaemon())
ipcMain.handle('daemon:stop', () => stopDaemon())
ipcMain.handle('daemon:restart', () => restartDaemon())

// Clips IPC
ipcMain.handle('clips:discover', () => discoverClips())
ipcMain.handle('clips:delete', (_event, filePath: string) => deleteClip(filePath))
ipcMain.handle('clips:show-in-explorer', (_event, filePath: string) => showClipInFolder(filePath))

app.whenReady().then(() => {
  // Serve local files through the custom protocol so the renderer can play videos
  protocol.handle('local-file', (request) => {
    const filePath = decodeURIComponent(request.url.slice('local-file:///'.length)).replace(
      /\//g,
      sep
    )
    return net.fetch(pathToFileURL(filePath).toString())
  })

  createWindow()
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit()
  }
})
