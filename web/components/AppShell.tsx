'use client';

import { useState } from 'react';
import { SessionSidebar } from './SessionSidebar';
import { NodeStatusBar } from './NodeStatusBar';
import { WalletConnect } from './WalletConnect';

interface Props {
  children: React.ReactNode;
}

export function AppShell({ children }: Props) {
  const [collapsed, setCollapsed] = useState(false);

  return (
    <div className="flex flex-col h-full">
      {/* Top bar */}
      <header className="flex items-center justify-between px-4 py-2 border-b border-surface-2 bg-surface-1 flex-shrink-0">
        <div className="flex items-center gap-2">
          <span className="text-base font-bold text-accent tracking-wide">Pinaivu</span>
          <span className="text-[10px] font-mono text-muted border border-surface-3 rounded px-1">BETA</span>
        </div>
        <WalletConnect />
      </header>

      {/* Body */}
      <div className="flex flex-1 min-h-0">
        <SessionSidebar collapsed={collapsed} onCollapse={setCollapsed} />
        <main className="flex-1 flex flex-col min-w-0 bg-surface">
          {children}
        </main>
      </div>

      {/* Status bar */}
      <NodeStatusBar />
    </div>
  );
}
