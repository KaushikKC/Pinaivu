'use client';

import { useState, useEffect } from 'react';
import { Wallet, LogOut, ChevronDown } from 'lucide-react';

type WalletType = 'evm' | 'solana';

interface WalletState {
  type:    WalletType;
  address: string;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyWindow = Window & Record<string, any>;

function truncateAddress(addr: string): string {
  if (addr.length <= 13) return addr;
  return `${addr.slice(0, 6)}…${addr.slice(-4)}`;
}

async function connectEvm(): Promise<string | null> {
  const eth = (window as AnyWindow).ethereum;
  if (!eth) return null;
  const accounts: string[] = await eth.request({ method: 'eth_requestAccounts' });
  return accounts[0] ?? null;
}

async function connectSolana(): Promise<string | null> {
  const sol = (window as AnyWindow).solana;
  if (!sol?.isPhantom) return null;
  const resp = await sol.connect();
  return resp.publicKey?.toString() ?? null;
}

export function WalletConnect() {
  const [wallet,     setWallet]     = useState<WalletState | null>(null);
  const [hasEvm,     setHasEvm]     = useState(false);
  const [hasSolana,  setHasSolana]  = useState(false);
  const [showMenu,   setShowMenu]   = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [error,      setError]      = useState<string | null>(null);

  useEffect(() => {
    const win = window as AnyWindow;
    const evmAvailable     = !!win.ethereum;
    const solanaAvailable  = !!(win.solana?.isPhantom);
    setHasEvm(evmAvailable);
    setHasSolana(solanaAvailable);

    // Auto-reconnect if already authorised from a prior session
    if (evmAvailable && win.ethereum.selectedAddress) {
      setWallet({ type: 'evm', address: win.ethereum.selectedAddress });
    } else if (solanaAvailable) {
      win.solana
        .connect({ onlyIfTrusted: true })
        .then((resp: { publicKey?: { toString(): string } }) => {
          const addr = resp?.publicKey?.toString();
          if (addr) setWallet({ type: 'solana', address: addr });
        })
        .catch(() => { /* not pre-authorised — fine */ });
    }
  }, []);

  // Close dropdown on outside click
  useEffect(() => {
    if (!showMenu) return;
    function handle(e: MouseEvent) {
      const target = e.target as HTMLElement;
      if (!target.closest('[data-wallet-menu]')) setShowMenu(false);
    }
    document.addEventListener('mousedown', handle);
    return () => document.removeEventListener('mousedown', handle);
  }, [showMenu]);

  async function handleConnect(type: WalletType) {
    setConnecting(true);
    setError(null);
    try {
      const address = type === 'evm' ? await connectEvm() : await connectSolana();
      if (address) setWallet({ type, address });
      else setError('No account returned — unlock your wallet and try again.');
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Connection rejected');
    } finally {
      setConnecting(false);
      setShowMenu(false);
    }
  }

  function handleDisconnect() {
    setWallet(null);
    setShowMenu(false);
  }

  const noWallets = !hasEvm && !hasSolana;

  // ── Connected state ────────────────────────────────────────────────────────
  if (wallet) {
    return (
      <div className="relative" data-wallet-menu>
        <button
          onClick={() => setShowMenu(p => !p)}
          className="flex items-center gap-2 px-3 py-1.5 rounded-lg border border-accent/40
                     bg-accent/10 text-xs text-white hover:bg-accent/20 transition-colors"
        >
          <Wallet className="w-3.5 h-3.5 text-accent" />
          <span className="font-mono">{truncateAddress(wallet.address)}</span>
          <ChevronDown className="w-3 h-3 text-muted" />
        </button>

        {showMenu && (
          <div className="absolute bottom-full right-0 mb-2 w-52 rounded-lg border border-surface-3
                          bg-surface-2 shadow-lg z-50 overflow-hidden">
            <div className="px-3 py-2 border-b border-surface-3">
              <p className="text-[10px] text-muted uppercase tracking-wider">
                {wallet.type === 'evm' ? 'EVM wallet' : 'Solana wallet'}
              </p>
              <p className="text-[11px] text-white font-mono mt-0.5 break-all">{wallet.address}</p>
            </div>
            <button
              onClick={handleDisconnect}
              className="flex items-center gap-2 w-full px-3 py-2.5 text-xs text-red-400
                         hover:bg-surface-3 transition-colors"
            >
              <LogOut className="w-3.5 h-3.5" />
              Disconnect
            </button>
          </div>
        )}
      </div>
    );
  }

  // ── Disconnected state ─────────────────────────────────────────────────────
  return (
    <div className="relative" data-wallet-menu>
      <button
        onClick={() => !noWallets && setShowMenu(p => !p)}
        disabled={connecting || noWallets}
        title={noWallets ? 'Install MetaMask or Phantom to connect a wallet' : undefined}
        className={`flex items-center gap-2 px-3 py-1.5 rounded-lg border text-xs transition-colors
          ${noWallets
            ? 'border-surface-3 bg-surface-2 text-muted opacity-60 cursor-not-allowed'
            : 'border-surface-3 bg-surface-2 text-muted hover:border-accent/50 hover:text-white cursor-pointer'
          }`}
      >
        <Wallet className="w-3.5 h-3.5" />
        <span className="font-mono">{connecting ? 'Connecting…' : 'Connect wallet'}</span>
      </button>

      {showMenu && !noWallets && (
        <div className="absolute bottom-full right-0 mb-2 w-52 rounded-lg border border-surface-3
                        bg-surface-2 shadow-lg z-50 overflow-hidden">
          <p className="px-3 py-2 text-[10px] text-muted uppercase tracking-wider border-b border-surface-3">
            Choose wallet
          </p>
          {hasEvm && (
            <button
              onClick={() => handleConnect('evm')}
              className="w-full px-3 py-2.5 text-xs text-white text-left
                         hover:bg-surface-3 transition-colors"
            >
              MetaMask / EVM
            </button>
          )}
          {hasSolana && (
            <button
              onClick={() => handleConnect('solana')}
              className="w-full px-3 py-2.5 text-xs text-white text-left
                         hover:bg-surface-3 transition-colors"
            >
              Phantom / Solana
            </button>
          )}
          {error && (
            <p className="px-3 py-2 text-[10px] text-red-400 border-t border-surface-3 break-words">
              {error}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
