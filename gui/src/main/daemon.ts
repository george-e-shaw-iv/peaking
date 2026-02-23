import { ChildProcess, spawn, exec } from 'child_process'
import { promisify } from 'util'
import { app } from 'electron'
import { join, dirname } from 'path'
import { access } from 'fs/promises'

const execAsync = promisify(exec)

let managedProcess: ChildProcess | null = null

function getDaemonPath(): string {
  if (app.isPackaged) {
    return join(dirname(app.getPath('exe')), 'peaking-daemon.exe')
  }
  return join(app.getAppPath(), '../../daemon/target/release/peaking-daemon.exe')
}

export async function spawnDaemon(): Promise<void> {
  if (managedProcess !== null && managedProcess.exitCode === null) {
    return
  }

  const daemonPath = getDaemonPath()

  try {
    await access(daemonPath)
  } catch {
    throw new Error(`Daemon executable not found at: ${daemonPath}`)
  }

  managedProcess = spawn(daemonPath, [], { stdio: 'ignore' })

  managedProcess.on('exit', () => {
    managedProcess = null
  })
}

export async function stopDaemon(): Promise<void> {
  if (managedProcess === null || managedProcess.exitCode !== null) {
    return
  }
  managedProcess.kill()
  managedProcess = null
}

export async function restartDaemon(): Promise<void> {
  await stopDaemon()
  await spawnDaemon()
}

export async function isDaemonRunning(): Promise<boolean> {
  if (managedProcess !== null && managedProcess.exitCode === null) {
    return true
  }

  if (process.platform === 'win32') {
    try {
      const { stdout } = await execAsync(
        'tasklist /FI "IMAGENAME eq peaking-daemon.exe" /NH /FO CSV'
      )
      return stdout.toLowerCase().includes('peaking-daemon.exe')
    } catch {
      return false
    }
  }

  return false
}
