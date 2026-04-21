'use client';

import { use } from 'react';
import { AppShell } from '@/components/AppShell';
import { ChatWindow } from '@/components/ChatWindow';

interface Props {
  params: Promise<{ sessionId: string }>;
}

export default function ChatSessionPage({ params }: Props) {
  const { sessionId } = use(params);
  return (
    <AppShell>
      <ChatWindow sessionId={sessionId} />
    </AppShell>
  );
}
