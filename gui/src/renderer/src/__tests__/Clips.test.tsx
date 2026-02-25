import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import Clips from '../pages/Clips'
import type { ClipGroup } from '../types/clips'

const sampleGroups: ClipGroup[] = [
  {
    game: 'Rocket League',
    clips: [
      {
        name: '2024-06-15_14-30-00',
        path: 'C:\\Clips\\Rocket League\\2024-06-15_14-30-00.mp4',
        timestamp: '2024-06-15T14:30:00.000Z'
      },
      {
        name: '2024-06-14_10-00-00',
        path: 'C:\\Clips\\Rocket League\\2024-06-14_10-00-00.mp4',
        timestamp: '2024-06-14T10:00:00.000Z'
      }
    ]
  },
  {
    game: 'Apex Legends',
    clips: [
      {
        name: '2024-06-13_20-00-00',
        path: 'C:\\Clips\\Apex Legends\\2024-06-13_20-00-00.mp4',
        timestamp: '2024-06-13T20:00:00.000Z'
      }
    ]
  }
]

const mockElectronAPI = {
  discoverClips: vi.fn<[], Promise<ClipGroup[]>>(),
  deleteClip: vi.fn<[string], Promise<void>>(),
  showInExplorer: vi.fn<[string], Promise<void>>()
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.stubGlobal('electronAPI', mockElectronAPI)
  mockElectronAPI.discoverClips.mockResolvedValue(sampleGroups)
  mockElectronAPI.deleteClip.mockResolvedValue(undefined)
  mockElectronAPI.showInExplorer.mockResolvedValue(undefined)
})

