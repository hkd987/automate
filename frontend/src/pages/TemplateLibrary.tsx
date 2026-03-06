import { useState, useMemo } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button, Input } from '../components/common'
import type { Template, TemplateCategory } from '../types'
import { useTauriCommand } from '../hooks/useTauriCommand'
import { useEffect } from 'react'

const CATEGORY_LABELS: Record<TemplateCategory, string> = {
  monitoring: 'Monitoring',
  dev_ops: 'DevOps',
  code_review: 'Code Review',
  reporting: 'Reporting',
  custom: 'Custom',
}

const CATEGORY_ICONS: Record<TemplateCategory, string> = {
  monitoring: '[M]',
  dev_ops: '[D]',
  code_review: '[C]',
  reporting: '[R]',
  custom: '[X]',
}

const ALL_CATEGORIES: TemplateCategory[] = ['monitoring', 'dev_ops', 'code_review', 'reporting', 'custom']

export function TemplateLibrary() {
  const navigate = useNavigate()
  const { data: templates, execute: loadTemplates } =
    useTauriCommand<Template[]>('list_templates')
  const [search, setSearch] = useState('')
  const [activeCategory, setActiveCategory] = useState<TemplateCategory | 'all'>('all')

  useEffect(() => {
    loadTemplates().catch(() => {})
  }, [loadTemplates])

  const filtered = useMemo(() => {
    if (!templates) return []
    return templates.filter((t) => {
      const matchesSearch = t.name.toLowerCase().includes(search.toLowerCase()) ||
        t.description.toLowerCase().includes(search.toLowerCase())
      const matchesCategory = activeCategory === 'all' || t.category === activeCategory
      return matchesSearch && matchesCategory
    })
  }, [templates, search, activeCategory])

  const handleUseTemplate = (template: Template) => {
    const params = new URLSearchParams({
      template_name: template.config.name,
      template_trigger: JSON.stringify(template.config.trigger),
      template_prompt: template.config.prompt,
    })
    navigate(`/automations?${params.toString()}`)
  }

  return (
    <div>
      <h1 className="text-2xl font-bold mb-6">Template Library</h1>

      <div className="mb-4">
        <Input
          label="Search"
          placeholder="Search templates..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>

      <div className="flex gap-2 mb-6 flex-wrap">
        <button
          onClick={() => setActiveCategory('all')}
          className={`px-3 py-1 rounded text-sm ${
            activeCategory === 'all'
              ? 'bg-blue-600 text-white'
              : 'bg-gray-800 text-gray-400 hover:text-white'
          }`}
        >
          All
        </button>
        {ALL_CATEGORIES.map((cat) => (
          <button
            key={cat}
            onClick={() => setActiveCategory(cat)}
            className={`px-3 py-1 rounded text-sm ${
              activeCategory === cat
                ? 'bg-blue-600 text-white'
                : 'bg-gray-800 text-gray-400 hover:text-white'
            }`}
          >
            {CATEGORY_LABELS[cat]}
          </button>
        ))}
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {filtered.map((template) => (
          <div
            key={template.id}
            className="bg-gray-800 border border-gray-700 rounded-lg p-4 flex flex-col"
          >
            <div className="flex items-start justify-between mb-2">
              <span className="text-lg font-mono text-gray-500">
                {CATEGORY_ICONS[template.category]}
              </span>
              <span className="text-xs px-2 py-0.5 rounded bg-gray-700 text-gray-300">
                {CATEGORY_LABELS[template.category]}
              </span>
            </div>
            <h3 className="text-base font-semibold text-white mb-1">{template.name}</h3>
            <p className="text-sm text-gray-400 flex-1 mb-4">{template.description}</p>
            <Button onClick={() => handleUseTemplate(template)}>Use Template</Button>
          </div>
        ))}
      </div>

      {filtered.length === 0 && templates && (
        <p className="text-gray-500 text-center mt-8">No templates match your search.</p>
      )}
    </div>
  )
}
