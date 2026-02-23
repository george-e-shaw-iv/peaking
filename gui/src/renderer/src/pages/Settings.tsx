import React, { useEffect, useState } from 'react'
import { FolderOpen, PlusCircle } from 'lucide-react'
import type { Config, GlobalConfig, AppConfig } from '../types/config'
import { DEFAULT_CONFIG, HOTKEY_OPTIONS, BUFFER_MIN, BUFFER_MAX } from '../types/config'
import ApplicationEntry from '../components/ApplicationEntry'

export default function Settings(): React.JSX.Element {
  const [global, setGlobal] = useState<GlobalConfig>(DEFAULT_CONFIG.global)
  const [apps, setApps] = useState<AppConfig[]>(DEFAULT_CONFIG.applications)
  const [loaded, setLoaded] = useState(false)
  const [saveStatus, setSaveStatus] = useState<'idle' | 'saved' | 'error'>('idle')

  useEffect(() => {
    window.electronAPI.readConfig().then((config) => {
      setGlobal(config.global)
      setApps(config.applications)
      setLoaded(true)
    })
  }, [])

  async function writeConfig(updatedGlobal: GlobalConfig, updatedApps: AppConfig[]): Promise<void> {
    const config: Config = { global: updatedGlobal, applications: updatedApps }
    await window.electronAPI.writeConfig(config)
  }

  async function handleSaveGlobal(): Promise<void> {
    try {
      await writeConfig(global, apps)
      setSaveStatus('saved')
      setTimeout(() => setSaveStatus('idle'), 2000)
    } catch {
      setSaveStatus('error')
      setTimeout(() => setSaveStatus('idle'), 3000)
    }
  }

  async function handleBrowseDir(): Promise<void> {
    const dir = await window.electronAPI.openDirectoryDialog()
    if (dir !== null) {
      setGlobal((g) => ({ ...g, clip_output_dir: dir }))
    }
  }

  async function handleAddApplication(): Promise<void> {
    const app = await window.electronAPI.openExecutableDialog()
    if (app === null) return
    const updated = [...apps, app]
    setApps(updated)
    await writeConfig(global, updated)
  }

  async function handleAppChange(index: number, updated: AppConfig): Promise<void> {
    const updatedApps = apps.map((a, i) => (i === index ? updated : a))
    setApps(updatedApps)
    await writeConfig(global, updatedApps)
  }

  async function handleRemoveApp(index: number): Promise<void> {
    const updatedApps = apps.filter((_, i) => i !== index)
    setApps(updatedApps)
    await writeConfig(global, updatedApps)
  }

  if (!loaded) {
    return (
      <div>
        <h2 className="text-2xl font-semibold mb-6">Settings</h2>
        <p className="text-gray-400">Loading configuration…</p>
      </div>
    )
  }

  return (
    <div>
      <h2 className="text-2xl font-semibold mb-6">Settings</h2>

      <div className="space-y-6">
        {/* Global Settings */}
        <div className="bg-gray-800 rounded-lg p-6 space-y-5">
          <h3 className="text-lg font-medium text-white">Global Settings</h3>

          <div>
            <div className="flex justify-between mb-1">
              <label className="text-sm font-medium text-gray-300">Buffer Length</label>
              <span className="text-sm text-white">{global.buffer_length_secs}s</span>
            </div>
            <input
              type="range"
              min={BUFFER_MIN}
              max={BUFFER_MAX}
              value={global.buffer_length_secs}
              onChange={(e) => setGlobal((g) => ({ ...g, buffer_length_secs: Number(e.target.value) }))}
              className="w-full accent-blue-500"
              aria-label="Buffer length"
            />
            <div className="flex justify-between text-xs text-gray-500 mt-1">
              <span>{BUFFER_MIN}s</span>
              <span>{BUFFER_MAX}s</span>
            </div>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-300 mb-1">Hotkey</label>
            <select
              value={global.hotkey}
              onChange={(e) => setGlobal((g) => ({ ...g, hotkey: e.target.value }))}
              className="bg-gray-700 border border-gray-600 rounded px-3 py-2 text-sm text-white focus:outline-none focus:border-blue-500"
              aria-label="Hotkey"
            >
              {HOTKEY_OPTIONS.map((key) => (
                <option key={key} value={key}>{key}</option>
              ))}
            </select>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-300 mb-1">Clip Output Directory</label>
            <div className="flex gap-2">
              <input
                type="text"
                value={global.clip_output_dir}
                onChange={(e) => setGlobal((g) => ({ ...g, clip_output_dir: e.target.value }))}
                className="flex-1 bg-gray-700 border border-gray-600 rounded px-3 py-2 text-sm text-white focus:outline-none focus:border-blue-500"
                aria-label="Clip output directory"
              />
              <button
                onClick={handleBrowseDir}
                className="flex items-center gap-1.5 px-3 py-2 bg-gray-700 hover:bg-gray-600 border border-gray-600 rounded text-sm text-gray-300 hover:text-white transition-colors"
                aria-label="Browse for directory"
              >
                <FolderOpen className="w-4 h-4" />
                Browse
              </button>
            </div>
          </div>

          <div className="flex items-center gap-3 pt-1">
            <button
              onClick={handleSaveGlobal}
              className="px-4 py-2 bg-blue-600 hover:bg-blue-500 rounded text-sm font-medium text-white transition-colors"
            >
              Save Settings
            </button>
            {saveStatus === 'saved' && (
              <span className="text-sm text-green-400">Saved!</span>
            )}
            {saveStatus === 'error' && (
              <span className="text-sm text-red-400">Failed to save.</span>
            )}
          </div>
        </div>

        {/* Applications */}
        <div className="bg-gray-800 rounded-lg p-6 space-y-4">
          <div className="flex items-center justify-between">
            <h3 className="text-lg font-medium text-white">Applications</h3>
            <button
              onClick={handleAddApplication}
              className="flex items-center gap-1.5 px-3 py-1.5 bg-blue-600 hover:bg-blue-500 rounded text-sm font-medium text-white transition-colors"
              aria-label="Add application"
            >
              <PlusCircle className="w-4 h-4" />
              Add Application
            </button>
          </div>

          {apps.length === 0 ? (
            <p className="text-sm text-gray-400">
              No applications configured. Click "Add Application" to get started.
            </p>
          ) : (
            <div className="space-y-2">
              {apps.map((app, index) => (
                <ApplicationEntry
                  key={`${app.executable_path}-${index}`}
                  app={app}
                  onChange={(updated) => handleAppChange(index, updated)}
                  onRemove={() => handleRemoveApp(index)}
                />
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