describe('Clips', () => {
  it('renders the clips heading', async () => {
    render(<Clips />)
    expect(screen.getByRole('heading', { name: 'Clips' })).toBeInTheDocument()
    await screen.findByText('Rocket League') // flush async discoverClips state update
  })

  it('shows scanning message while loading', async () => {
    render(<Clips />)
    expect(screen.getByText('Scanning for clips…')).toBeInTheDocument()
    await screen.findByText('Rocket League') // flush async discoverClips state update
  })

  it('displays game group names after loading', async () => {
    render(<Clips />)
    await waitFor(() => expect(screen.getByText('Rocket League')).toBeInTheDocument())
    expect(screen.getByText('Apex Legends')).toBeInTheDocument()
  })

  it('shows clip count per group', async () => {
    render(<Clips />)
    await waitFor(() => expect(screen.getByText('2 clips')).toBeInTheDocument())
    expect(screen.getByText('1 clip')).toBeInTheDocument()
  })

  it('displays clip names', async () => {
    render(<Clips />)
    await waitFor(() => expect(screen.getByText('2024-06-15_14-30-00')).toBeInTheDocument())
    expect(screen.getByText('2024-06-14_10-00-00')).toBeInTheDocument()
  })

  it('shows empty state when no clips are found', async () => {
    mockElectronAPI.discoverClips.mockResolvedValue([])
    render(<Clips />)
    await waitFor(() =>
      expect(screen.getByText(/No clips found/)).toBeInTheDocument()
    )
  })

  it('calls discoverClips on mount', async () => {
    render(<Clips />)
    await waitFor(() => expect(mockElectronAPI.discoverClips).toHaveBeenCalledOnce())
  })

  it('calls discoverClips again when Refresh is clicked', async () => {
    const user = userEvent.setup()
    render(<Clips />)
    await waitFor(() => expect(screen.getByLabelText('Refresh clips')).not.toBeDisabled())
    await user.click(screen.getByLabelText('Refresh clips'))
    expect(mockElectronAPI.discoverClips).toHaveBeenCalledTimes(2)
  })

  it('shows video player when Play is clicked', async () => {
    const user = userEvent.setup()
    render(<Clips />)
    await waitFor(() => expect(screen.getAllByLabelText('Play video')[0]).toBeInTheDocument())
    await user.click(screen.getAllByLabelText('Play video')[0])
    expect(screen.getByLabelText('Video player for 2024-06-15_14-30-00')).toBeInTheDocument()
  })

  it('hides video player when Close is clicked', async () => {
    const user = userEvent.setup()
    render(<Clips />)
    await waitFor(() => expect(screen.getAllByLabelText('Play video')[0]).toBeInTheDocument())
    await user.click(screen.getAllByLabelText('Play video')[0])
    await user.click(screen.getByLabelText('Close video'))
    expect(screen.queryByLabelText('Video player for 2024-06-15_14-30-00')).not.toBeInTheDocument()
  })

  it('calls showInExplorer with correct path when Show in explorer is clicked', async () => {
    const user = userEvent.setup()
    render(<Clips />)
    await waitFor(() => expect(screen.getAllByLabelText('Show in explorer')[0]).toBeInTheDocument())
    await user.click(screen.getAllByLabelText('Show in explorer')[0])
    expect(mockElectronAPI.showInExplorer).toHaveBeenCalledWith(
      'C:\\Clips\\Rocket League\\2024-06-15_14-30-00.mp4'
    )
  })

  it('shows inline confirmation when Delete is clicked', async () => {
    const user = userEvent.setup()
    render(<Clips />)
    await waitFor(() => expect(screen.getAllByLabelText('Delete clip')[0]).toBeInTheDocument())
    await user.click(screen.getAllByLabelText('Delete clip')[0])
    expect(screen.getByLabelText('Confirm delete')).toBeInTheDocument()
    expect(screen.getByLabelText('Cancel delete')).toBeInTheDocument()
  })

  it('dismisses confirmation without deleting when Cancel is clicked', async () => {
    const user = userEvent.setup()
    render(<Clips />)
    await waitFor(() => expect(screen.getAllByLabelText('Delete clip')[0]).toBeInTheDocument())
    await user.click(screen.getAllByLabelText('Delete clip')[0])
    await user.click(screen.getByLabelText('Cancel delete'))
    expect(screen.queryByLabelText('Confirm delete')).not.toBeInTheDocument()
    expect(mockElectronAPI.deleteClip).not.toHaveBeenCalled()
  })

  it('calls deleteClip and removes clip from list when Confirm is clicked', async () => {
    const user = userEvent.setup()
    render(<Clips />)
    await waitFor(() => expect(screen.getAllByLabelText('Delete clip')[0]).toBeInTheDocument())
    await user.click(screen.getAllByLabelText('Delete clip')[0])
    await user.click(screen.getByLabelText('Confirm delete'))
    expect(mockElectronAPI.deleteClip).toHaveBeenCalledWith(
      'C:\\Clips\\Rocket League\\2024-06-15_14-30-00.mp4'
    )
    await waitFor(() =>
      expect(screen.queryByText('2024-06-15_14-30-00')).not.toBeInTheDocument()
    )
  })

  it('removes game group when its last clip is deleted', async () => {
    const user = userEvent.setup()
    render(<Clips />)
    // Apex Legends has only one clip
    await waitFor(() => expect(screen.getByText('Apex Legends')).toBeInTheDocument())
    const deleteButtons = screen.getAllByLabelText('Delete clip')
    // Last delete button belongs to the Apex Legends clip
    await user.click(deleteButtons[deleteButtons.length - 1])
    await user.click(screen.getByLabelText('Confirm delete'))
    await waitFor(() =>
      expect(screen.queryByText('Apex Legends')).not.toBeInTheDocument()
    )
  })

  it('video src uses local-file:// protocol', async () => {
    const user = userEvent.setup()
    render(<Clips />)
    await waitFor(() => expect(screen.getAllByLabelText('Play video')[0]).toBeInTheDocument())
    await user.click(screen.getAllByLabelText('Play video')[0])
    const video = screen.getByLabelText('Video player for 2024-06-15_14-30-00') as HTMLVideoElement
    expect(video.src).toMatch(/^local-file:\/\/\//)
    expect(video.src).toContain('2024-06-15_14-30-00.mp4')
  })
})
