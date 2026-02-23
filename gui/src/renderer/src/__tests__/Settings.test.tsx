import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import Settings from '../pages/Settings'
import type { Config, AppConfig } from '../types/config'

const defaultConfig: Config = {
  global: {
    buffer_length_secs: 15,
    hotkey: 'F8',
    clip_output_dir: '%USERPROFILE%\\Videos\\Peaking'
  },
  applications: []
}

const mockElectronAPI = {
  readConfig: vi.fn<[], Promise<Config>>(),
  writeConfig: vi.fn<[Config], Promise<void>>(),
  openDirectoryDialog: vi.fn<[], Promise<string | null>>(),
  openExecutableDialog: vi.fn<[], Promise<AppConfig | null>>()
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.stubGlobal('electronAPI', mockElectronAPI)
  mockElectronAPI.readConfig.mockResolvedValue(structuredClone(defaultConfig))
  mockElectronAPI.writeConfig.mockResolvedValue(undefined)
  mockElectronAPI.openDirectoryDialog.mockResolvedValue(null)
  mockElectronAPI.openExecutableDialog.mockResolvedValue(null)
})

describe('Settings', () => {
  it('renders the settings heading', async () => {
    render(<Settings />)
    expect(screen.getByRole('heading', { name: 'Settings' })).toBeInTheDocument()
  })

  it('loads and displays default config values after mount', async () => {
    render(<Settings />)
    await waitFor(() => expect(screen.getByLabelText('Buffer length')).toBeInTheDocument())
    expect((screen.getByLabelText('Buffer length') as HTMLInputElement).value).toBe('15')
    expect((screen.getByLabelText('Hotkey') as HTMLSelectElement).value).toBe('F8')
    expect((screen.getByLabelText('Clip output directory') as HTMLInputElement).value).toBe(
      '%USERPROFILE%\\Videos\\Peaking'
    )
  })

  it('updates buffer length display when slider is moved', async () => {
    render(<Settings />)
    await waitFor(() => expect(screen.getByLabelText('Buffer length')).toBeInTheDocument())
    const slider = screen.getByLabelText('Buffer length')
    fireSliderChange(slider, '45')
    expect(screen.getByText('45s')).toBeInTheDocument()
  })

  it('does not call writeConfig before Save is clicked', async () => {
    render(<Settings />)
    await waitFor(() => expect(screen.getByLabelText('Hotkey')).toBeInTheDocument())
    await userEvent.setup().selectOptions(screen.getByLabelText('Hotkey'), 'F12')
    expect(mockElectronAPI.writeConfig).not.toHaveBeenCalled()
  })

  it('calls writeConfig with updated values when Save is clicked', async () => {
    const user = userEvent.setup()
    render(<Settings />)
    await waitFor(() => expect(screen.getByLabelText('Hotkey')).toBeInTheDocument())
    await user.selectOptions(screen.getByLabelText('Hotkey'), 'F12')
    await user.click(screen.getByRole('button', { name: 'Save Settings' }))
    expect(mockElectronAPI.writeConfig).toHaveBeenCalledWith(
      expect.objectContaining({
        global: expect.objectContaining({ hotkey: 'F12' })
      })
    )
  })

  it('shows Saved! feedback after successful save', async () => {
    const user = userEvent.setup()
    render(<Settings />)
    await waitFor(() => expect(screen.getByRole('button', { name: 'Save Settings' })).toBeInTheDocument())
    await user.click(screen.getByRole('button', { name: 'Save Settings' }))
    expect(await screen.findByText('Saved!')).toBeInTheDocument()
  })

  it('calls openDirectoryDialog when Browse is clicked and updates dir field', async () => {
    const user = userEvent.setup()
    mockElectronAPI.openDirectoryDialog.mockResolvedValue('D:\\Clips')
    render(<Settings />)
    await waitFor(() => expect(screen.getByLabelText('Browse for directory')).toBeInTheDocument())
    await user.click(screen.getByLabelText('Browse for directory'))
    expect(mockElectronAPI.openDirectoryDialog).toHaveBeenCalledOnce()
    expect((screen.getByLabelText('Clip output directory') as HTMLInputElement).value).toBe('D:\\Clips')
  })

  it('does not update dir field when directory dialog is cancelled', async () => {
    const user = userEvent.setup()
    mockElectronAPI.openDirectoryDialog.mockResolvedValue(null)
    render(<Settings />)
    await waitFor(() => expect(screen.getByLabelText('Browse for directory')).toBeInTheDocument())
    await user.click(screen.getByLabelText('Browse for directory'))
    expect((screen.getByLabelText('Clip output directory') as HTMLInputElement).value).toBe(
      '%USERPROFILE%\\Videos\\Peaking'
    )
  })

  it('calls openExecutableDialog when Add Application is clicked', async () => {
    const user = userEvent.setup()
    render(<Settings />)
    await waitFor(() => expect(screen.getByLabelText('Add application')).toBeInTheDocument())
    await user.click(screen.getByLabelText('Add application'))
    expect(mockElectronAPI.openExecutableDialog).toHaveBeenCalledOnce()
  })

  it('adds app to list and writes config when executable is selected', async () => {
    const user = userEvent.setup()
    const newApp: AppConfig = {
      display_name: 'Rocket League',
      executable_name: 'RocketLeague.exe',
      executable_path: 'C:\\Games\\RocketLeague.exe'
    }
    mockElectronAPI.openExecutableDialog.mockResolvedValue(newApp)
    render(<Settings />)
    await waitFor(() => expect(screen.getByLabelText('Add application')).toBeInTheDocument())
    await user.click(screen.getByLabelText('Add application'))
    expect(await screen.findByText('Rocket League')).toBeInTheDocument()
    expect(mockElectronAPI.writeConfig).toHaveBeenCalledWith(
      expect.objectContaining({
        applications: [newApp]
      })
    )
  })

  it('does not modify app list when executable dialog is cancelled', async () => {
    const user = userEvent.setup()
    mockElectronAPI.openExecutableDialog.mockResolvedValue(null)
    render(<Settings />)
    await waitFor(() => expect(screen.getByLabelText('Add application')).toBeInTheDocument())
    await user.click(screen.getByLabelText('Add application'))
    expect(mockElectronAPI.writeConfig).not.toHaveBeenCalled()
    expect(screen.getByText('No applications configured. Click "Add Application" to get started.')).toBeInTheDocument()
  })

  it('removes app and writes config when remove button is clicked', async () => {
    const app: AppConfig = {
      display_name: 'Rocket League',
      executable_name: 'RocketLeague.exe',
      executable_path: 'C:\\Games\\RocketLeague.exe'
    }
    mockElectronAPI.readConfig.mockResolvedValue({
      ...defaultConfig,
      applications: [app]
    })
    const user = userEvent.setup()
    render(<Settings />)
    await waitFor(() => expect(screen.getByText('Rocket League')).toBeInTheDocument())
    await user.click(screen.getByLabelText('Remove application'))
    expect(screen.queryByText('Rocket League')).not.toBeInTheDocument()
    expect(mockElectronAPI.writeConfig).toHaveBeenCalledWith(
      expect.objectContaining({ applications: [] })
    )
  })
})

// Helper to trigger a slider change since userEvent doesn't handle range inputs well
function fireSliderChange(element: Element, value: string): void {
  Object.defineProperty(element, 'value', { writable: true, value })
  element.dispatchEvent(new Event('change', { bubbles: true }))
}
