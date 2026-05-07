'use client';

import { useEffect, useState, useCallback } from 'react';
import { AppShell } from '@/components/AppShell';
import {
  Trophy,
  RefreshCw,
  AlertTriangle,
  Zap,
  Clock,
  Coins,
  CheckCircle2,
  ExternalLink,
  ServerOff,
} from 'lucide-react';
import clsx from 'clsx';
import type { LeaderboardResponse, NodeScoreEntry } from '@/app/api/leaderboard/route';

// ─── Helpers ─────────────────────────────────────────────────────────────────

function shortPubkey(hex: string): string {
  if (hex.length < 16) return hex;
  return `${hex.slice(0, 6)}…${hex.slice(-6)}`;
}

function fmtScore(score: number): string {
  return (score / 1_000_000_000 * 100).toFixed(2);
}

function fmtSuccessRate(bps: number): string {
  return (bps / 100).toFixed(1);
}

function fmtSol(lamports: number): string {
  if (lamports === 0) return '0 SOL';
  const sol = lamports / 1_000_000_000;
  return `${sol < 0.001 ? '<0.001' : sol.toFixed(4)} SOL`;
}

function fmtLatency(ms: number): string {
  if (ms === 0) return '—';
  if (ms < 1000) return `${ms} ms`;
  return `${(ms / 1000).toFixed(1)} s`;
}

function timeAgo(fetchedAt: number): string {
  const s = Math.round((Date.now() - fetchedAt) / 1000);
  if (s < 5)  return 'just now';
  if (s < 60) return `${s}s ago`;
  return `${Math.round(s / 60)}m ago`;
}

function rankBadge(rank: number): React.ReactNode {
  if (rank === 1) return <span className="text-yellow-400 text-base" title="1st">🥇</span>;
  if (rank === 2) return <span className="text-slate-300 text-base"  title="2nd">🥈</span>;
  if (rank === 3) return <span className="text-amber-600 text-base"  title="3rd">🥉</span>;
  return <span className="text-muted font-mono text-sm w-6 text-center">{rank}</span>;
}

function latencyColor(ms: number): string {
  if (ms === 0 || ms > 5000) return 'text-muted';
  if (ms < 800)  return 'text-green-400';
  if (ms < 2000) return 'text-yellow-400';
  return 'text-red-400';
}

function successColor(bps: number): string {
  const pct = bps / 100;
  if (pct >= 95) return 'text-green-400';
  if (pct >= 80) return 'text-yellow-400';
  return 'text-red-400';
}

// ─── Components ──────────────────────────────────────────────────────────────

function ScoreBar({ score }: { score: number }) {
  const pct = score / 1_000_000_000 * 100;
  return (
    <div className="flex items-center gap-2 min-w-[120px]">
      <div className="flex-1 bg-surface-3 rounded-full h-1.5 overflow-hidden">
        <div
          className="h-full bg-accent rounded-full transition-all duration-500"
          style={{ width: `${Math.min(pct, 100)}%` }}
        />
      </div>
      <span className="text-white font-semibold text-sm tabular-nums w-14 text-right">
        {fmtScore(score)}%
      </span>
    </div>
  );
}

function NodeRow({ entry, rank }: { entry: NodeScoreEntry; rank: number }) {
  const [copied, setCopied] = useState(false);

  function copy() {
    navigator.clipboard.writeText(entry.nodePubkey).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  }

  return (
    <tr className="border-b border-surface-2 hover:bg-surface-1 transition-colors">
      {/* Rank */}
      <td className="px-4 py-3 text-center w-12">
        {rankBadge(rank)}
      </td>

      {/* Node pubkey */}
      <td className="px-4 py-3">
        <button
          onClick={copy}
          className="flex items-center gap-1.5 group text-left"
          title="Click to copy full pubkey"
        >
          <span className="font-mono text-xs text-gray-300 group-hover:text-white transition-colors">
            {shortPubkey(entry.nodePubkey)}
          </span>
          <span className="text-[10px] text-muted group-hover:text-accent transition-colors">
            {copied ? '✓' : 'copy'}
          </span>
        </button>
      </td>

      {/* Score bar */}
      <td className="px-4 py-3">
        <ScoreBar score={entry.score} />
      </td>

      {/* Jobs */}
      <td className="px-4 py-3 text-center">
        <span className="text-sm text-gray-300 tabular-nums">
          {entry.totalJobs.toLocaleString()}
        </span>
      </td>

      {/* Success rate */}
      <td className="px-4 py-3 text-center">
        <span className={clsx('text-sm font-medium tabular-nums', successColor(entry.successRateBps))}>
          {fmtSuccessRate(entry.successRateBps)}%
        </span>
      </td>

      {/* Latency */}
      <td className="px-4 py-3 text-center">
        <span className={clsx('text-sm tabular-nums', latencyColor(entry.avgLatencyMs))}>
          {fmtLatency(entry.avgLatencyMs)}
        </span>
      </td>

      {/* Earned */}
      <td className="px-4 py-3 text-right">
        <span className="text-sm text-gray-300 tabular-nums">
          {fmtSol(entry.totalLamportsEarned)}
        </span>
      </td>
    </tr>
  );
}

