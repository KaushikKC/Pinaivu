/**
 * session-store.ts — Client-side session persistence using localStorage.
 *
 * In standalone mode, sessions are stored locally in the browser.
 * Each session holds its full message history so the web UI can render
 * conversation context without hitting the daemon.
 *
 * Storage layout:
 *   localStorage['deai:sessions'] = JSON array of SessionRecord[]
 */

export type Role = 'user' | 'assistant';

export interface MessageReceipt {
  proofId:       string;
  settlementId:  string;
  proofValid:    boolean;
  inputTokens:   number;
  outputTokens:  number;
  latencyMs:     number;
  nodePubkey:    string;
  signature:     string;
  canonicalHex:  string;
}

export interface Message {
  id:          string;
  role:        Role;
  content:     string;
  timestamp:   number;
  nodeId?:     string;
  durationMs?: number;
  receipt?:    MessageReceipt;
}

export interface SessionRecord {
  id:          string;
  modelId:     string;
  title:       string;       // first user message truncated
  createdAt:   number;
  updatedAt:   number;
  messages:    Message[];
}

const STORAGE_KEY = 'deai:sessions';

// ---------------------------------------------------------------------------
// Read / write helpers
// ---------------------------------------------------------------------------

function readAll(): SessionRecord[] {
  if (typeof window === 'undefined') return [];
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? (JSON.parse(raw) as SessionRecord[]) : [];
  } catch {
    return [];
  }
}

function writeAll(sessions: SessionRecord[]): void {
  if (typeof window === 'undefined') return;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(sessions));
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export function listSessions(): SessionRecord[] {
  return readAll().sort((a, b) => b.updatedAt - a.updatedAt);
}

export function getSession(id: string): SessionRecord | null {
  return readAll().find(s => s.id === id) ?? null;
}

export function createSession(modelId: string): SessionRecord {
  const now = Date.now();
  const session: SessionRecord = {
    id:        crypto.randomUUID(),
    modelId,
    title:     'New chat',
    createdAt: now,
    updatedAt: now,
    messages:  [],
  };
  const all = readAll();
  writeAll([session, ...all]);
  return session;
}

export function deleteSession(id: string): void {
  writeAll(readAll().filter(s => s.id !== id));
}

export function appendMessage(
  sessionId: string,
  message:   Omit<Message, 'id' | 'timestamp'>,
): Message {
  const msg: Message = {
    ...message,
    id:        crypto.randomUUID(),
    timestamp: Date.now(),
  };

  const all = readAll();
  const idx = all.findIndex(s => s.id === sessionId);
  if (idx === -1) throw new Error(`session ${sessionId} not found`);

  const session = { ...all[idx] };
  session.messages  = [...session.messages, msg];
  session.updatedAt = Date.now();

  // Auto-title from first user message
  if (session.title === 'New chat' && message.role === 'user') {
    session.title = message.content.slice(0, 60) + (message.content.length > 60 ? '…' : '');
  }

  all[idx] = session;
  writeAll(all);
  return msg;
}

export function updateSessionTitle(sessionId: string, title: string): void {
  const all = readAll();
  const idx = all.findIndex(s => s.id === sessionId);
  if (idx === -1) return;
  all[idx] = { ...all[idx], title };
  writeAll(all);
}

/** Update the last assistant message's content (used during streaming). */
export function updateLastAssistantMessage(
  sessionId:  string,
  content:    string,
  extra?:     Partial<Pick<Message, 'durationMs' | 'nodeId' | 'receipt'>>,
): void {
  const all = readAll();
  const idx = all.findIndex(s => s.id === sessionId);
  if (idx === -1) return;

  const msgs = [...all[idx].messages];
  const lastIdx = msgs.length - 1;
  if (lastIdx >= 0 && msgs[lastIdx].role === 'assistant') {
    msgs[lastIdx] = { ...msgs[lastIdx], content, ...extra };
    all[idx] = { ...all[idx], messages: msgs, updatedAt: Date.now() };
    writeAll(all);
  }
}
