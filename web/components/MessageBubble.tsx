'use client';

import { memo, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { User, Bot, ChevronDown, ChevronRight, Lock, Server, Network, Clock, FileText, CheckCircle2, XCircle } from 'lucide-react';
import clsx from 'clsx';
import type { Message, MessageReceipt } from '@/lib/session-store';

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
          <Row label="Settlement"  value={receipt.settlementId} />
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
            {isUser ? 'You' : 'DeAI'}
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
            ) : inThink ? null : (
              <span className="text-muted text-sm">Thinking…</span>
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
