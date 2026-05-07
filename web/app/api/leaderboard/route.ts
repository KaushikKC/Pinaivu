import { createHash } from 'node:crypto';
import { NextResponse } from 'next/server';

const RPC_URL    = process.env.PINAIVU_SOLANA_RPC    ?? 'https://api.devnet.solana.com';
const PROGRAM_ID = process.env.PINAIVU_PROGRAM_ID    ?? '';

// NodeScore account size matches state.rs LEN = 183
const NODE_SCORE_SIZE = 183;

export interface NodeScoreEntry {
  /** Base58 address of the NodeScore PDA account */
  pubkey: string;
  /** Hex-encoded 32-byte Ed25519 P2P pubkey of the node */
  nodePubkey: string;
  /** 0–1_000_000_000  (integer, scale by 1e9 to get 0–1 fraction) */
  score: number;
  totalJobs: number;
  /** 0–10_000 (divide by 100 for %) */
  successRateBps: number;
  avgLatencyMs: number;
  totalLamportsEarned: number;
  totalTokensEarned: number;
  /** Unix timestamp (seconds) */
  lastUpdated: number;
}

export interface LeaderboardResponse {
  error:   string | null;
  entries: NodeScoreEntry[];
  /** Unix ms when the data was fetched */
  fetchedAt: number;
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/** Anchor account discriminant: sha256("account:<Name>")[0..8] as base64 */
function anchorDiscriminant(name: string): string {
  return createHash('sha256')
    .update(`account:${name}`)
    .digest()
    .subarray(0, 8)
    .toString('base64');
}

function readU64LE(buf: Buffer, offset: number): number {
  const lo = buf.readUInt32LE(offset);
  const hi = buf.readUInt32LE(offset + 4);
  // Safe for values up to ~2^53 — score, lamports, tokens all fit
  return lo + hi * 2 ** 32;
}

function readI64LE(buf: Buffer, offset: number): number {
  const lo = buf.readUInt32LE(offset);
  const hi = buf.readInt32LE(offset + 4);
  return lo + hi * 2 ** 32;
}

/**
 * Decode a raw NodeScore account buffer.
 *
 * Byte layout (mirrors state.rs NodeScore):
 *   [0]   discriminant      8
 *   [8]   node_pubkey       32
 *   [40]  authority         32
 *   [72]  merkle_root       32
 *   [104] merkle_root_label 32
 *   [136] total_jobs        u64
 *   [144] total_tokens      u64
 *   [152] total_lamports    u64
 *   [160] success_rate_bps  u16
 *   [162] avg_latency_ms    u32
 *   [166] score             u64
 *   [174] last_updated      i64
 *   [182] bump              u8
 */
function decodeNodeScore(accountPubkey: string, data: Buffer): NodeScoreEntry {
  if (data.length < NODE_SCORE_SIZE) {
    throw new Error(`account data too short: ${data.length}`);
  }
  return {
    pubkey:              accountPubkey,
    nodePubkey:          data.subarray(8, 40).toString('hex'),
    totalJobs:           readU64LE(data, 136),
    totalTokensEarned:   readU64LE(data, 144),
    totalLamportsEarned: readU64LE(data, 152),
    successRateBps:      data.readUInt16LE(160),
    avgLatencyMs:        data.readUInt32LE(162),
    score:               readU64LE(data, 166),
    lastUpdated:         readI64LE(data, 174),
  };
}

// ─── Route ───────────────────────────────────────────────────────────────────

export async function GET() {
  const notDeployed =
    !PROGRAM_ID || PROGRAM_ID.startsWith('PiNaivuXXX') || PROGRAM_ID.length < 32;

  if (notDeployed) {
    return NextResponse.json<LeaderboardResponse>({
      error:     'not_deployed',
      entries:   [],
      fetchedAt: Date.now(),
    });
  }

  const disc = anchorDiscriminant('NodeScore');

  let rpcRes: Response;
  try {
    rpcRes = await fetch(RPC_URL, {
      method:  'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        jsonrpc: '2.0',
        id:      1,
        method:  'getProgramAccounts',
        params: [
          PROGRAM_ID,
          {
            encoding: 'base64',
            filters: [
              { dataSize: NODE_SCORE_SIZE },
              { memcmp: { offset: 0, bytes: disc, encoding: 'base64' } },
            ],
          },
        ],
      }),
      // Revalidate every 30 s so the Next.js cache doesn't serve stale data forever
      next: { revalidate: 30 },
    } as RequestInit & { next?: { revalidate?: number } });
  } catch (err) {
    return NextResponse.json<LeaderboardResponse>(
      { error: `rpc_unreachable: ${String(err)}`, entries: [], fetchedAt: Date.now() },
      { status: 502 },
    );
  }

  if (!rpcRes.ok) {
    return NextResponse.json<LeaderboardResponse>(
      { error: `rpc_http_${rpcRes.status}`, entries: [], fetchedAt: Date.now() },
      { status: 502 },
    );
  }

  const json = await rpcRes.json();
  if (json.error) {
    return NextResponse.json<LeaderboardResponse>(
      { error: String(json.error.message ?? json.error), entries: [], fetchedAt: Date.now() },
      { status: 502 },
    );
  }

  type RpcAccount = { pubkey: string; account: { data: [string, string] } };
  const accounts: RpcAccount[] = json.result ?? [];

  const entries: NodeScoreEntry[] = accounts
    .flatMap(a => {
      try {
        const raw = Buffer.from(a.account.data[0], 'base64');
        return [decodeNodeScore(a.pubkey, raw)];
      } catch {
        return [];
      }
    })
    .sort((a, b) => b.score - a.score);

  return NextResponse.json<LeaderboardResponse>({
    error:     null,
    entries,
    fetchedAt: Date.now(),
  });
}
