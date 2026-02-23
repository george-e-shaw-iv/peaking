import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import Sidebar from '../components/Sidebar'

describe('Sidebar', () => {
  it('renders all three navigation items', () => {
    render(<Sidebar currentPage="status" onNavigate={vi.fn()} />)

    expect(screen.getByRole('button', { name: /status/i })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /settings/i })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /clips/i })).toBeInTheDocument()
  })

  it('renders the app title', () => {
    render(<Sidebar currentPage="status" onNavigate={vi.fn()} />)

    expect(screen.getByText('Peaking')).toBeInTheDocument()
  })

  it('highlights the active page button', () => {
    render(<Sidebar currentPage="settings" onNavigate={vi.fn()} />)

    const settingsBtn = screen.getByRole('button', { name: /settings/i })
    expect(settingsBtn.className).toContain('bg-blue-600')

    const statusBtn = screen.getByRole('button', { name: /status/i })
    expect(statusBtn.className).not.toContain('bg-blue-600')
  })

  it('calls onNavigate with the correct page when a nav item is clicked', () => {
    const onNavigate = vi.fn()
    render(<Sidebar currentPage="status" onNavigate={onNavigate} />)

    fireEvent.click(screen.getByRole('button', { name: /clips/i }))
    expect(onNavigate).toHaveBeenCalledWith('clips')

    fireEvent.click(screen.getByRole('button', { name: /settings/i }))
    expect(onNavigate).toHaveBeenCalledWith('settings')
  })

  it('calls onNavigate when the active page button is clicked again', () => {
    const onNavigate = vi.fn()
    render(<Sidebar currentPage="status" onNavigate={onNavigate} />)

    fireEvent.click(screen.getByRole('button', { name: /status/i }))
    expect(onNavigate).toHaveBeenCalledWith('status')
  })
})