function EmptyState({ notDeployed }: { notDeployed: boolean }) {
  if (notDeployed) {
    return (
      <div className="flex flex-col items-center justify-center py-16 text-center px-6">
        <ServerOff className="w-10 h-10 text-surface-3 mb-4" />
        <p className="text-white font-medium mb-1">Program not deployed</p>
        <p className="text-muted text-sm max-w-sm">
          Set <code className="text-accent bg-surface-1 px-1 rounded">PINAIVU_PROGRAM_ID</code> in{' '}
          <code className="text-accent bg-surface-1 px-1 rounded">.env.local</code> after running{' '}
          <code className="text-accent bg-surface-1 px-1 rounded">anchor deploy</code>.
        </p>
      </div>
    );
  }
  return (
    <div className="flex flex-col items-center justify-center py-16 text-center px-6">
      <Trophy className="w-10 h-10 text-surface-3 mb-4" />
      <p className="text-white font-medium mb-1">No nodes registered yet</p>
      <p className="text-muted text-sm">
        Run <code className="text-accent bg-surface-1 px-1 rounded">pinaivu start --features solana</code>{' '}
        with a Solana config to appear here.
      </p>
    </div>
  );
}

// ─── Page ─────────────────────────────────────────────────────────────────────

export default function LeaderboardPage() {
  const [data,       setData]       = useState<LeaderboardResponse | null>(null);
  const [loading,    setLoading]    = useState(true);
  const [error,      setError]      = useState<string | null>(null);
  const [fetchedAt,  setFetchedAt]  = useState(0);
  const [, setTick]                 = useState(0);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res  = await fetch('/api/leaderboard', { cache: 'no-store' });
      const json = await res.json() as LeaderboardResponse;
      setData(json);
      setFetchedAt(json.fetchedAt);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  // Initial load + auto-refresh every 30 s
  useEffect(() => {
    load();
    const interval = setInterval(load, 30_000);
    return () => clearInterval(interval);
  }, [load]);

  // Tick every 5 s so "X ago" stays fresh
  useEffect(() => {
    const t = setInterval(() => setTick(n => n + 1), 5_000);
    return () => clearInterval(t);
  }, []);

  const entries      = data?.entries ?? [];
  const notDeployed  = data?.error === 'not_deployed';
  const rpcError     = data?.error && !notDeployed ? data.error : null;

  return (
    <AppShell>
      <div className="flex-1 overflow-y-auto p-6 max-w-5xl mx-auto w-full">

        {/* Header */}
        <div className="flex items-center justify-between mb-6">
          <div className="flex items-center gap-3">
            <Trophy className="w-5 h-5 text-yellow-400" />
            <h1 className="text-xl font-semibold text-white">Leaderboard</h1>

            {/* Live on Solana badge */}
            {!notDeployed && (
              <span className="flex items-center gap-1.5 text-[11px] font-medium
                               bg-[#9945FF]/15 border border-[#9945FF]/30 text-[#c084fc]
                               rounded-full px-2.5 py-0.5">
                <span className="relative flex h-1.5 w-1.5">
                  <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-[#14F195] opacity-60" />
                  <span className="relative inline-flex rounded-full h-1.5 w-1.5 bg-[#14F195]" />
                </span>
                Live · Solana Devnet
              </span>
            )}
          </div>

          <div className="flex items-center gap-3">
            {fetchedAt > 0 && (
              <span className="text-xs text-muted">
                Synced {timeAgo(fetchedAt)}
              </span>
            )}
            <button
              onClick={load}
              disabled={loading}
              className={clsx(
                'flex items-center gap-1.5 text-xs text-muted hover:text-white',
                'bg-surface-1 border border-surface-2 rounded-lg px-3 py-1.5',
                'transition-colors hover:border-surface-3',
                loading && 'opacity-50 cursor-not-allowed',
              )}
            >
              <RefreshCw className={clsx('w-3 h-3', loading && 'animate-spin')} />
              Refresh
            </button>
          </div>
        </div>

        {/* RPC error banner */}
        {rpcError && (
          <div className="flex items-center gap-2 rounded-lg border border-red-800/40 bg-red-900/10
                          px-4 py-3 text-sm text-red-400 mb-6">
            <AlertTriangle className="w-4 h-4 flex-shrink-0" />
            <span>RPC error: {rpcError}</span>
          </div>
        )}

        {/* Stats strip — only when we have data */}
        {entries.length > 0 && (
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 mb-6">
            <StatCard
              icon={<Trophy className="w-4 h-4 text-yellow-400" />}
              label="Nodes ranked"
              value={String(entries.length)}
            />
            <StatCard
              icon={<Zap className="w-4 h-4 text-accent" />}
              label="Total jobs"
              value={entries.reduce((s, e) => s + e.totalJobs, 0).toLocaleString()}
            />
            <StatCard
              icon={<CheckCircle2 className="w-4 h-4 text-green-400" />}
              label="Avg success"
              value={`${(entries.reduce((s, e) => s + e.successRateBps, 0) / entries.length / 100).toFixed(1)}%`}
            />
            <StatCard
              icon={<Coins className="w-4 h-4 text-amber-400" />}
              label="Total earned"
              value={fmtSol(entries.reduce((s, e) => s + e.totalLamportsEarned, 0))}
            />
          </div>
        )}

        {/* Table */}
        <div className="rounded-xl border border-surface-2 bg-surface-1 overflow-hidden">
          {entries.length > 0 ? (
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-surface-2 text-muted text-xs">
                  <th className="px-4 py-3 text-center w-12">#</th>
                  <th className="px-4 py-3 text-left">
                    <div className="flex items-center gap-1.5">
                      Node pubkey
                    </div>
                  </th>
                  <th className="px-4 py-3 text-left">Score</th>
                  <th className="px-4 py-3 text-center">
                    <div className="flex items-center justify-center gap-1">
                      <Zap className="w-3 h-3" /> Jobs
                    </div>
                  </th>
                  <th className="px-4 py-3 text-center">
                    <div className="flex items-center justify-center gap-1">
                      <CheckCircle2 className="w-3 h-3" /> Success
                    </div>
                  </th>
                  <th className="px-4 py-3 text-center">
                    <div className="flex items-center justify-center gap-1">
                      <Clock className="w-3 h-3" /> Latency
                    </div>
                  </th>
                  <th className="px-4 py-3 text-right">
                    <div className="flex items-center justify-end gap-1">
                      <Coins className="w-3 h-3" /> Earned
                    </div>
                  </th>
                </tr>
              </thead>
              <tbody>
                {entries.map((entry, i) => (
                  <NodeRow key={entry.pubkey} entry={entry} rank={i + 1} />
                ))}
              </tbody>
            </table>
          ) : loading ? (
            <SkeletonRows />
          ) : (
            <EmptyState notDeployed={notDeployed} />
          )}
        </div>

        {/* Footer — explorer link */}
        {!notDeployed && (
          <div className="mt-4 flex justify-end">
            <a
              href={`https://explorer.solana.com/address/${process.env.NEXT_PUBLIC_PINAIVU_PROGRAM_ID ?? ''}?cluster=devnet`}
              target="_blank"
              rel="noopener noreferrer"
              className="flex items-center gap-1 text-xs text-muted hover:text-accent transition-colors"
            >
              <ExternalLink className="w-3 h-3" />
              View program on Solana Explorer
            </a>
          </div>
        )}
      </div>
    </AppShell>
  );
}

// ─── Sub-components ───────────────────────────────────────────────────────────

function StatCard({ icon, label, value }: { icon: React.ReactNode; label: string; value: string }) {
  return (
    <div className="rounded-lg border border-surface-2 bg-surface px-4 py-3">
      <div className="flex items-center gap-1.5 text-muted text-xs mb-1.5">
        {icon}
        {label}
      </div>
      <div className="text-white font-semibold text-sm tabular-nums">{value}</div>
    </div>
  );
}

function SkeletonRows() {
  return (
    <div className="divide-y divide-surface-2 animate-pulse">
      {Array.from({ length: 5 }).map((_, i) => (
        <div key={i} className="flex items-center gap-4 px-4 py-4">
          <div className="h-4 w-6 bg-surface-3 rounded" />
          <div className="h-3 w-32 bg-surface-3 rounded" />
          <div className="flex-1 h-2 bg-surface-3 rounded-full" />
          <div className="h-3 w-12 bg-surface-3 rounded" />
          <div className="h-3 w-12 bg-surface-3 rounded" />
          <div className="h-3 w-16 bg-surface-3 rounded" />
          <div className="h-3 w-20 bg-surface-3 rounded" />
        </div>
      ))}
    </div>
  );
}
