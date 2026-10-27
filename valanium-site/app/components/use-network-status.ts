"use client";
import { useCallback, useEffect, useRef, useState } from 'react';
import type { StatusData } from '../network-status';
export function useNetworkStatus() {
  const [data, setData] = useState<StatusData | null>(null);
  const [failed, setFailed] = useState(false);
  const [busy, setBusy] = useState(false);
  const active = useRef<AbortController | null>(null);
  const mounted = useRef(false);
  const refresh = useCallback(async () => {
    if (active.current || !mounted.current || document.hidden) return;
    const controller = new AbortController(); active.current = controller; setBusy(true);
    const timeout = setTimeout(() => controller.abort(), 8000);
    try {
      const response = await fetch('/api/status', {cache: 'no-store', signal: controller.signal});
      if (!response.ok) throw new Error('Status unavailable');
      const next = await response.json() as StatusData;
      if (!Array.isArray(next.servers) || next.servers.length !== 3 || !Array.isArray(next.services)
          || !Number.isFinite(Date.parse(next.checkedAt))
          || !next.servers.every(node => ['FI', 'NL'].includes(node.country) && node.metrics && typeof node.name === 'string')
          || !next.services.every(service => typeof service.name === 'string' && ['operational', 'degraded', 'outage'].includes(service.status))) throw new Error('Invalid status');
      if (mounted.current) {setData(next); setFailed(false);}
    } catch { if (mounted.current) setFailed(true); }
    finally {clearTimeout(timeout); active.current = null; if (mounted.current) setBusy(false);}
  }, []);
  useEffect(() => {
    mounted.current = true;
    const initial = setTimeout(() => { void refresh(); }, 0);
    const timer = setInterval(() => { void refresh(); }, 15000);
    const visible = () => { if (!document.hidden) void refresh(); };
    document.addEventListener('visibilitychange', visible);
    return () => {mounted.current = false; clearTimeout(initial); clearInterval(timer); active.current?.abort(); document.removeEventListener('visibilitychange', visible);};
  }, [refresh]);
  return {data, failed, busy, refresh};
}
