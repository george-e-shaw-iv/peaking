import React, { useState } from 'react'
import Sidebar from './components/Sidebar'
import Status from './pages/Status'
import Settings from './pages/Settings'
import Clips from './pages/Clips'

export type Page = 'status' | 'settings' | 'clips'

export default function App(): React.JSX.Element {
  const [currentPage, setCurrentPage] = useState<Page>('status')

  return (
    <div className="flex h-screen bg-gray-900 text-gray-100">
      <Sidebar currentPage={currentPage} onNavigate={setCurrentPage} />
      <main className="flex-1 overflow-auto p-6">
        {currentPage === 'status' && <Status />}
        {currentPage === 'settings' && <Settings />}
        {currentPage === 'clips' && <Clips />}
      </main>
    </div>
  )
}
