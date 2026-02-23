import React, { useState } from 'react'
import { Play, Square, FolderOpen, Trash2 } from 'lucide-react'
import type { Clip } from '../types/clips'

interface ClipCardProps {
  clip: Clip
  onDeleted: (path: string) => void
}

function pathToLocalUrl(filePath: string): string {
  return 'local-file:///' + filePath.replace(/\\/g, '/')
}

function formatTimestamp(iso: string): string {
  const d = new Date(iso)
  if (isNaN(d.getTime())) return iso
  return d.toLocaleString()
}

export default function ClipCard({ clip, onDeleted }: ClipCardProps): React.JSX.Element {
  const [playing, setPlaying] = useState(false)
  const [confirming, setConfirming] = useState(false)
  const [deleting, setDeleting] = useState(false)
  const [deleteError, setDeleteError] = useState<string | null>(null)

  async function handleDelete(): Promise<void> {
    setDeleting(true)
    setDeleteError(null)
    try {
      await window.electronAPI.deleteClip(clip.path)
      onDeleted(clip.path)
    } catch (e) {
      setDeleteError(e instanceof Error ? e.message : 'Delete failed')
      setConfirming(false)
    } finally {
      setDeleting(false)
    }
  }

  return (
    <div className="bg-gray-700 rounded-md p-3 space-y-2">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <p className="text-sm font-medium text-white font-mono truncate">{clip.name}</p>
          <p className="text-xs text-gray-400">{formatTimestamp(clip.timestamp)}</p>
        </div>

        <div className="flex items-center gap-1.5 shrink-0">
          <button
            onClick={() => setPlaying((p) => !p)}
            className="p-1.5 text-gray-400 hover:text-white hover:bg-gray-600 rounded transition-colors"
            aria-label={playing ? 'Close video' : 'Play video'}
          >
            {playing ? <Square className="w-3.5 h-3.5" /> : <Play className="w-3.5 h-3.5" />}
          </button>

          <button
            onClick={() => window.electronAPI.showInExplorer(clip.path)}
            className="p-1.5 text-gray-400 hover:text-white hover:bg-gray-600 rounded transition-colors"
            aria-label="Show in explorer"
          >
            <FolderOpen className="w-3.5 h-3.5" />
          </button>

          {confirming ? (
            <div className="flex items-center gap-1">
              <span className="text-xs text-gray-300">Delete?</span>
              <button
                onClick={() => setConfirming(false)}
                className="px-1.5 py-0.5 text-xs text-gray-400 hover:text-white hover:bg-gray-600 rounded transition-colors"
                aria-label="Cancel delete"
              >
                Cancel
              </button>
              <button
                onClick={handleDelete}
                disabled={deleting}
                className="px-1.5 py-0.5 text-xs bg-red-700 hover:bg-red-600 disabled:opacity-50 text-white rounded transition-colors"
                aria-label="Confirm delete"
              >
                Delete
              </button>
            </div>
          ) : (
            <button
              onClick={() => setConfirming(true)}
              className="p-1.5 text-gray-400 hover:text-red-400 hover:bg-gray-600 rounded transition-colors"
              aria-label="Delete clip"
            >
              <Trash2 className="w-3.5 h-3.5" />
            </button>
          )}
        </div>
      </div>

      {deleteError && (
        <p className="text-xs text-red-400">{deleteError}</p>
      )}

      {playing && (
        // eslint-disable-next-line jsx-a11y/media-has-caption
        <video
          src={pathToLocalUrl(clip.path)}
          controls
          className="w-full rounded"
          aria-label={`Video player for ${clip.name}`}
        />
      )}
    </div>
  )
}
