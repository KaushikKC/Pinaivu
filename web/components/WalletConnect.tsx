'use client';

import { useState } from 'react';
import { Wallet } from 'lucide-react';

export function WalletConnect() {
  const [showTooltip, setShowTooltip] = useState(false);

  return (
    <div className="relative">
      <button
        className="flex items-center gap-2 px-3 py-1.5 rounded-lg border border-surface-3
                   bg-surface-2 text-muted text-xs hover:border-accent/50 hover:text-white
                   transition-colors cursor-not-allowed opacity-60"
        onMouseEnter={() => setShowTooltip(true)}
        onMouseLeave={() => setShowTooltip(false)}
        disabled
        title="Wallet connect — coming in network_paid mode"
      >
        <Wallet className="w-3.5 h-3.5" />
        <span className="font-mono">Connect wallet</span>
      </button>

      {showTooltip && (
        <div className="absolute bottom-full right-0 mb-2 w-64 rounded-lg border border-surface-3
                        bg-surface-2 p-3 text-xs text-muted shadow-lg z-50 animate-fade-in">
          <p className="font-medium text-white mb-1">Web3 wallet — coming soon</p>
          <p>
            On-chain payments are part of the{' '}
            <code className="text-accent">network_paid</code> mode.
            Supports EVM chains (Base, Arbitrum, Ethereum) and Solana.
          </p>
        </div>
      )}
    </div>
  );
}
