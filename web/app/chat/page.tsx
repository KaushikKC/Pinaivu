'use client';

import { useEffect } from 'react';
import { useRouter } from 'next/navigation';
import { AppShell } from '@/components/AppShell';
import { listSessions, createSession } from '@/lib/session-store';
import { fetchModels } from '@/lib/daemon';
import { Cpu } from 'lucide-react';

/**
 * /chat — redirect to the most recent session, or create one if none exist.
 * Creates the session with the first model available from the daemon.
 */
export default function ChatIndexPage() {
  const router = useRouter();

  useEffect(() => {
    const sessions = listSessions();
    if (sessions.length > 0) {
      router.replace(`/chat/${sessions[0].id}`);
      return;
    }
    // Fetch the real model list so we don't hardcode a model name
    fetchModels()
      .then(ms => ms[0]?.name ?? 'deepseek-r1:7b')
      .catch(() => 'deepseek-r1:7b')
      .then(modelId => {
        const s = createSession(modelId);
        router.replace(`/chat/${s.id}`);
      });
  }, [router]);

  return (
    <AppShell>
      <div className="flex-1 flex items-center justify-center text-muted gap-3">
        <Cpu className="w-5 h-5 text-accent animate-pulse" />
        <span className="text-sm">Loading…</span>
      </div>
    </AppShell>
  );
}
