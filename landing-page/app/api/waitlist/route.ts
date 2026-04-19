import { NextRequest, NextResponse } from 'next/server';
import { promises as fs } from 'fs';
import path from 'path';

const FILE = path.join(process.cwd(), 'waitlist.json');

export async function POST(req: NextRequest) {
  const body = await req.json();
  const email: string = (body.email || '').trim().toLowerCase();

  if (!email || !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) {
    return NextResponse.json({ error: 'Invalid email address.' }, { status: 400 });
  }

  let entries: { email: string; ts: string }[] = [];
  try {
    const raw = await fs.readFile(FILE, 'utf-8');
    entries = JSON.parse(raw);
  } catch {
    // file doesn't exist yet
  }

  if (entries.some(e => e.email === email)) {
    return NextResponse.json({ error: 'Already on the waitlist.' }, { status: 409 });
  }

  entries.push({ email, ts: new Date().toISOString() });
  await fs.writeFile(FILE, JSON.stringify(entries, null, 2));

  return NextResponse.json({ ok: true });
}
