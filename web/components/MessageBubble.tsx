'use client';

import { memo, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { User, Bot, ChevronDown, ChevronRight, Lock, Server, Network, Clock, FileText, CheckCircle2, XCircle, ExternalLink } from 'lucide-react';
import clsx from 'clsx';
import type { Message, MessageReceipt } from '@/lib/session-store';

function chainInfoFromSettlement(settlementId: string): { name: string; explorerBase?: string } {
  if (settlementId === 'free')    return { name: 'Free (no payment)' };
  if (settlementId === 'receipt') return { name: 'Signed Receipt' };
  if (settlementId === 'channel') return { name: 'Payment Channel' };
  if (settlementId === 'sui')     return { name: 'Sui Network' };
  if (settlementId === 'solana')  return { name: 'Solana' };
  if (settlementId.startsWith('evm-')) {
    const chainId = settlementId.slice(4);
    const chains: Record<string, { name: string; explorerBase: string }> = {
      '1':       { name: 'Ethereum',        explorerBase: 'https://etherscan.io/tx/' },
      '11155111':{ name: 'Eth Sepolia',     explorerBase: 'https://sepolia.etherscan.io/tx/' },
      '8453':    { name: 'Base',            explorerBase: 'https://basescan.org/tx/' },
      '84532':   { name: 'Base Sepolia',    explorerBase: 'https://sepolia.basescan.org/tx/' },
      '42161':   { name: 'Arbitrum One',    explorerBase: 'https://arbiscan.io/tx/' },
      '421614':  { name: 'Arbitrum Sepolia',explorerBase: 'https://sepolia.arbiscan.io/tx/' },
      '137':     { name: 'Polygon',         explorerBase: 'https://polygonscan.com/tx/' },
      '100':     { name: 'Gnosis Chain',    explorerBase: 'https://gnosisscan.io/tx/' },
    };
    return chains[chainId] ?? { name: `EVM chain ${chainId}` };
  }
  return { name: settlementId };
}

interface Props {
  message:    Message;
  streaming?: boolean;
}

function parseContent(raw: string): { thinking: string; answer: string; inThink: boolean } {
  if (!raw.startsWith('<think>')) return { thinking: '', answer: raw, inThink: false };

  const closeIdx = raw.indexOf('</think>');
  if (closeIdx === -1) {
    // Still inside the think block while streaming
    return { thinking: raw.slice(7), answer: '', inThink: true };
  }

  return {
    thinking: raw.slice(7, closeIdx).trim(),
    answer:   raw.slice(closeIdx + 8).trim(),
    inThink:  false,
  };
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

function ReceiptPanel({ receipt }: { receipt: MessageReceipt }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="mt-3">
      <button
        onClick={() => setOpen(v => !v)}
        className="flex items-center gap-1.5 text-[11px] text-muted hover:text-white transition-colors"
      >
        {open ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
        <FileText className="w-3 h-3" />
        <span>Proof of Inference</span>
        {receipt.proofValid
          ? <CheckCircle2 className="w-3 h-3 text-emerald-400" />
          : <XCircle className="w-3 h-3 text-red-400" />}
      </button>

      {open && (
        <div className="mt-2 rounded-lg border border-surface-3 bg-surface-2 p-3 text-[11px] font-mono space-y-1.5">
          <Row label="Valid"       value={receipt.proofValid ? '✓ yes' : '✗ no'} accent={receipt.proofValid} />
          {(() => {
            const chain = chainInfoFromSettlement(receipt.settlementId);
            return (
              <div className="flex gap-2">
                <span className="w-24 flex-shrink-0 text-muted">Settlement</span>
                <span className="flex items-center gap-2 text-gray-300">
                  <span>{chain.name}</span>
                  {receipt.chainTxId && chain.explorerBase && (
                    <a
                      href={`${chain.explorerBase}${receipt.chainTxId}`}
                      target="_blank"
                      rel="noreferrer"
                      className="inline-flex items-center gap-1 text-accent hover:underline text-[10px]"
                    >
                      <ExternalLink className="w-3 h-3" />
                      View on chain
                    </a>
                  )}
                </span>
              </div>
            );
          })()}
          <Row label="In tokens"   value={String(receipt.inputTokens)} />
          <Row label="Out tokens"  value={String(receipt.outputTokens)} />
          <Row label="Latency"     value={`${receipt.latencyMs} ms`} />
          <Row label="Proof ID"    value={receipt.proofId} mono truncate />
          <Row label="Node pubkey" value={receipt.nodePubkey} mono truncate />
          <Row label="Signature"   value={receipt.signature.slice(0, 40) + '…'} mono />
          <div className="pt-1 border-t border-surface-3">
            <p className="text-muted text-[10px]">
              canonical_bytes SHA-256 signed with Ed25519 — verifiable offline
            </p>
          </div>
        </div>
      )}
    </div>
  );
}

function Row({
  label, value, accent, mono, truncate,
}: {
  label: string; value: string; accent?: boolean; mono?: boolean; truncate?: boolean;
}) {
  return (
    <div className="flex gap-2">
      <span className="w-24 flex-shrink-0 text-muted">{label}</span>
      <span className={clsx(
        'flex-1 break-all',
        accent ? 'text-emerald-400' : 'text-gray-300',
        mono && 'font-mono',
        truncate && 'truncate',
      )}>
        {value}
      </span>
    </div>
  );
}

export const MessageBubble = memo(function MessageBubble({ message, streaming }: Props) {
  const isUser = message.role === 'user';
  const [thinkOpen, setThinkOpen] = useState(false);

  const { thinking, answer, inThink } = isUser
    ? { thinking: '', answer: message.content, inThink: false }
    : parseContent(message.content);

  return (
    <div
      className={clsx(
        'group flex gap-3 px-4 py-5 animate-fade-in',
        isUser ? 'bg-transparent' : 'bg-surface-1',
      )}
    >
      {/* Avatar */}
      <div
        className={clsx(
          'flex-shrink-0 w-7 h-7 rounded-full flex items-center justify-center text-xs',
          isUser ? 'bg-accent text-white' : 'bg-surface-3 text-accent',
        )}
      >
        {isUser ? <User className="w-3.5 h-3.5" /> : <Bot className="w-3.5 h-3.5" />}
      </div>

      {/* Content */}
      <div className="flex-1 min-w-0">
        {/* Label row */}
        <div className="flex items-center gap-2 mb-1.5">
          <span className="text-[11px] font-medium text-muted">
            {isUser ? 'You' : 'Pinaivu'}
          </span>

          {/* Trust badges — only on assistant messages */}
          {!isUser && (
            <span className="flex items-center gap-1.5">
              {message.nodeId ? (
                <span className="inline-flex items-center gap-1 text-[9px] font-medium px-1.5 py-0.5 rounded
                                 bg-violet-900/30 border border-violet-700/40 text-violet-300"
                      title={`Network node: ${message.nodeId}`}>
                  <Network className="w-2.5 h-2.5" />
                  Network
                </span>
              ) : (
                <span className="inline-flex items-center gap-1 text-[9px] font-medium px-1.5 py-0.5 rounded
                                 bg-emerald-900/30 border border-emerald-700/40 text-emerald-400">
                  <Server className="w-2.5 h-2.5" />
                  Local
                </span>
              )}
              <span className="inline-flex items-center gap-1 text-[9px] font-medium px-1.5 py-0.5 rounded
                               bg-blue-900/30 border border-blue-700/40 text-blue-400">
                <Lock className="w-2.5 h-2.5" />
                E2E
              </span>
            </span>
          )}
        </div>

        {/* Thinking block (deepseek-r1 reasoning) */}
        {!isUser && (thinking || inThink) && (
          <div className="mb-3">
            <button
              onClick={() => setThinkOpen(v => !v)}
              className="flex items-center gap-1.5 text-[11px] text-muted hover:text-white transition-colors"
            >
              {thinkOpen ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
              <span className="italic">
                {inThink && streaming ? 'Reasoning…' : 'Reasoning'}
              </span>
              {inThink && streaming && (
                <span className="w-1.5 h-1.5 bg-accent rounded-full animate-pulse ml-0.5" />
              )}
            </button>

            {thinkOpen && (
              <div className="mt-1.5 pl-3 border-l-2 border-surface-3 text-[12px] text-muted
                              leading-relaxed whitespace-pre-wrap break-words max-h-48 overflow-y-auto">
                {thinking}
              </div>
            )}
          </div>
        )}

        {/* Main answer */}
        {isUser ? (
          <p className="text-sm leading-relaxed whitespace-pre-wrap break-words text-gray-200">
            {answer}
          </p>
        ) : (
          <div className={clsx('prose-deai text-gray-200', streaming && !inThink && 'cursor-blink')}>
            {answer ? (
              <ReactMarkdown remarkPlugins={[remarkGfm]}>{answer}</ReactMarkdown>
            ) : inThink ? null : streaming ? (
              <div className="flex flex-col gap-1.5">
                <div className="flex items-center gap-1.5">
                  <span className="w-1 h-1 bg-accent rounded-full animate-bounce [animation-delay:0ms]" />
                  <span className="w-1 h-1 bg-accent rounded-full animate-bounce [animation-delay:150ms]" />
                  <span className="w-1 h-1 bg-accent rounded-full animate-bounce [animation-delay:300ms]" />
                </div>
                {message.nodeId && (
                  <div className="flex flex-col gap-0.5">
                    <div className="flex items-center gap-1 text-[10px] text-violet-300">
                      <Network className="w-3 h-3" />
                      <span className="font-medium">Executing on network node</span>
                    </div>
                    <span className="font-mono text-[10px] text-muted">{message.nodeId.slice(-20)}</span>
                  </div>
                )}
              </div>
            ) : (
              <span className="text-muted text-sm">…</span>
            )}
          </div>
        )}

        {/* Proof of Inference receipt */}
        {!isUser && message.receipt && (
          <ReceiptPanel receipt={message.receipt} />
        )}

        {/* Footer: timestamp + duration + node */}
        <div className="mt-2 flex items-center gap-3 text-[10px] text-muted opacity-0 group-hover:opacity-100 transition-opacity">
          <span>
            {new Date(message.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
          </span>

          {message.durationMs !== undefined && (
            <span className="flex items-center gap-1">
              <Clock className="w-2.5 h-2.5" />
              {formatDuration(message.durationMs)}
            </span>
          )}

          {message.nodeId && (
            <span className="font-mono">via {message.nodeId.slice(0, 12)}…</span>
          )}
        </div>
      </div>
    </div>
  );
});
