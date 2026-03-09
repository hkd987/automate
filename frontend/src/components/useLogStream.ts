import { useEffect, useRef, useState } from 'react'
import type { LogLine } from './LogViewer'

interface UseLogStreamOptions {
  runId: string
  enabled?: boolean
  daemonUrl?: string
}

export function useLogStream({ runId, enabled = true, daemonUrl = 'ws://127.0.0.1:4111' }: UseLogStreamOptions) {
  const [lines, setLines] = useState<LogLine[]>([])
  const [connected, setConnected] = useState(false)
  const [closed, setClosed] = useState(false)
  const wsRef = useRef<WebSocket | null>(null)

  useEffect(() => {
    if (!enabled || !runId) return

    const ws = new WebSocket(`${daemonUrl}/ws/runs/${runId}/stream`)
    wsRef.current = ws

    ws.onopen = () => setConnected(true)

    ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data)
        if (data.type === 'stream_closed' || data.type === 'no_stream') {
          setClosed(true)
          return
        }
        if (data.content !== undefined) {
          setLines(prev => [...prev, data as LogLine])
        }
      } catch {
        // ignore parse errors
      }
    }

    ws.onclose = () => {
      setConnected(false)
      setClosed(true)
    }

    ws.onerror = () => {
      setConnected(false)
    }

    return () => {
      ws.close()
      wsRef.current = null
    }
  }, [runId, enabled, daemonUrl])

  return { lines, connected, closed }
}
