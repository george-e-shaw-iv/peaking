import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import App from '../App'
import type { Config } from '../types/config'

const mockConfig: Config = {
  global: { buffer_length_secs: 15, hotkey: 'F8', clip_output_dir: '%USERPROFILE%\\Videos\\Peaking' },
  applications: []
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.stubGlobal('electronAPI', {
    readConfig: vi.fn().mockResolvedValue(mockConfig),
    writeConfig: vi.fn().mockResolvedValue(undefined),
    openDirectoryDialog: vi.fn().mockResolvedValue(null),
    openExecutableDialog: vi.fn().mockResolvedValue(null),
    onStatusUpdate: vi.fn().mockReturnValue(() => {}),
    daemonStart: vi.fn().mockResolvedValue(undefined),
    daemonStop: vi.fn().mockResolvedValue(undefined),
    daemonRestart: vi.fn().mockResolvedValue(undefined),
    discoverClips: vi.fn().mockResolvedValue([]),
    deleteClip: vi.fn().mockResolvedValue(undefined),
    showInExplorer: vi.fn().mockResolvedValue(undefined)
  })
})

describe('App', () => {
  it('renders the status page by default', () => {
    render(<App />)
    expect(screen.getByRole('heading', { name: 'Status' })).toBeInTheDocument()
    expect(screen.getByLabelText('Start daemon')).toBeInTheDocument()
  })

  it('navigates to the settings page', async () => {
    const user = userEvent.setup()
    render(<App />)

    await user.click(screen.getByRole('button', { name: /settings/i }))

    await waitFor(() =>
      expect(screen.getByRole('heading', { name: 'Settings' })).toBeInTheDocument()
    )
  })

  it('navigates to the clips page', async () => {
    const user = userEvent.setup()
    render(<App />)

    await user.click(screen.getByRole('button', { name: /clips/i }))

    await waitFor(() =>
      expect(screen.getByRole('heading', { name: 'Clips' })).toBeInTheDocument()
    )
  })

  it('navigates back to the status page', async () => {
    const user = userEvent.setup()
    render(<App />)

    await user.click(screen.getByRole('button', { name: /settings/i }))
    await user.click(screen.getByRole('button', { name: /status/i }))

    expect(screen.getByRole('heading', { name: 'Status' })).toBeInTheDocument()
  })
})
