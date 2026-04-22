'use client';

import { useState } from 'react';
import { AppShell } from '@/components/AppShell';
import { Settings, Shield, Cpu, Network, Trash2 } from 'lucide-react';

type Mode = 'standalone' | 'network' | 'network_paid';

const MODE_DESCRIPTIONS: Record<Mode, string> = {
  standalone:    'Local daemon only. No P2P, no blockchain. Best for development.',
  network:       'P2P network with free inference. Conversations encrypted end-to-end.',
  network_paid:  'Full decentralised network with Sui blockchain payments. Coming soon.',
};

export default function SettingsPage() {
  const [mode,       setMode]       = useState<Mode>('standalone');
  const [daemonUrl,  setDaemonUrl]  = useState('http://localhost:4002');
  const [bidWindow,  setBidWindow]  = useState('500');
  const [saved,      setSaved]      = useState(false);

  function handleSave(e: React.FormEvent) {
    e.preventDefault();
    // Settings are localStorage-only for now; full config via .toml for the daemon.
    localStorage.setItem('deai:settings', JSON.stringify({ mode, daemonUrl, bidWindowMs: Number(bidWindow) }));
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  }

  function handleClearHistory() {
    if (confirm('Delete all local chat history? This cannot be undone.')) {
      localStorage.removeItem('deai:sessions');
      window.location.href = '/chat';
    }
  }

  return (
    <AppShell>
      <div className="flex-1 overflow-y-auto p-6 max-w-2xl mx-auto w-full">
        <h1 className="text-xl font-semibold text-white mb-6 flex items-center gap-2">
          <Settings className="w-5 h-5 text-accent" />
          Settings
        </h1>

        <form onSubmit={handleSave} className="space-y-8">

          {/* Network mode */}
          <Section icon={<Network className="w-4 h-4" />} title="Network Mode">
            <div className="space-y-2">
              {(['standalone', 'network', 'network_paid'] as Mode[]).map(m => (
                <label
                  key={m}
                  className={`flex items-start gap-3 rounded-lg border px-4 py-3 cursor-pointer transition-colors
                    ${mode === m
                      ? 'border-accent/60 bg-accent/5'
                      : 'border-surface-2 bg-surface-1 hover:border-surface-3'
                    }
                    ${m === 'network_paid' ? 'opacity-50 cursor-not-allowed' : ''}
                  `}
                >
                  <input
                    type="radio"
                    name="mode"
                    value={m}
                    checked={mode === m}
                    onChange={() => setMode(m)}
                    disabled={m === 'network_paid'}
                    className="mt-0.5 accent-accent"
                  />
                  <div>
                    <div className="text-sm font-medium text-white flex items-center gap-2">
                      <code className="font-mono text-accent">{m}</code>
                      {m === 'network_paid' && (
                        <span className="text-[10px] border border-surface-3 rounded px-1 text-muted">coming soon</span>
                      )}
                    </div>
                    <div className="text-xs text-muted mt-0.5">{MODE_DESCRIPTIONS[m]}</div>
                  </div>
                </label>
              ))}
            </div>
          </Section>

          {/* Daemon */}
          <Section icon={<Cpu className="w-4 h-4" />} title="Local Daemon">
            <div className="space-y-3">
              <Field label="Daemon URL" hint="Where pinaivu is listening.">
                <input
                  type="url"
                  value={daemonUrl}
                  onChange={e => setDaemonUrl(e.target.value)}
                  className="w-full rounded-lg border border-surface-3 bg-surface-1 px-3 py-2
                             text-sm text-white placeholder:text-muted outline-none
                             focus:border-accent/60 transition-colors font-mono"
                />
              </Field>

              <Field label="Bid window (ms)" hint="How long to wait for bids in network mode.">
                <input
                  type="number"
                  min={100}
                  max={5000}
                  step={100}
                  value={bidWindow}
                  onChange={e => setBidWindow(e.target.value)}
                  className="w-full rounded-lg border border-surface-3 bg-surface-1 px-3 py-2
                             text-sm text-white outline-none focus:border-accent/60 transition-colors"
                />
              </Field>
            </div>
          </Section>

          {/* Privacy */}
          <Section icon={<Shield className="w-4 h-4" />} title="Privacy">
            <div className="rounded-lg border border-surface-2 bg-surface-1 px-4 py-3 text-sm text-muted">
              <p>
                All conversations are encrypted with <strong className="text-white">AES-256-GCM</strong> before
                leaving your device. Your session keys never leave the browser. In standalone mode,
                sessions are stored encrypted in your browser&apos;s local storage.
              </p>
              <p className="mt-2 text-xs">
                Full Walrus decentralised storage + TEE node verification available in network mode.
              </p>
            </div>
          </Section>

          {/* Danger zone */}
          <Section icon={<Trash2 className="w-4 h-4 text-red-400" />} title="Danger Zone">
            <button
              type="button"
              onClick={handleClearHistory}
              className="flex items-center gap-2 rounded-lg border border-red-800/40 bg-red-900/10
                         hover:bg-red-900/20 px-4 py-2.5 text-sm text-red-400 transition-colors"
            >
              <Trash2 className="w-4 h-4" />
              Clear all chat history
            </button>
          </Section>

          {/* Save */}
          <div className="flex items-center gap-3">
            <button
              type="submit"
              className="rounded-lg bg-accent hover:bg-accent-hover px-5 py-2.5 text-sm
                         font-medium text-white transition-colors"
            >
              Save settings
            </button>
            {saved && <span className="text-sm text-green-400">Saved!</span>}
          </div>
        </form>
      </div>
    </AppShell>
  );
}

function Section({
  icon,
  title,
  children,
}: {
  icon:     React.ReactNode;
  title:    string;
  children: React.ReactNode;
}) {
  return (
    <section>
      <h2 className="flex items-center gap-2 text-sm font-medium text-muted mb-3">
        <span className="text-accent">{icon}</span>
        {title}
      </h2>
      {children}
    </section>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label:    string;
  hint?:    string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <label className="block text-xs text-muted mb-1.5">{label}</label>
      {children}
      {hint && <p className="text-[11px] text-muted mt-1">{hint}</p>}
    </div>
  );
}
