import { useCallback, useEffect, useRef, useState } from 'react'
import type { AirPicture, ClientCommand, ServerMessage } from './types'

function wsUrl() {
  const proto = window.location.protocol === 'https:' ? 'wss' : 'ws'
  return `${proto}://${window.location.host}/ws`
}

export function useSocket() {
  const [picture, setPicture] = useState<AirPicture | null>(null)
  const [connected, setConnected] = useState(false)
  const wsRef = useRef<WebSocket | null>(null)

  useEffect(() => {
    let closed = false
    let retry: number | undefined

    const connect = () => {
      const ws = new WebSocket(wsUrl())
      wsRef.current = ws
      ws.onopen = () => {
        if (!closed) setConnected(true)
      }
      ws.onclose = () => {
        setConnected(false)
        if (!closed) retry = window.setTimeout(connect, 1200)
      }
      ws.onmessage = (ev) => {
        try {
          const msg = JSON.parse(ev.data) as ServerMessage
          if (msg.type === 'snapshot') {
            const { type: _t, ...rest } = msg as ServerMessage & AirPicture
            setPicture(rest as AirPicture)
          }
        } catch {
          // ignore malformed
        }
      }
    }

    connect()
    return () => {
      closed = true
      if (retry) window.clearTimeout(retry)
      wsRef.current?.close()
    }
  }, [])

  const send = useCallback((cmd: ClientCommand) => {
    const ws = wsRef.current
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(cmd))
    }
  }, [])

  return { picture, connected, send }
}
