'use client';

import { useEffect, useState, useCallback } from 'react';
import Link from 'next/link';
import { useRouter, usePathname } from 'next/navigation';
import {
  PlusCircle,
  MessageSquare,
  Trash2,
  Settings,
  Network,
  ChevronLeft,
  ChevronRight,
} from 'lucide-react';
import clsx from 'clsx';
import {
  listSessions,
  createSession,
  deleteSession,
  type SessionRecord,
} from '@/lib/session-store';

const DEFAULT_MODEL = 'deepseek-r1:7b';

interface Props {
  collapsed:  boolean;
  onCollapse: (v: boolean) => void;
}

export function SessionSidebar({ collapsed, onCollapse }: Props) {
  const [sessions, setSessions] = useState<SessionRecord[]>([]);
  const router   = useRouter();
  const pathname = usePathname();

  const refresh = useCallback(() => setSessions(listSessions()), []);

  useEffect(() => {
    refresh();
    // Refresh when localStorage changes (other tabs / same tab after navigation)
    const onStorage = (e: StorageEvent) => {
      if (e.key === 'deai:sessions') refresh();
    };
    window.addEventListener('storage', onStorage);
    return () => window.removeEventListener('storage', onStorage);
  }, [refresh]);

  // Keyboard shortcut: Cmd/Ctrl+K → new chat
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault();
        handleNew();
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  });

  function handleNew() {
    const session = createSession(DEFAULT_MODEL);
    refresh();
    router.push(`/chat/${session.id}`);
  }

  function handleDelete(e: React.MouseEvent, id: string) {
    e.preventDefault();
    e.stopPropagation();
    deleteSession(id);
    refresh();
    // If we deleted the active session, go to /chat
    if (pathname === `/chat/${id}`) router.push('/chat');
  }

  function formatDate(ts: number) {
    const d = new Date(ts);
    const now = new Date();
    const diffDays = Math.floor((now.getTime() - d.getTime()) / 86_400_000);
    if (diffDays === 0) return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    if (diffDays === 1) return 'Yesterday';
    if (diffDays < 7)  return d.toLocaleDateString([], { weekday: 'short' });
    return d.toLocaleDateString([], { month: 'short', day: 'numeric' });
  }

  return (
    <aside
      className={clsx(
        'flex flex-col h-full bg-surface-1 border-r border-surface-2 transition-all duration-200',
        collapsed ? 'w-12' : 'w-60',
      )}
    >
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-3 border-b border-surface-2">
        {!collapsed && (
          <span className="text-sm font-semibold text-accent tracking-wide">Pinaivu</span>
        )}
        <button
          onClick={() => onCollapse(!collapsed)}
          className="ml-auto p-1 rounded hover:bg-surface-2 text-muted hover:text-white transition-colors"
          title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
        >
          {collapsed ? <ChevronRight className="w-4 h-4" /> : <ChevronLeft className="w-4 h-4" />}
        </button>
      </div>

      {/* New Chat */}
      <div className="px-2 pt-2">
        <button
          onClick={handleNew}
          className={clsx(
            'w-full flex items-center gap-2 rounded-lg px-2.5 py-2 text-sm',
            'bg-accent/10 hover:bg-accent/20 text-accent hover:text-accent-hover',
            'transition-colors font-medium',
            collapsed ? 'justify-center' : '',
          )}
          title="New chat (⌘K)"
        >
          <PlusCircle className="w-4 h-4 flex-shrink-0" />
          {!collapsed && <span>New chat</span>}
        </button>
      </div>

      {/* Session list */}
      <nav className="flex-1 overflow-y-auto px-2 py-2 space-y-0.5">
        {sessions.length === 0 && !collapsed && (
          <p className="text-xs text-muted px-2 py-4 text-center">
            No chats yet.<br />Start one above.
          </p>
        )}

        {sessions.map(session => {
          const isActive = pathname === `/chat/${session.id}`;
          return (
            <Link
              key={session.id}
              href={`/chat/${session.id}`}
              className={clsx(
                'group flex items-center gap-2 rounded-lg px-2.5 py-2 text-sm transition-colors',
                isActive
                  ? 'bg-surface-3 text-white'
                  : 'text-muted hover:bg-surface-2 hover:text-white',
                collapsed ? 'justify-center' : '',
              )}
              title={collapsed ? session.title : undefined}
            >
              <MessageSquare className="w-3.5 h-3.5 flex-shrink-0" />

              {!collapsed && (
                <>
                  <div className="flex-1 min-w-0">
                    <div className="truncate text-[13px]">{session.title}</div>
                    <div className="text-[10px] text-muted mt-0.5">
                      {formatDate(session.updatedAt)}
                    </div>
                  </div>

                  <button
                    onClick={e => handleDelete(e, session.id)}
                    className={clsx(
                      'flex-shrink-0 p-1 rounded opacity-0 group-hover:opacity-100',
                      'hover:bg-surface-3 hover:text-red-400 text-muted transition-all',
                    )}
                    title="Delete chat"
                  >
                    <Trash2 className="w-3 h-3" />
                  </button>
                </>
              )}
            </Link>
          );
        })}
      </nav>

      {/* Footer nav */}
      {!collapsed && (
        <div className="border-t border-surface-2 px-2 py-2 space-y-0.5">
          <Link
            href="/nodes"
            className={clsx(
              'flex items-center gap-2 rounded-lg px-2.5 py-2 text-sm text-muted',
              'hover:bg-surface-2 hover:text-white transition-colors',
              pathname === '/nodes' && 'bg-surface-2 text-white',
            )}
          >
            <Network className="w-3.5 h-3.5" />
            <span>Network</span>
          </Link>
          <Link
            href="/settings"
            className={clsx(
              'flex items-center gap-2 rounded-lg px-2.5 py-2 text-sm text-muted',
              'hover:bg-surface-2 hover:text-white transition-colors',
              pathname === '/settings' && 'bg-surface-2 text-white',
            )}
          >
            <Settings className="w-3.5 h-3.5" />
            <span>Settings</span>
          </Link>
        </div>
      )}
    </aside>
  );
}
