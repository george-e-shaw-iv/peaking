import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import Status from '../pages/Status'
import type { StatusUpdate } from '../types/status'

type StatusCallback = (data: StatusUpdate) => void

let capturedCallback: StatusCallback | null = null

const mockElectronAPI = {
  onStatusUpdate: vi.fn((cb: StatusCallback) => {
    capturedCallback = cb
    return () => { capturedCallback = null }
  }),
  daemonStart: vi.fn<[], Promise<void>>(),
  daemonStop: vi.fn<[], Promise<void>>(),
  daemonRestart: vi.fn<[], Promise<void>>()
}

function pushUpdate(data: StatusUpdate): void {
  act(() => { capturedCallback?.(data) })
}

const idleUpdate: StatusUpdate = {
  running: true,
  status: {
    version: '0.1.0',
    state: 'idle',
    active_application: undefined,
    last_clip_path: undefined,
    last_clip_timestamp: undefined,
    error: undefined
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  capturedCallback = null
  vi.stubGlobal('electronAPI', mockElectronAPI)
  mockElectronAPI.daemonStart.mockResolvedValue(undefined)
  mockElectronAPI.daemonStop.mockResolvedValue(undefined)
  mockElectronAPI.daemonRestart.mockResolvedValue(undefined)
})

describe('Status', () => {
  it('renders the status heading', () => {
    render(<Status />)
    expect(screen.getByRole('heading', { name: 'Status' })).toBeInTheDocument()
  })

  it('shows Stopped indicator before any update arrives', () => {
    render(<Status />)
    expect(screen.getByText('Stopped')).toBeInTheDocument()
    expect(screen.getByLabelText('Stopped')).toBeInTheDocument()
  })

  it('shows Running indicator when running is true', () => {
    render(<Status />)
    pushUpdate({ running: true, status: null })
    expect(screen.getByText('Running')).toBeInTheDocument()
    expect(screen.getByLabelText('Running')).toBeInTheDocument()
  })

  it('shows Stopped indicator when running is false', () => {
    render(<Status />)
    pushUpdate({ running: false, status: null })
    expect(screen.getByText('Stopped')).toBeInTheDocument()
  })

  it('shows "no status" message when status is null', () => {
    render(<Status />)
    pushUpdate({ running: false, status: null })
    expect(screen.getByText(/No status available/)).toBeInTheDocument()
  })

  it('shows daemon version when status is present', () => {
    render(<Status />)
    pushUpdate(idleUpdate)
    expect(screen.getByText('Version 0.1.0')).toBeInTheDocument()
  })

  it('displays idle state badge', () => {
    render(<Status />)
    pushUpdate(idleUpdate)
    expect(screen.getByText('idle')).toBeInTheDocument()
  })

  it('displays recording state badge', () => {
    render(<Status />)
    pushUpdate({ ...idleUpdate, status: { ...idleUpdate.status!, state: 'recording', active_application: 'Rocket League' } })
    expect(screen.getByText('recording')).toBeInTheDocument()
  })

  it('displays flushing state badge', () => {
    render(<Status />)
    pushUpdate({ ...idleUpdate, status: { ...idleUpdate.status!, state: 'flushing' } })
    expect(screen.getByText('flushing')).toBeInTheDocument()
  })

  it('shows active application name when present', () => {
    render(<Status />)
    pushUpdate({ ...idleUpdate, status: { ...idleUpdate.status!, state: 'recording', active_application: 'Rocket League' } })
    expect(screen.getByText('Rocket League')).toBeInTheDocument()
  })

  it('shows None for active application when absent', () => {
    render(<Status />)
    pushUpdate(idleUpdate)
    expect(screen.getByText('None')).toBeInTheDocument()
  })

  it('shows last clip path and timestamp when present', () => {
    render(<Status />)
    pushUpdate({
      ...idleUpdate,
      status: {
        ...idleUpdate.status!,
        last_clip_path: 'C:\\Clips\\game\\2024-01-01_12-00-00.mp4',
        last_clip_timestamp: '2024-01-01T12:00:00Z'
      }
    })
    expect(screen.getByText('C:\\Clips\\game\\2024-01-01_12-00-00.mp4')).toBeInTheDocument()
  })

  it('shows "None saved yet" when no clip exists', () => {
    render(<Status />)
    pushUpdate(idleUpdate)
    expect(screen.getByText('None saved yet')).toBeInTheDocument()
  })

  it('shows error message when error field is present', () => {
    render(<Status />)
    pushUpdate({ ...idleUpdate, status: { ...idleUpdate.status!, error: 'encoder failed' } })
    expect(screen.getByText('encoder failed')).toBeInTheDocument()
  })

  it('does not show error section when error is absent', () => {
    render(<Status />)
    pushUpdate(idleUpdate)
    expect(screen.queryByText('encoder failed')).not.toBeInTheDocument()
  })

  it('calls daemonStart when Start button is clicked', async () => {
    const user = userEvent.setup()
    render(<Status />)
    await user.click(screen.getByLabelText('Start daemon'))
    expect(mockElectronAPI.daemonStart).toHaveBeenCalledOnce()
  })

  it('calls daemonStop when Stop button is clicked', async () => {
    const user = userEvent.setup()
    render(<Status />)
    await user.click(screen.getByLabelText('Stop daemon'))
    expect(mockElectronAPI.daemonStop).toHaveBeenCalledOnce()
  })

  it('calls daemonRestart when Restart button is clicked', async () => {
    const user = userEvent.setup()
    render(<Status />)
    await user.click(screen.getByLabelText('Restart daemon'))
    expect(mockElectronAPI.daemonRestart).toHaveBeenCalledOnce()
  })

  it('shows error message when daemon control fails', async () => {
    const user = userEvent.setup()
    mockElectronAPI.daemonStart.mockRejectedValue(new Error('Daemon executable not found'))
    render(<Status />)
    await user.click(screen.getByLabelText('Start daemon'))
    expect(await screen.findByText('Daemon executable not found')).toBeInTheDocument()
  })

  it('unsubscribes from status updates on unmount', () => {
    const { unmount } = render(<Status />)
    expect(capturedCallback).not.toBeNull()
    unmount()
    expect(capturedCallback).toBeNull()
  })
})
