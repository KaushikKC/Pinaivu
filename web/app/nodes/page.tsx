'use client';

import { useEffect, useState } from 'react';
import { AppShell } from '@/components/AppShell';
import { fetchHealth, fetchPeers, type HealthResponse, type PeersResponse } from '@/lib/daemon';
import { Activity, Cpu, Network, Users, AlertTriangle } from 'lucide-react';

export default function NodesPage() {
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [peers,  setPeers]  = useState<PeersResponse | null>(null);
  const [error,  setError]  = useState<string | null>(null);

  useEffect(() => {
    Promise.all([fetchHealth(), fetchPeers()])
      .then(([h, p]) => { setHealth(h); setPeers(p); })
      .catch(e => setError(e.message));
  }, []);

  return (
    <AppShell>
      <div className="flex-1 overflow-y-auto p-6 max-w-3xl mx-auto w-full">
        <h1 className="text-xl font-semibold text-white mb-6 flex items-center gap-2">
          <Network className="w-5 h-5 text-accent" />
          Network Explorer
        </h1>

        {error && (
          <div className="flex items-center gap-2 rounded-lg border border-red-800/40 bg-red-900/10 px-4 py-3 text-sm text-red-400 mb-6">
            <AlertTriangle className="w-4 h-4" />
            Cannot reach daemon: {error}
          </div>
        )}

        {health && (
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 mb-8">
            <StatCard icon={<Cpu className="w-4 h-4" />}     label="Mode"    value={health.mode} />
            <StatCard icon={<Activity className="w-4 h-4" />} label="Status"  value={health.status} color="green" />
            <StatCard icon={<Users className="w-4 h-4" />}    label="Peers"   value={String(peers?.count ?? 0)} />
            <StatCard icon={<Network className="w-4 h-4" />}  label="Version" value={`v${health.version}`} />
          </div>
        )}

        {health && health.settlements && health.settlements.length > 0 && (
          <section className="mb-8">
            <h2 className="text-sm font-medium text-muted mb-3">Settlement Adapters</h2>
            <div className="flex flex-wrap gap-2">
              {health.settlements.map(s => {
                const isChain = s.startsWith('evm-') || s === 'sui' || s === 'solana';
                return (
                  <span
                    key={s}
                    className={`inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg border text-xs font-medium
                      ${isChain
                        ? 'border-violet-700/40 bg-violet-900/20 text-violet-300'
                        : 'border-surface-3 bg-surface-2 text-muted'
                      }`}
                  >
                    {isChain && <span className="w-1.5 h-1.5 rounded-full bg-violet-400" />}
                    {s}
                  </span>
                );
              })}
            </div>
          </section>
        )}

        {peers && peers.count > 0 ? (
          <section>
            <h2 className="text-sm font-medium text-muted mb-3">Connected Peers</h2>
            <div className="space-y-2">
              {peers.peers.map(peer => (
                <div
                  key={peer}
                  className="flex items-center gap-3 rounded-lg border border-surface-2
                             bg-surface-1 px-4 py-3"
                >
                  <span className="relative flex h-2 w-2">
                    <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-green-400 opacity-40" />
                    <span className="relative inline-flex rounded-full h-2 w-2 bg-green-500" />
                  </span>
                  <span className="font-mono text-sm text-gray-300 truncate">{peer}</span>
                </div>
              ))}
            </div>
          </section>
        ) : health ? (
          <div className="rounded-lg border border-surface-2 bg-surface-1 px-6 py-8 text-center text-muted text-sm">
            <Network className="w-8 h-8 mx-auto mb-3 text-surface-3" />
            <p>No peers connected.</p>
            <p className="mt-1 text-xs">
              Running in <code className="text-accent">{health.mode}</code> mode.
              {health.mode === 'Standalone' && (
                <> Start with <code className="text-accent">--mode network</code> to join the P2P network.</>
              )}
            </p>
          </div>
        ) : null}
      </div>
    </AppShell>
  );
}

function StatCard({
  icon,
  label,
  value,
  color,
}: {
  icon:   React.ReactNode;
  label:  string;
  value:  string;
  color?: 'green';
}) {
  return (
    <div className="rounded-lg border border-surface-2 bg-surface-1 px-4 py-3">
      <div className="flex items-center gap-1.5 text-muted text-xs mb-1.5">
        {icon}
        {label}
      </div>
      <div
        className={
          color === 'green'
            ? 'text-green-400 font-semibold text-sm'
            : 'text-white font-semibold text-sm'
        }
      >
        {value}
      </div>
    </div>
  );
}
