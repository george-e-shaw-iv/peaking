import React from 'react'
import { Activity, Settings, Film } from 'lucide-react'
import type { Page } from '../App'

export interface SidebarProps {
  currentPage: Page
  onNavigate: (page: Page) => void
}

const navItems: Array<{
  page: Page
  label: string
  Icon: React.ComponentType<{ className?: string }>
}> = [
  { page: 'status', label: 'Status', Icon: Activity },
  { page: 'settings', label: 'Settings', Icon: Settings },
  { page: 'clips', label: 'Clips', Icon: Film },
]

export default function Sidebar({ currentPage, onNavigate }: SidebarProps): React.JSX.Element {
  return (
    <nav className="w-48 bg-gray-800 flex flex-col py-4 shrink-0">
      <div className="px-4 mb-6">
        <h1 className="text-xl font-bold text-white tracking-wide">Peaking</h1>
      </div>
      <ul className="space-y-1 px-2">
        {navItems.map(({ page, label, Icon }) => (
          <li key={page}>
            <button
              onClick={() => onNavigate(page)}
              className={`w-full flex items-center gap-3 px-3 py-2 rounded-md text-sm transition-colors ${
                currentPage === page
                  ? 'bg-blue-600 text-white'
                  : 'text-gray-400 hover:bg-gray-700 hover:text-white'
              }`}
            >
              <Icon className="w-4 h-4" />
              {label}
            </button>
          </li>
        ))}
      </ul>
    </nav>
  )
}
