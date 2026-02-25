import React, { useState } from 'react'
import { ChevronDown, ChevronRight, FolderOpen, X } from 'lucide-react'
import type { AppConfig } from '../types/config'
import { HOTKEY_OPTIONS, BUFFER_MIN, BUFFER_MAX } from '../types/config'

interface ApplicationEntryProps {
  app: AppConfig
  onChange: (updated: AppConfig) => void
  onRemove: () => void
}

export default function ApplicationEntry({ app, onChange, onRemove }: ApplicationEntryProps): React.JSX.Element {
  const [expanded, setExpanded] = useState(false)

  function handleDisplayNameBlur(e: React.FocusEvent<HTMLInputElement>): void {
    const value = e.currentTarget.value.trim()
    if (value && value !== app.display_name) {
      onChange({ ...app, display_name: value })
    }
  }

  function handleBufferOverrideToggle(checked: boolean): void {
    onChange({
      ...app,
      buffer_length_secs: checked ? (app.buffer_length_secs ?? 15) : undefined
    })
  }

  function handleBufferOverrideChange(value: number): void {
    onChange({ ...app, buffer_length_secs: value })
  }

  function handleHotkeyOverrideToggle(checked: boolean): void {
    onChange({
      ...app,
      hotkey: checked ? (app.hotkey ?? 'F8') : undefined
    })
  }

  function handleHotkeyOverrideChange(value: string): void {
    onChange({ ...app, hotkey: value })
  }

  async function handleChangeExecutable(): Promise<void> {
    const result = await window.electronAPI.openExecutableDialog()
    if (result === null) return
    onChange({
      ...app,
      executable_name: result.executable_name,
      executable_path: result.executable_path
    })
  }

  return (
    <div className="bg-gray-700 rounded-md">
      <div className="flex items-center gap-3 px-4 py-3">
        <button
          onClick={() => setExpanded((e) => !e)}
          className="text-gray-400 hover:text-white transition-colors"
          aria-label={expanded ? 'Collapse' : 'Expand'}
        >
          {expanded ? <ChevronDown className="w-4 h-4" /> : <ChevronRight className="w-4 h-4" />}
        </button>
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium text-white truncate">{app.display_name}</p>
          <p className="text-xs text-gray-400 truncate">{app.executable_name}</p>
        </div>
        <button
          onClick={onRemove}
          className="text-gray-500 hover:text-red-400 transition-colors"
          aria-label="Remove application"
        >
          <X className="w-4 h-4" />
        </button>
      </div>

      {expanded && (
        <div className="px-4 pb-4 space-y-4 border-t border-gray-600 pt-3">
          <div>
            <label className="block text-xs font-medium text-gray-400 mb-1">Display Name</label>
            <input
              type="text"
              defaultValue={app.display_name}
              onBlur={handleDisplayNameBlur}
              className="w-full bg-gray-800 border border-gray-600 rounded px-3 py-1.5 text-sm text-white focus:outline-none focus:border-blue-500"
            />
          </div>

          <div>
            <label className="block text-xs font-medium text-gray-400 mb-1">Executable Path</label>
            <div className="flex gap-2">
              <input
                type="text"
                value={app.executable_path}
                readOnly
                className="flex-1 bg-gray-800 border border-gray-600 rounded px-3 py-1.5 text-sm text-gray-300 font-mono focus:outline-none cursor-default"
                aria-label="Executable path"
              />
              <button
                onClick={handleChangeExecutable}
                className="flex items-center gap-1.5 px-3 py-1.5 bg-gray-600 hover:bg-gray-500 border border-gray-500 rounded text-sm text-gray-300 hover:text-white transition-colors"
                aria-label="Change executable"
              >
                <FolderOpen className="w-4 h-4" />
                Change
              </button>
            </div>
          </div>

          <div>
            <div className="flex items-center gap-2 mb-2">
              <input
                type="checkbox"
                id={`buffer-override-${app.executable_name}`}
                checked={app.buffer_length_secs !== undefined}
                onChange={(e) => handleBufferOverrideToggle(e.target.checked)}
                className="accent-blue-500"
              />
              <label
                htmlFor={`buffer-override-${app.executable_name}`}
                className="text-xs font-medium text-gray-400"
              >
                Override buffer length
              </label>
              {app.buffer_length_secs !== undefined && (
                <span className="ml-auto text-sm text-white">{app.buffer_length_secs}s</span>
              )}
            </div>
            {app.buffer_length_secs !== undefined && (
              <input
                type="range"
                min={BUFFER_MIN}
                max={BUFFER_MAX}
                value={app.buffer_length_secs}
                onChange={(e) => handleBufferOverrideChange(Number(e.target.value))}
                className="w-full accent-blue-500"
                aria-label="Buffer length override"
              />
            )}
          </div>

          <div>
            <div className="flex items-center gap-2 mb-2">
              <input
                type="checkbox"
                id={`hotkey-override-${app.executable_name}`}
                checked={app.hotkey !== undefined}
                onChange={(e) => handleHotkeyOverrideToggle(e.target.checked)}
                className="accent-blue-500"
              />
              <label
                htmlFor={`hotkey-override-${app.executable_name}`}
                className="text-xs font-medium text-gray-400"
              >
                Override hotkey
              </label>
            </div>
            {app.hotkey !== undefined && (
              <select
                value={app.hotkey}
                onChange={(e) => handleHotkeyOverrideChange(e.target.value)}
                className="bg-gray-800 border border-gray-600 rounded px-3 py-1.5 text-sm text-white focus:outline-none focus:border-blue-500"
                aria-label="Hotkey override"
              >
                {HOTKEY_OPTIONS.map((key) => (
                  <option key={key} value={key}>{key}</option>
                ))}
              </select>
            )}
          </div>
        </div>
      )}
    </div>
  )
}
