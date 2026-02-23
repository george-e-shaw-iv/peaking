import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import ApplicationEntry from '../components/ApplicationEntry'
import type { AppConfig } from '../types/config'

const baseApp: AppConfig = {
  display_name: 'Rocket League',
  executable_name: 'RocketLeague.exe',
  executable_path: 'C:\\Games\\RocketLeague.exe'
}

describe('ApplicationEntry', () => {
  it('renders app name and executable name collapsed by default', () => {
    render(<ApplicationEntry app={baseApp} onChange={vi.fn()} onRemove={vi.fn()} />)
    expect(screen.getByText('Rocket League')).toBeInTheDocument()
    expect(screen.getByText('RocketLeague.exe')).toBeInTheDocument()
    expect(screen.queryByLabelText('Buffer length override')).not.toBeInTheDocument()
  })

  it('shows override section when expanded', async () => {
    const user = userEvent.setup()
    render(<ApplicationEntry app={baseApp} onChange={vi.fn()} onRemove={vi.fn()} />)
    await user.click(screen.getByLabelText('Expand'))
    expect(screen.getByText('Display Name')).toBeInTheDocument()
    expect(screen.getByText('Override buffer length')).toBeInTheDocument()
    expect(screen.getByText('Override hotkey')).toBeInTheDocument()
  })

  it('collapses when expand button is clicked again', async () => {
    const user = userEvent.setup()
    render(<ApplicationEntry app={baseApp} onChange={vi.fn()} onRemove={vi.fn()} />)
    await user.click(screen.getByLabelText('Expand'))
    await user.click(screen.getByLabelText('Collapse'))
    expect(screen.queryByText('Display Name')).not.toBeInTheDocument()
  })

  it('calls onRemove when remove button is clicked', async () => {
    const user = userEvent.setup()
    const onRemove = vi.fn()
    render(<ApplicationEntry app={baseApp} onChange={vi.fn()} onRemove={onRemove} />)
    await user.click(screen.getByLabelText('Remove application'))
    expect(onRemove).toHaveBeenCalledOnce()
  })

  it('calls onChange with updated display_name on blur', async () => {
    const user = userEvent.setup()
    const onChange = vi.fn()
    render(<ApplicationEntry app={baseApp} onChange={onChange} onRemove={vi.fn()} />)
    await user.click(screen.getByLabelText('Expand'))
    const input = screen.getByDisplayValue('Rocket League')
    await user.clear(input)
    await user.type(input, 'RL')
    await user.tab()
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ display_name: 'RL' }))
  })

  it('enables buffer override slider when checkbox is checked', async () => {
    const user = userEvent.setup()
    const onChange = vi.fn()
    render(<ApplicationEntry app={baseApp} onChange={onChange} onRemove={vi.fn()} />)
    await user.click(screen.getByLabelText('Expand'))
    await user.click(screen.getByLabelText('Override buffer length'))
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ buffer_length_secs: 15 })
    )
  })

  it('disables buffer override when checkbox is unchecked', async () => {
    const user = userEvent.setup()
    const onChange = vi.fn()
    const appWithOverride: AppConfig = { ...baseApp, buffer_length_secs: 30 }
    render(<ApplicationEntry app={appWithOverride} onChange={onChange} onRemove={vi.fn()} />)
    await user.click(screen.getByLabelText('Expand'))
    await user.click(screen.getByLabelText('Override buffer length'))
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ buffer_length_secs: undefined })
    )
  })

  it('shows buffer override slider when buffer_length_secs is set', async () => {
    const user = userEvent.setup()
    const appWithOverride: AppConfig = { ...baseApp, buffer_length_secs: 30 }
    render(<ApplicationEntry app={appWithOverride} onChange={vi.fn()} onRemove={vi.fn()} />)
    await user.click(screen.getByLabelText('Expand'))
    expect(screen.getByLabelText('Buffer length override')).toBeInTheDocument()
  })

  it('enables hotkey override dropdown when checkbox is checked', async () => {
    const user = userEvent.setup()
    const onChange = vi.fn()
    render(<ApplicationEntry app={baseApp} onChange={onChange} onRemove={vi.fn()} />)
    await user.click(screen.getByLabelText('Expand'))
    await user.click(screen.getByLabelText('Override hotkey'))
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ hotkey: 'F8' })
    )
  })

  it('disables hotkey override when checkbox is unchecked', async () => {
    const user = userEvent.setup()
    const onChange = vi.fn()
    const appWithHotkey: AppConfig = { ...baseApp, hotkey: 'F9' }
    render(<ApplicationEntry app={appWithHotkey} onChange={onChange} onRemove={vi.fn()} />)
    await user.click(screen.getByLabelText('Expand'))
    await user.click(screen.getByLabelText('Override hotkey'))
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ hotkey: undefined })
    )
  })

  it('shows hotkey override dropdown when hotkey is set', async () => {
    const user = userEvent.setup()
    const appWithHotkey: AppConfig = { ...baseApp, hotkey: 'F9' }
    render(<ApplicationEntry app={appWithHotkey} onChange={vi.fn()} onRemove={vi.fn()} />)
    await user.click(screen.getByLabelText('Expand'))
    expect(screen.getByLabelText('Hotkey override')).toBeInTheDocument()
  })

  it('calls onChange with updated hotkey when dropdown changes', async () => {
    const user = userEvent.setup()
    const onChange = vi.fn()
    const appWithHotkey: AppConfig = { ...baseApp, hotkey: 'F8' }
    render(<ApplicationEntry app={appWithHotkey} onChange={onChange} onRemove={vi.fn()} />)
    await user.click(screen.getByLabelText('Expand'))
    await user.selectOptions(screen.getByLabelText('Hotkey override'), 'F12')
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ hotkey: 'F12' }))
  })
})
