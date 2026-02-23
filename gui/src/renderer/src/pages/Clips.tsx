import React, { useEffect, useState } from 'react'
import { RefreshCw } from 'lucide-react'
import type { ClipGroup } from '../types/clips'
import ClipCard from '../components/ClipCard'

export default function Clips(): React.JSX.Element {
  const [groups, setGroups] = useState<ClipGroup[]>([])
  const [loading, setLoading] = useState(true)

  async function load(): Promise<void> {
    setLoading(true)
    const discovered = await window.electronAPI.discoverClips()
    setGroups(discovered)
    setLoading(false)
  }

  useEffect(() => {
    load()
  }, [])

  function handleDeleted(deletedPath: string): void {
    setGroups((prev) =>
      prev
        .map((group) => ({
          ...group,
          clips: group.clips.filter((c) => c.path !== deletedPath)
        }))
        .filter((group) => group.clips.length > 0)
    )
  }

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <h2 className="text-2xl font-semibold">Clips</h2>
        <button
          onClick={load}
          disabled={loading}
          className="flex items-center gap-1.5 px-3 py-1.5 bg-gray-700 hover:bg-gray-600 disabled:opacity-50 rounded text-sm text-gray-300 hover:text-white transition-colors"
          aria-label="Refresh clips"
        >
          <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
          Refresh
        </button>
      </div>

      {loading ? (
        <p className="text-gray-400 text-sm">Scanning for clips…</p>
      ) : groups.length === 0 ? (
        <div className="bg-gray-800 rounded-lg p-6">
          <p className="text-gray-400 text-sm">
            No clips found. Start recording and press the hotkey to save a clip.
          </p>
        </div>
      ) : (
        <div className="space-y-6">
          {groups.map((group) => (
            <div key={group.game}>
              <div className="flex items-baseline gap-2 mb-3">
                <h3 className="text-lg font-medium text-white">{group.game}</h3>
                <span className="text-xs text-gray-500">
                  {group.clips.length} {group.clips.length === 1 ? 'clip' : 'clips'}
                </span>
              </div>
              <div className="space-y-2">
                {group.clips.map((clip) => (
                  <ClipCard key={clip.path} clip={clip} onDeleted={handleDeleted} />
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
