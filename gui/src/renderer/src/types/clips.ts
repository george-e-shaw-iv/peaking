export interface Clip {
  /** Filename without extension, e.g. "2024-01-01_12-00-00" */
  name: string
  /** Full Windows path to the .mp4 file */
  path: string
  /** ISO 8601 timestamp parsed from the filename */
  timestamp: string
}

export interface ClipGroup {
  /** Game display name (subdirectory name under the clip output dir) */
  game: string
  /** Clips sorted newest-first */
  clips: Clip[]
}
