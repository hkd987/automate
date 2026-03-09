import { useEffect, useRef, useState, useCallback } from 'react'

export interface LogLine {
  timestamp: string
  stream: 'stdout' | 'stderr'
  content: string
}

interface LogViewerProps {
  lines: LogLine[]
  loading?: boolean
  autoScroll?: boolean
}

export function LogViewer({ lines, loading = false, autoScroll: initialAutoScroll = true }: LogViewerProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [autoScroll, setAutoScroll] = useState(initialAutoScroll)
  const [copied, setCopied] = useState(false)
  const userScrolledRef = useRef(false)

  useEffect(() => {
    if (autoScroll && containerRef.current && !userScrolledRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight
    }
  }, [lines, autoScroll])

  const handleScroll = useCallback(() => {
    if (!containerRef.current) return
    const { scrollTop, scrollHeight, clientHeight } = containerRef.current
    const isAtBottom = scrollHeight - scrollTop - clientHeight < 30
    if (!isAtBottom) {
      userScrolledRef.current = true
      setAutoScroll(false)
    } else {
      userScrolledRef.current = false
      setAutoScroll(true)
    }
  }, [])

  const handleCopyAll = useCallback(() => {
    const text = lines.map(l => `[${l.stream}] ${l.content}`).join('\n')
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    })
  }, [lines])

  const scrollToBottom = useCallback(() => {
    if (containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight
      userScrolledRef.current = false
      setAutoScroll(true)
    }
  }, [])

  return (
    <div className="relative bg-gray-950 rounded-lg border border-gray-700">
      <div className="absolute top-2 right-2 flex items-center gap-2 z-10">
        {!autoScroll && (
          <button
            onClick={scrollToBottom}
            className="px-2 py-1 text-xs bg-gray-700 text-gray-300 rounded hover:bg-gray-600 transition-colors"
          >
            Scroll to bottom
          </button>
        )}
        <button
          onClick={handleCopyAll}
          className="px-2 py-1 text-xs bg-gray-700 text-gray-300 rounded hover:bg-gray-600 transition-colors"
        >
          {copied ? 'Copied!' : 'Copy All'}
        </button>
      </div>
      <div
        ref={containerRef}
        onScroll={handleScroll}
        className="max-h-96 overflow-y-auto p-4 font-mono text-sm"
      >
        {lines.length === 0 && !loading && (
          <p className="text-gray-600">No log output yet.</p>
        )}
        {lines.map((line, idx) => (
          <div key={idx} className="flex gap-3 leading-relaxed">
            <span className="text-gray-600 text-xs shrink-0 select-none tabular-nums pt-0.5">
              {new Date(line.timestamp).toLocaleTimeString()}
            </span>
            <span
              className={
                line.stream === 'stderr'
                  ? 'text-orange-400 whitespace-pre-wrap break-all'
                  : 'text-gray-300 whitespace-pre-wrap break-all'
              }
            >
              {line.content}
            </span>
          </div>
        ))}
        {loading && (
          <div className="flex items-center gap-2 text-gray-500 mt-1">
            <span className="inline-block w-2 h-2 bg-blue-500 rounded-full animate-pulse" />
            Streaming...
          </div>
        )}
      </div>
    </div>
  )
}

