/**
 * POST /api/node-proxy
 *
 * Proxies a streaming inference request to any deai-node in the network.
 * The browser can't talk directly to peer nodes due to CORS, so it sends
 * the request here with the target node's api_url in the X-Node-Url header.
 *
 * Body: same InferRequest JSON that /v1/infer accepts.
 * Header: X-Node-Url — full base URL of the target node (e.g. http://127.0.0.1:4003)
 */
export const dynamic = 'force-dynamic';

export async function POST(req: Request) {
  const nodeUrl = req.headers.get('x-node-url');
  if (!nodeUrl) {
    return new Response(JSON.stringify({ error: 'Missing X-Node-Url header' }), {
      status: 400,
      headers: { 'Content-Type': 'application/json' },
    });
  }

  // Validate — only allow loopback / LAN targets to prevent SSRF to the open web.
  let parsed: URL;
  try {
    parsed = new URL(nodeUrl);
  } catch {
    return new Response(JSON.stringify({ error: 'Invalid X-Node-Url' }), {
      status: 400,
      headers: { 'Content-Type': 'application/json' },
    });
  }

  const allowedHosts = /^(localhost|127\.\d+\.\d+\.\d+|10\.\d+\.\d+\.\d+|192\.168\.\d+\.\d+|172\.(1[6-9]|2\d|3[01])\.\d+\.\d+)$/;
  if (!allowedHosts.test(parsed.hostname)) {
    return new Response(JSON.stringify({ error: 'Target host not in allowed range' }), {
      status: 403,
      headers: { 'Content-Type': 'application/json' },
    });
  }

  const body = await req.arrayBuffer();
  const upstream = await fetch(`${nodeUrl}/v1/infer`, {
    method:  'POST',
    headers: { 'Content-Type': 'application/json' },
    body,
    // @ts-expect-error — Node.js fetch supports duplex streaming
    duplex: 'half',
  });

  if (!upstream.body) {
    return new Response(JSON.stringify({ error: 'Upstream returned no body' }), {
      status: 502,
      headers: { 'Content-Type': 'application/json' },
    });
  }

  return new Response(upstream.body, {
    status: upstream.status,
    headers: {
      'Content-Type': upstream.headers.get('Content-Type') ?? 'application/x-ndjson',
      'Cache-Control': 'no-cache',
      'X-Accel-Buffering': 'no',
    },
  });
}
