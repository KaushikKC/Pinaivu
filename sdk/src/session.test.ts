/**
 * Session tests — use a mock transport so no real network or daemon is needed.
 */

import { describe, it, expect } from '@jest/globals';
import { Session, type Transport }  from './session.js';
import { SessionKey }               from './crypto.js';
import { MemoryStorage }            from './storage/memory.js';
import type { InferenceRequest, SessionContext } from './types.js';
import { randomUUID } from 'crypto';

// ---------------------------------------------------------------------------
// Stub transport — yields fixed tokens
// ---------------------------------------------------------------------------

class StubTransport implements Transport {
  constructor(private tokens: string[] = ['Hello', ' world', '!']) {}

  async *sendRequest(
    _req: InferenceRequest,
    _key: SessionKey,
    _bidWindowMs: number,
  ): AsyncIterable<string> {
    for (const token of this.tokens) {
      yield token;
    }
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeSession(tokens: string[] = ['Hello', ' world', '!']): Session {
  const now     = Math.floor(Date.now() / 1000);
  const context: SessionContext = {
    session_id:     randomUUID(),
    user_address:   '0xTest',
    model_id:       'llama3.1:8b',
    messages:       [],
    context_window: {
      system_prompt:   null,
      summary:         null,
      recent_messages: [],
      total_tokens:    0,
    },
    metadata: {
      created_at:        now,
      last_updated:      now,
      turn_count:        0,
      total_tokens_used: 0,
      total_cost_nanox:  0,
      walrus_blob_id:    null,
      prev_blob_id:      null,
    },
  };

  const key       = SessionKey.generate();
  const transport = new StubTransport(tokens);
  const storage   = new MemoryStorage();

  return new Session(context, key, transport, storage);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('Session.send()', () => {
  it('streams tokens from transport', async () => {
    const session = makeSession(['tok1', ' tok2', ' tok3']);
    const tokens: string[] = [];
    for await (const token of session.send('hi')) {
      tokens.push(token);
    }
    expect(tokens).toEqual(['tok1', ' tok2', ' tok3']);
  });

  it('appends user + assistant messages to history', async () => {
    const session = makeSession(['hi', '!']);
    await session.ask('hello');
    expect(session.messages).toHaveLength(2);
    expect(session.messages[0].role).toBe('user');
    expect(session.messages[0].content).toBe('hello');
    expect(session.messages[1].role).toBe('assistant');
    expect(session.messages[1].content).toBe('hi!');
  });

  it('increments turn count on each send', async () => {
    const session = makeSession();
    expect(session.turnCount).toBe(0);
    await session.ask('q1');
    expect(session.turnCount).toBe(1);
    await session.ask('q2');
    expect(session.turnCount).toBe(2);
  });

  it('ask() returns full joined reply', async () => {
    const session = makeSession(['Hello', ' ', 'world']);
    const reply   = await session.ask('hi');
    expect(reply).toBe('Hello world');
  });
});

describe('Session save / load', () => {
  it('save → load roundtrip', async () => {
    const session   = makeSession(['saved!']);
    const key       = SessionKey.fromBytes(session.exportKey());
    const storage   = new MemoryStorage();

    // Do a turn so there's content
    await session.ask('test question');

    // Manually save using the same storage
    const blobId = await saveToStorage(session, key, storage);

    // Load from storage
    const transport = new StubTransport();
    const loaded    = await Session.load(blobId, key, storage, transport);

    expect(loaded.sessionId).toBe(session.sessionId);
    expect(loaded.modelId).toBe('llama3.1:8b');
    expect(loaded.messages).toHaveLength(2);
  });
});

// Helper to save a session directly
async function saveToStorage(
  session: Session,
  key:     SessionKey,
  storage: MemoryStorage,
): Promise<string> {
  const { encryptJson } = await import('./crypto.js');
  // Access private context via any-cast for test
  const context = (session as any).context;
  const blob    = await encryptJson(context, key);
  return storage.put(blob.toBytes());
}
