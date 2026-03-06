import { Link, useLocation } from 'react-router-dom'

const nav = [
  { path: '/', label: 'Dashboard' },
  { path: '/vms', label: 'VMs' },
  { path: '/automations', label: 'Automations' },
  { path: '/templates', label: 'Templates' },
  { path: '/deploy', label: 'Deploy' },
  { path: '/channels', label: 'Channels' },
  { path: '/history', label: 'History' },
]

export function Sidebar() {
  const location = useLocation()

  return (
    <aside className="w-56 bg-gray-900 border-r border-gray-800 flex flex-col">
      <div className="p-4 text-lg font-bold border-b border-gray-800">
        Automate
      </div>
      <nav className="flex-1 p-2 space-y-1">
        {nav.map((item) => (
          <Link
            key={item.path}
            to={item.path}
            className={`block px-3 py-2 rounded text-sm ${
              location.pathname === item.path
                ? 'bg-gray-800 text-white'
                : 'text-gray-400 hover:text-white hover:bg-gray-800/50'
            }`}
          >
            {item.label}
          </Link>
        ))}
      </nav>
      <div className="p-2 border-t border-gray-800">
        <Link
          to="/settings"
          className={`block px-3 py-2 rounded text-sm ${
            location.pathname === '/settings'
              ? 'bg-gray-800 text-white'
              : 'text-gray-400 hover:text-white hover:bg-gray-800/50'
          }`}
        >
          Settings
        </Link>
      </div>
    </aside>
  )
}
