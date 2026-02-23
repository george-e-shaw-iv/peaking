import React, { useEffect, useState } from 'react'
import { Play, Square, RefreshCw, Circle } from 'lucide-react'
import type { StatusUpdate, DaemonState } from '../types/status'

function StateBadge({ state }: { state: DaemonState }): React.JSX.Element {
  const styles: Record<DaemonState, string> = {
    idle: 'bg-gray-700 text-gray-300',
    recording: 'bg-red-900 text-red-300',
    flushing: 'bg-amber-900 text-amber-300'
  }
  return (
    <span className={`inline-block px-2 py-0.5 rounded text-xs font-medium capitalize ${styles[state]}`}>
      {state}
    </span>
  )
}

export default function Status(): React.JSX.Element {
  const [update, setUpdate] = useState<StatusUpdate | null>(null)
  const [controlling, setControlling] = useState<'start' | 'stop' | 'restart' | null>(null)
  const [controlError, setControlError] = useState<string | null>(null)

  useEffect(() => {
    const unsubscribe = window.electronAPI.onStatusUpdate((data) => {
      setUpdate(data)
    })
    return unsubscribe
  }, [])

  async function handleControl(action: 'start' | 'stop' | 'restart'): Promise<void> {
    setControlling(action)
    setControlError(null)
    try {
      if (action === 'start') await window.electronAPI.daemonStart()
      else if (action === 'stop') await window.electronAPI.daemonStop()
      else await window.electronAPI.daemonRestart()
    } catch (e) {
      setControlError(e instanceof Error ? e.message : 'Unknown error')
    } finally {
      setControlling(null)
    }
  }

  const running = update?.running ?? false
  const status = update?.status ?? null

  return (
    <div>
      <h2 className="text-2xl font-semibold mb-6">Status</h2>

      <div className="space-y-6">
        {/* Daemon control card */}
        <div className="bg-gray-800 rounded-lg p-6">
          <div className="flex items-center justify-between mb-4">
            <h3 className="text-lg font-medium text-white">Daemon</h3>
            <div className="flex items-center gap-2">
              <Circle
                className={`w-3 h-3 fill-current ${running ? 'text-green-500' : 'text-gray-500'}`}
                aria-label={running ? 'Running' : 'Stopped'}
              />
              <span className={`text-sm font-medium ${running ? 'text-green-400' : 'text-gray-400'}`}>
                {running ? 'Running' : 'Stopped'}
              </span>
            </div>
          </div>

          {status && (
            <p className="text-xs text-gray-500 mb-4">Version {status.version}</p>
          )}

          <div className="flex gap-2">
            <button
              onClick={() => handleControl('start')}
              disabled={controlling !== null}
              className="flex items-center gap-1.5 px-3 py-1.5 bg-green-700 hover:bg-green-600 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium text-white transition-colors"
              aria-label="Start daemon"
            >
              <Play className="w-3.5 h-3.5" />
              Start
            </button>
            <button
              onClick={() => handleControl('stop')}
              disabled={controlling !== null}
              className="flex items-center gap-1.5 px-3 py-1.5 bg-red-800 hover:bg-red-700 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium text-white transition-colors"
              aria-label="Stop daemon"
            >
              <Square className="w-3.5 h-3.5" />
              Stop
            </button>
            <button
              onClick={() => handleControl('restart')}
              disabled={controlling !== null}
              className="flex items-center gap-1.5 px-3 py-1.5 bg-gray-700 hover:bg-gray-600 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium text-white transition-colors"
              aria-label="Restart daemon"
            >
              <RefreshCw className="w-3.5 h-3.5" />
              Restart
            </button>
          </div>

          {controlError && (
            <p className="mt-3 text-sm text-red-400">{controlError}</p>
          )}
        </div>

        {/* Runtime status card */}
        {status === null ? (
          <div className="bg-gray-800 rounded-lg p-6">
            <p className="text-sm text-gray-400">
              No status available. Start the daemon to begin.
            </p>
          </div>
        ) : (
          <div className="bg-gray-800 rounded-lg p-6 space-y-4">
            <h3 className="text-lg font-medium text-white">Runtime</h3>

            <div className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-3 text-sm">
              <span className="text-gray-400">State</span>
              <span><StateBadge state={status.state} /></span>

              <span className="text-gray-400">Active Application</span>
              <span className="text-white">
                {status.active_application ?? <span className="text-gray-500">None</span>}
              </span>

              <span className="text-gray-400">Last Clip</span>
              <span className="text-white">
                {status.last_clip_path ? (
                  <span>
                    <span className="font-mono text-xs break-all">{status.last_clip_path}</span>
                    {status.last_clip_timestamp && (
                      <span className="text-gray-400 ml-2">
                        {new Date(status.last_clip_timestamp).toLocaleString()}
                      </span>
                    )}
                  </span>
                ) : (
                  <span className="text-gray-500">None saved yet</span>
                )}
              </span>
            </div>

            {status.error && (
              <div className="mt-2 p-3 bg-red-950 border border-red-800 rounded text-sm text-red-300">
                {status.error}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}
