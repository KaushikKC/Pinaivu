'use client';

import {
  useEffect,
  useRef,
  useState,
  useCallback,
  type KeyboardEvent,
} from 'react';
import { Send, StopCircle, AlertTriangle, Cpu, Lock, ShieldCheck, Network } from 'lucide-react';
import clsx from 'clsx';
import { MessageBubble } from './MessageBubble';
import { useStream } from '@/lib/hooks/useStream';
import {
  getSession,
  appendMessage,
  type SessionRecord,
} from '@/lib/session-store';
import { fetchModels, fetchPeers } from '@/lib/daemon';

interface Props {
  sessionId: string;
}

export function ChatWindow({ sessionId }: Props) {
  const [session,    setSession]    = useState<SessionRecord | null>(null);
  const [input,      setInput]      = useState('');
  const [model,      setModel]      = useState('');
  const [models,     setModels]     = useState<string[]>([]);
  const [daemonDown, setDaemonDown] = useState(false);
  const [peerCount,  setPeerCount]  = useState<number | null>(null);

  const bottomRef   = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Load session from localStorage
  const reload = useCallback(() => {
    const s = getSession(sessionId);
    setSession(s);
    if (s?.modelId) setModel(s.modelId);
  }, [sessionId]);

  useEffect(() => { reload(); }, [reload]);

  // Fetch available models from daemon; auto-select first if current model unavailable
  useEffect(() => {
    fetchModels()
      .then(ms => {
        const names = ms.map(m => m.name).filter(n => n && !n.includes('guard') && !n.includes('embed'));
        setModels(names);
        setModel(prev => (names.includes(prev) ? prev : (names[0] ?? prev)));
      })
      .catch(() => setDaemonDown(true));
  }, []);

  // Poll peer count every 10 seconds
  useEffect(() => {
    let cancelled = false;
    async function poll() {
      try {
        const { count } = await fetchPeers();
        if (!cancelled) setPeerCount(count);
      } catch { /* standalone or daemon down */ }
    }
    poll();
    const timer = setInterval(poll, 10_000);
    return () => { cancelled = true; clearInterval(timer); };
  }, []);

  const { streaming, streamingText, error, executingNode, send, abort } = useStream(
    session ?? { id: sessionId, modelId: model, title: '', createdAt: 0, updatedAt: 0, messages: [] },
    (updated) => setSession(updated),
  );

  // Auto-scroll to bottom
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [session?.messages, streamingText]);

  // Auto-resize textarea
  useEffect(() => {
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
      textareaRef.current.style.height = `${Math.min(textareaRef.current.scrollHeight, 200)}px`;
    }
  }, [input]);

  async function handleSend() {
    const text = input.trim();
    if (!text || streaming || !session) return;
    setInput('');

    // Reset textarea height
    if (textareaRef.current) textareaRef.current.style.height = 'auto';

    await send(text, model);
    reload();
  }

  function handleKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }

  if (!session) {
    return (
      <div className="flex-1 flex items-center justify-center text-muted text-sm">
        Loading…
      </div>
    );
  }

  const messages = session.messages;
  const isEmpty  = messages.length === 0 && !streaming;

  return (
    <div className="flex flex-col h-full">
      {/* Top bar */}
      <div className="flex items-center justify-between px-4 py-2.5 border-b border-surface-2 bg-surface-1 flex-shrink-0">
        <div className="flex items-center gap-2">
          <Cpu className="w-4 h-4 text-accent" />
          {models.length > 0 ? (
            <select
              value={model}
              onChange={e => setModel(e.target.value)}
              className="text-sm bg-transparent text-white border-none outline-none cursor-pointer"
              disabled={streaming}
            >
              {models.map(m => (
                <option key={m} value={m} className="bg-surface-2">{m}</option>
              ))}
            </select>
          ) : (
            <span className="text-sm text-white font-medium">{model}</span>
          )}
        </div>

        <div className="flex items-center gap-3">
          {/* Network / Standalone badge */}
          {peerCount !== null && peerCount > 0 ? (
            <div className="flex items-center gap-1 text-[10px] text-violet-300 font-medium">
              <Network className="w-3 h-3" />
              <span>{peerCount} peer{peerCount !== 1 ? 's' : ''}</span>
            </div>
          ) : (
            <div className="flex items-center gap-1 text-[10px] text-muted font-medium">
              <Network className="w-3 h-3" />
              <span>Standalone</span>
            </div>
          )}
          <div className="flex items-center gap-1.5 text-[10px] text-emerald-400 font-medium">
            <ShieldCheck className="w-3 h-3" />
            <span>No logs · Private</span>
          </div>
          <div className="flex items-center gap-1 text-[10px] text-blue-400 font-medium">
            <Lock className="w-3 h-3" />
            <span>AES-256-GCM</span>
          </div>
          <span className="text-xs text-muted font-mono">
            {session.messages.length > 0
              ? `${Math.ceil(session.messages.length / 2)} turn${Math.ceil(session.messages.length / 2) !== 1 ? 's' : ''}`
              : 'new chat'}
          </span>
        </div>
      </div>

      {/* Messages */}
      <div className="flex-1 overflow-y-auto">
        {isEmpty && (
          <EmptyState model={model} daemonDown={daemonDown} />
        )}

        {messages.map((msg, i) => {
          const isLastAssistant =
            !streaming &&
            msg.role === 'assistant' &&
            i === messages.length - 1;

          return (
            <MessageBubble
              key={msg.id}
              message={msg}
              streaming={streaming && isLastAssistant}
            />
          );
        })}

        {/* Streaming placeholder shown until the first token arrives */}
        {streaming && messages.length > 0 && messages[messages.length - 1].role === 'user' && (
          <div className="flex gap-3 px-4 py-5 bg-surface-1">
            <div className="flex-shrink-0 w-7 h-7 rounded-full bg-surface-3 flex items-center justify-center">
              <div className="flex gap-1 items-center">
                <span className="w-1 h-1 bg-accent rounded-full animate-bounce [animation-delay:0ms]" />
                <span className="w-1 h-1 bg-accent rounded-full animate-bounce [animation-delay:150ms]" />
                <span className="w-1 h-1 bg-accent rounded-full animate-bounce [animation-delay:300ms]" />
              </div>
            </div>
            <div className="flex-1 min-w-0">
              <div className="text-[11px] font-medium text-muted mb-1.5">Pinaivu</div>
              {executingNode ? (
                <div className="flex flex-col gap-1">
                  <div className="flex items-center gap-1.5 text-[10px] text-emerald-400">
                    <Network className="w-3 h-3" />
                    <span className="font-medium">Executing on network node</span>
                  </div>
                  <div className="font-mono text-[10px] text-muted truncate max-w-xs">
                    {executingNode.node_peer_id}
                  </div>
                  <div className="flex items-center gap-2 text-[10px] text-muted">
                    <span>{executingNode.model_id}</span>
                    <span>·</span>
                    <span>{executingNode.accepted_settlements[0]?.price_per_1k ?? '?'} /1k tokens</span>
                    <span>·</span>
                    <span>~{executingNode.estimated_latency_ms}ms</span>
                  </div>
                </div>
              ) : (
                <p className="text-sm text-gray-400">Generating…</p>
              )}
            </div>
          </div>
        )}

        {error && (
          <div className="flex items-center gap-2 mx-4 my-2 px-3 py-2 rounded-lg bg-red-900/20 border border-red-800/40 text-red-400 text-sm">
            <AlertTriangle className="w-4 h-4 flex-shrink-0" />
            <span>{error}</span>
          </div>
        )}

        <div ref={bottomRef} />
      </div>

      {/* Input area */}
      <div className="flex-shrink-0 px-4 pb-4 pt-2">
        <div className="relative flex items-end gap-2 rounded-xl border border-surface-3
                        bg-surface-1 focus-within:border-accent/60 transition-colors">
          <textarea
            ref={textareaRef}
            value={input}
            onChange={e => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={streaming ? 'Generating…' : 'Send a message… (Shift+Enter for newline)'}
            disabled={streaming || daemonDown}
            rows={1}
            className={clsx(
              'flex-1 resize-none bg-transparent px-4 py-3 text-sm text-white',
              'placeholder:text-muted outline-none max-h-[200px] min-h-[48px]',
              (streaming || daemonDown) && 'opacity-50 cursor-not-allowed',
            )}
          />

          <div className="flex-shrink-0 pr-2 pb-2">
            {streaming ? (
              <button
                onClick={abort}
                className="p-2 rounded-lg bg-red-600/20 hover:bg-red-600/40 text-red-400 transition-colors"
                title="Stop generating"
              >
                <StopCircle className="w-4 h-4" />
              </button>
            ) : (
              <button
                onClick={handleSend}
                disabled={!input.trim() || daemonDown}
                className={clsx(
                  'p-2 rounded-lg transition-colors',
                  input.trim() && !daemonDown
                    ? 'bg-accent hover:bg-accent-hover text-white'
                    : 'bg-surface-3 text-muted cursor-not-allowed',
                )}
                title="Send (Enter)"
              >
                <Send className="w-4 h-4" />
              </button>
            )}
          </div>
        </div>

        {daemonDown && (
          <p className="text-xs text-red-400 mt-1.5 px-1">
            Cannot reach <code>pinaivu</code> daemon at localhost:4002.
            Run <code className="text-accent">pinaivu start --mode standalone</code>.
          </p>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Empty state
// ---------------------------------------------------------------------------

function EmptyState({ model, daemonDown }: { model: string; daemonDown: boolean }) {
  return (
    <div className="flex flex-col items-center justify-center h-full gap-6 px-6 text-center">
      <div className="w-16 h-16 rounded-2xl bg-accent/10 border border-accent/20
                      flex items-center justify-center">
        <Cpu className="w-8 h-8 text-accent" />
      </div>

      <div>
        <h2 className="text-xl font-semibold text-white mb-1">
          Pinaivu — Decentralised Intelligence
        </h2>
        <p className="text-muted text-sm max-w-sm">
          Your conversations are end-to-end encrypted. No company logs your prompts.
          GPU nodes bid to run your inference.
        </p>
      </div>

      <div className="grid grid-cols-1 gap-2 w-full max-w-sm text-left">
        {daemonDown ? (
          <div className="rounded-lg border border-red-800/40 bg-red-900/10 px-4 py-3 text-sm text-red-400">
            <span className="font-medium">Daemon offline.</span>
            {' '}Run:{' '}
            <code className="font-mono text-xs">pinaivu start --mode standalone</code>
          </div>
        ) : (
          <>
            {[
              ['Explain quantum entanglement simply', 'Science'],
              ['Write a Rust async TCP echo server', 'Code'],
              ['Summarise the Byzantine fault tolerance problem', 'CS'],
            ].map(([prompt, label]) => (
              <SuggestionChip key={label} prompt={prompt} label={label} model={model} />
            ))}
          </>
        )}
      </div>
    </div>
  );
}

function SuggestionChip({
  prompt,
  label,
  model: _model,
}: {
  prompt: string;
  label:  string;
  model:  string;
}) {
  return (
    <button
      className="flex items-center justify-between rounded-lg border border-surface-3
                 bg-surface-1 hover:bg-surface-2 hover:border-accent/40 px-4 py-3
                 text-sm text-left transition-colors group"
      onClick={() => {
        const textarea = document.querySelector('textarea');
        if (textarea) {
          const nativeInputValueSetter = Object.getOwnPropertyDescriptor(
            window.HTMLTextAreaElement.prototype,
            'value',
          )?.set;
          nativeInputValueSetter?.call(textarea, prompt);
          textarea.dispatchEvent(new Event('input', { bubbles: true }));
          textarea.focus();
        }
      }}
    >
      <span className="text-gray-300 group-hover:text-white">{prompt}</span>
      <span className="ml-3 flex-shrink-0 text-[10px] font-medium text-muted
                       bg-surface-3 rounded px-1.5 py-0.5">
        {label}
      </span>
    </button>
  );
}
