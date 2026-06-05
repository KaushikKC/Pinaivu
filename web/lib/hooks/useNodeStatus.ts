'use client';

import { useState, useEffect, useCallback } from 'react';
import { fetchHealth, fetchPeers, isDaemonAvailable, getActiveNodeUrl, type HealthResponse, type PeersResponse } from '../daemon';

export interface NodeStatus {
  available:   boolean;
  health:      HealthResponse | null;
  peers:       PeersResponse | null;
  latencyMs:   number | null;
  lastChecked: number;
  activeUrl:   string;
}

const POLL_INTERVAL_MS = 10_000;

export function useNodeStatus(): NodeStatus {
  const [status, setStatus] = useState<NodeStatus>({
    available:   false,
    health:      null,
    peers:       null,
    latencyMs:   null,
    lastChecked: 0,
    activeUrl:   '',
  });

  const check = useCallback(async () => {
    const t0 = performance.now();
    const available = await isDaemonAvailable();
    const latencyMs = Math.round(performance.now() - t0);

    if (!available) {
      setStatus(prev => ({ ...prev, available: false, latencyMs: null, lastChecked: Date.now() }));
      return;
    }

    const [health, peers] = await Promise.allSettled([fetchHealth(), fetchPeers()]);

    setStatus({
      available:   true,
      health:      health.status  === 'fulfilled' ? health.value  : null,
      peers:       peers.status   === 'fulfilled' ? peers.value   : null,
      latencyMs,
      lastChecked: Date.now(),
      activeUrl:   getActiveNodeUrl(),
    });
  }, []);

  useEffect(() => {
    check();
    const interval = setInterval(check, POLL_INTERVAL_MS);
    return () => clearInterval(interval);
  }, [check]);

  return status;
}
