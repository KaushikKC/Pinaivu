'use client';

import { cloneElement, isValidElement, useCallback, useEffect, useRef, useState } from 'react';
import { useScrollProgress } from './hooks/useScrollProgress';
import { useRevealOnScroll } from './hooks/useRevealOnScroll';
import { useCounters } from './hooks/useCounters';
import { useModelTabs } from './hooks/useModelTabs';
import { useFlowLines } from './hooks/useFlowLines';
import { useCardEffects } from './hooks/useCardEffects';
import { useTicker } from './hooks/useTicker';
import { useHeroGradient } from './hooks/useHeroGradient';
import { useWordReveal } from './hooks/useWordReveal';
import { useFloatingLabels } from './hooks/useFloatingLabels';

const CornerSVG = () => (
  <svg viewBox="0 0 10 10"><path d="M0,10 L0,0 L10,0" stroke="currentColor" fill="none"/></svg>
);

function SecLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="mb-[22px] flex items-center gap-2.5 font-mono text-[10px] uppercase tracking-[3px] text-fg-3">
      <span className="inline-block h-px w-[22px] shrink-0 bg-fg-3"></span>
      {children}
    </div>
  );
}

const CORNER_POS = {
  tl: 'top-1 left-1',
  tr: 'top-1 right-1 rotate-90',
  bl: 'bottom-1 left-1 -rotate-90',
  br: 'bottom-1 right-1 rotate-180',
} as const;
const Corner = ({ pos }: { pos: keyof typeof CORNER_POS }) => (
  <div className={`pointer-events-none absolute size-2.5 opacity-0 transition-opacity duration-400ms ease-out group-hover:opacity-100 text-fg ${CORNER_POS[pos]}`}>
    <CornerSVG />
  </div>
);

function CursorGlow() {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const move = (e: MouseEvent) => {
      el.style.transform = `translate(${e.clientX - 14}px, ${e.clientY - 14}px)`;
      el.style.opacity = '1';
    };
    const leave = () => { if (el) el.style.opacity = '0'; };
    window.addEventListener('mousemove', move, { passive: true });
    document.documentElement.addEventListener('mouseleave', leave);
    return () => {
      window.removeEventListener('mousemove', move);
      document.documentElement.removeEventListener('mouseleave', leave);
    };
  }, []);
  return (
    <div
      ref={ref}
      aria-hidden="true"
      style={{ transform: 'translate(-200px,-200px)', opacity: 0, willChange: 'transform' }}
      className="pointer-events-none fixed top-0 left-0 z-9999 size-[10px] rounded-full blur-2xl transition-opacity duration-300 bg-fg/20"
    />
  );
}

function WaitlistModal({ onClose }: { onClose: () => void }) {
  const [email, setEmail] = useState('');
  const [status, setStatus] = useState<'idle'|'loading'|'success'|'error'>('idle');
  const [msg, setMsg] = useState('');

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setStatus('loading');
    try {
      const res = await fetch('/api/waitlist', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email }),
      });
      const data = await res.json();
      if (res.ok) {
        setStatus('success');
      } else {
        setStatus('error');
        setMsg(data.error || 'Something went wrong.');
      }
    } catch {
      setStatus('error');
      setMsg('Network error — please try again.');
    }
  }

  return (
    <div className="fixed inset-0 z-[999] bg-black/72 backdrop-blur-md flex items-center justify-center p-6 animate-[modal-in_.18s_ease-out]" onClick={onClose}>
      <div className="relative bg-bg-1 border border-line-h max-w-[480px] w-full px-9 py-10 animate-[modal-slide_.22s_cubic-bezier(.2,.8,.2,1)]" onClick={e => e.stopPropagation()}>
        <button onClick={onClose} aria-label="Close" className="absolute top-4 right-4 flex cursor-pointer border-none bg-none p-1 text-fg-3 transition-colors duration-200 hover:text-fg">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg>
        </button>
        <div className="mb-3.5 font-mono text-[9px] uppercase tracking-[0.28em] text-fg-3">Join the waitlist</div>
        {status === 'success' ? (
          <div className="flex flex-col items-start gap-3">
            <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="text-fg opacity-80"><path d="M5 12l5 5 9-9"/><circle cx="12" cy="12" r="10"/></svg>
            <h3 className="m-0 font-fraunces text-[25px] font-medium leading-[1.2] text-fg">You&apos;re on the list.</h3>
            <p className="m-0 text-[14px] leading-[1.6] text-fg-2">We&apos;ll reach out when early access opens. Phase C is live — Ed25519 identity and signed-receipt settlement work today.</p>
          </div>
        ) : (
          <>
            <h3 className="mb-2.5 mt-0 font-fraunces text-[25px] font-medium leading-[1.2] text-fg">Early access to Pinaivu AI</h3>
            <p className="mb-6 mt-0 text-[14px] leading-[1.6] text-fg-2">Be among the first to run a node or use the network. No token, no chain required.</p>
            <form onSubmit={submit} className="flex flex-col gap-2.5">
              <div className="flex overflow-hidden border border-line-h">
                <input
                  type="email"
                  placeholder="your@email.com"
                  value={email}
                  onChange={e => setEmail(e.target.value)}
                  required
                  autoFocus
                  className="flex-1 bg-transparent border-none outline-none py-3 px-4 font-mono text-[13px] text-fg placeholder:text-fg-4 min-w-0"
                />
                <button type="submit" disabled={status === 'loading'} className="flex cursor-pointer items-center gap-1 border-none bg-inv px-5 py-3 font-mono text-[12px] font-semibold uppercase tracking-[0.12em] text-inv-fg whitespace-nowrap transition-opacity duration-200 hover:opacity-85 disabled:cursor-default disabled:opacity-50">
                  {status === 'loading' ? <span className="w-[14px] h-[14px] border-2 border-inv-fg/30 border-t-inv-fg rounded-full animate-spin"></span> : <><span>Request Access</span><span className="arrow"> ↗</span></>}
                </button>
              </div>
              {status === 'error' && <div className="border border-[rgba(248,113,113,0.25)] bg-[rgba(248,113,113,0.06)] px-3 py-2 font-mono text-[12px] text-[#f87171]">{msg}</div>}
            </form>
            <div className="mt-2.5 font-mono text-[10px] tracking-[0.08em] text-fg-4">No spam. Unsubscribe any time.</div>
          </>
        )}
      </div>
    </div>
  );
}

export default function Home() {
  const [showWaitlist, setShowWaitlist] = useState(false);

  useScrollProgress();
  useRevealOnScroll();
  useCounters();
  useModelTabs();
  useFlowLines();
  useCardEffects();
  useTicker();
  useHeroGradient();
  useWordReveal();
  useFloatingLabels();

  const toggleTheme = useCallback(() => {
    const cur = document.documentElement.getAttribute('data-theme') || 'dark';
    const next = cur === 'dark' ? 'light' : 'dark';
    document.documentElement.setAttribute('data-theme', next);
    try { localStorage.setItem('pinaivu-theme', next); } catch { /* ignore */ }
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') setShowWaitlist(false); };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, []);

  return (
    <div className="relative z-1">
      <CursorGlow />
      {showWaitlist && <WaitlistModal onClose={() => setShowWaitlist(false)} />}
      {/* Scroll Progress */}
      <div className="prog fixed top-0 left-0 right-0 h-[2px] z-[200] pointer-events-none before:absolute before:inset-0 before:bg-fg-5"><div className="prog-bar absolute inset-0 bg-inv scale-x-0 origin-left transition-transform duration-100 [box-shadow:0_0_12px_var(--fg-2)]" id="progBar"></div></div>

      {/* NAV */}
      <nav
        className="fixed left-0 right-0 top-0 z-100 flex justify-center px-[28px] pt-4.5 transition-all duration-400 ease-[cubic-bezier(.16,1,.3,1)] [&.scrolled]:pt-3"
        id="nav"
      >
        <div className="pointer-events-auto flex w-full max-w-[1160px] items-center gap-6 rounded-full border border-line-2 bg-bg/86 px-[14px] py-[10px] pl-[18px] shadow-[0_10px_32px_rgba(0,0,0,.35)] backdrop-blur-[18px] transition-all duration-300 hover:border-line-h [&.scrolled]:bg-bg/92 [&.scrolled]:backdrop-blur-[26px] [&.scrolled]:shadow-[0_16px_48px_rgba(0,0,0,.45)] [&.scrolled]:border-line-h">
          <a
            href="#top"
            className="flex items-center gap-2.5 whitespace-nowrap font-mono text-[13px] font-semibold uppercase tracking-[0.8px]"
          >
            <span className="grid h-[22px] w-[22px] place-items-center">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="animate-[slow-spin_22s_linear_infinite]">
                <circle cx="12" cy="12" r="10"/>
                <circle cx="12" cy="12" r="5"/>
                <circle cx="12" cy="12" r="1.5" fill="currentColor"/>
                <line x1="12" y1="2" x2="12" y2="6"/>
                <line x1="12" y1="18" x2="12" y2="22"/>
                <line x1="2" y1="12" x2="6" y2="12"/>
                <line x1="18" y1="12" x2="22" y2="12"/>
              </svg>
            </span>
            Pinaivu AI
          </a>
          <ul className="hidden flex-1 items-center justify-center gap-1 md:flex">
            <li><a className="relative rounded-full px-3 py-1.5 font-mono text-[11px] uppercase tracking-[0.9px] text-fg-3 transition-colors hover:bg-fg-5 hover:text-fg after:absolute after:left-[10px] after:right-[10px] after:bottom-1 after:h-px after:bg-fg after:scale-x-0 after:origin-left after:transition-transform after:duration-350 after:ease-[cubic-bezier(.16,1,.3,1)] hover:after:scale-x-1" href="#problem">Problem</a></li>
            <li><a className="relative rounded-full px-3 py-1.5 font-mono text-[11px] uppercase tracking-[0.9px] text-fg-3 transition-colors hover:bg-fg-5 hover:text-fg after:absolute after:left-[10px] after:right-[10px] after:bottom-1 after:h-px after:bg-fg after:scale-x-0 after:origin-left after:transition-transform after:duration-350 after:ease-[cubic-bezier(.16,1,.3,1)] hover:after:scale-x-1" href="#features">Features</a></li>
            <li><a className="relative rounded-full px-3 py-1.5 font-mono text-[11px] uppercase tracking-[0.9px] text-fg-3 transition-colors hover:bg-fg-5 hover:text-fg after:absolute after:left-[10px] after:right-[10px] after:bottom-1 after:h-px after:bg-fg after:scale-x-0 after:origin-left after:transition-transform after:duration-350 after:ease-[cubic-bezier(.16,1,.3,1)] hover:after:scale-x-1" href="#flow">Flow</a></li>
            <li><a className="relative rounded-full px-3 py-1.5 font-mono text-[11px] uppercase tracking-[0.9px] text-fg-3 transition-colors hover:bg-fg-5 hover:text-fg after:absolute after:left-[10px] after:right-[10px] after:bottom-1 after:h-px after:bg-fg after:scale-x-0 after:origin-left after:transition-transform after:duration-350 after:ease-[cubic-bezier(.16,1,.3,1)] hover:after:scale-x-1" href="#models">Models</a></li>
            <li><a className="relative rounded-full px-3 py-1.5 font-mono text-[11px] uppercase tracking-[0.9px] text-fg-3 transition-colors hover:bg-fg-5 hover:text-fg after:absolute after:left-[10px] after:right-[10px] after:bottom-1 after:h-px after:bg-fg after:scale-x-0 after:origin-left after:transition-transform after:duration-350 after:ease-[cubic-bezier(.16,1,.3,1)] hover:after:scale-x-1" href="#tech">Tech</a></li>
            <li><a className="relative rounded-full px-3 py-1.5 font-mono text-[11px] uppercase tracking-[0.9px] text-fg-3 transition-colors hover:bg-fg-5 hover:text-fg after:absolute after:left-[10px] after:right-[10px] after:bottom-1 after:h-px after:bg-fg after:scale-x-0 after:origin-left after:transition-transform after:duration-350 after:ease-[cubic-bezier(.16,1,.3,1)] hover:after:scale-x-1" href="#roadmap">Roadmap</a></li>
          </ul>
          <button className="theme-toggle relative w-[46px] h-[26px] rounded-full bg-fg-5 border border-line cursor-pointer flex items-center p-[3px] transition-[background,border-color] duration-300 hover:border-line-h before:content-[''] before:absolute before:top-[3px] before:left-[3px] before:w-[18px] before:h-[18px] before:rounded-full before:bg-fg before:transition-transform before:duration-500 before:ease-[cubic-bezier(.16,1,.3,1)] light:before:translate-x-[20px]" onClick={toggleTheme} aria-label="Toggle theme">
            <svg className="sun w-[11px] h-[11px] relative z-2 text-inv-fg opacity-0 transition-opacity duration-300 light:opacity-100 ml-[3px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/></svg>
            <svg className="moon w-[11px] h-[11px] relative z-2 text-white opacity-100 transition-opacity duration-300 light:opacity-0 ml-auto mr-[3px]" viewBox="0 0 24 24" fill="currentColor" stroke="none"><path d="M21 12.79A9 9 0 1111.21 3 7 7 0 0021 12.79z"/></svg>
          </button>
          <button
            className="rounded-full bg-inv px-[18px] py-[9px] font-mono text-[11px] font-semibold uppercase tracking-[1.1px] text-inv-fg transition-opacity hover:opacity-90 relative overflow-hidden whitespace-nowrap flex-shrink-0"
            onClick={() => setShowWaitlist(true)}
          >
            <span className="relative z-2 flex items-center gap-1.5">Join Waitlist <span className="arrow">↗</span></span>
          </button>
        </div>
      </nav>

      {/* HERO */}
      <section className="hero relative flex min-h-screen pt-[140px] pb-0 flex-col overflow-hidden isolate bg-bg [transition:background_var(--theme-t)]" id="top">
        <canvas id="hero-canvas" className="hidden"></canvas>
        <div className="absolute inset-0 z-0 pointer-events-none [background:radial-gradient(circle_800px_at_var(--mx,50%)_var(--my,40%),var(--grad-a)_0%,var(--grad-b)_25%,transparent_70%),var(--grad-base)] [transition:background_.12s_ease-out]" id="heroGradient"></div>
        <div className="absolute inset-0 z-0 pointer-events-none bg-[linear-gradient(var(--fg-6)_1px,transparent_1px),linear-gradient(90deg,var(--fg-6)_1px,transparent_1px)] [background-size:16px_16px] opacity-60 mask-[linear-gradient(to_bottom,transparent,black_20%,black_80%,transparent)]"></div>
        <div className="absolute inset-0 z-0 pointer-events-none bg-[linear-gradient(var(--fg-6)_1px,transparent_1px),linear-gradient(90deg,var(--fg-6)_1px,transparent_1px)] [background-size:64px_64px] mask-[radial-gradient(ellipse_at_center,#000_20%,transparent_85%)]"></div>
        <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-px h-px z-2 pointer-events-none before:content-[''] before:absolute before:left-1/2 before:-top-[40vh] before:-bottom-[40vh] before:w-px before:bg-fg-4 after:content-[''] after:absolute after:top-1/2 after:-left-[50vw] after:-right-[50vw] after:h-px after:bg-fg-4"></div>
        <div className="absolute inset-0 z-1 pointer-events-none opacity-60 [background:radial-gradient(ellipse_120%_90%_at_50%_40%,transparent_30%,var(--bg)_100%)]"></div>
        <div className="scanlines"></div>
        <div className="absolute inset-[28px] z-2 pointer-events-none hidden md:block before:content-[''] before:absolute before:top-0 before:left-0 before:w-[28px] before:h-[28px] before:border before:border-line-h before:border-r-0 before:border-b-0 after:content-[''] after:absolute after:top-0 after:right-0 after:w-[28px] after:h-[28px] after:border after:border-line-h after:border-l-0 after:border-b-0">
          <span className="absolute bottom-0 left-0 w-[28px] h-[28px] border border-line-h border-r-0 border-t-0"></span>
          <span className="absolute bottom-0 right-0 w-[28px] h-[28px] border border-line-h border-l-0 border-t-0"></span>
        </div>
        {/* <div className="hero-glyphs" id="heroGlyphs"></div>
        <div className="hero-marker tl hidden md:flex"><span className="dot"></span> 0x<span id="heroHash">3f2a9b…c417</span></div> */}
        {/* <div className="hero-marker tr hidden md:flex">Ed25519 / Merkle / libp2p <span className="bar"></span></div> */}

        <div className="relative z-3 flex flex-1 flex-col items-center justify-center gap-8 text-center px-(--content-pad) pb-[120px] max-w-[1200px] w-full mx-auto">
          <div className="flex flex-col items-center gap-5">
            <div data-float className="inline-flex items-center gap-2.5 rounded-full border border-line-2 bg-fg-6 px-4 py-[7px] font-mono text-[10px] font-medium uppercase tracking-[0.25em] text-fg backdrop-blur-md shadow-[0_4px_24px_rgba(0,0,0,0.2)]">
              <span className="size-1.5 rounded-full bg-fg shadow-[0_0_6px_currentColor] animate-pulse-dot"></span>
              Phase C · Protocol v2.0 · Live
            </div>
            <div data-float className="flex items-center gap-4 font-mono text-[10px] uppercase tracking-[0.35em] text-fg-3">
              <span className="h-px w-8 bg-linear-to-r from-transparent to-fg-4"></span>
              A P2P Inference Protocol
              <span className="h-px w-8 bg-linear-to-l from-transparent to-fg-4"></span>
            </div>
          </div>

          <h1 className="max-w-[1100px] overflow-visible px-6 text-center font-fraunces text-[115px] leading-[0.9] tracking-[-4px] text-fg max-md:px-5 max-md:text-[64px] max-sm:text-[44px]">
            <span className="block opacity-0 animate-[hero-title-in_.9s_cubic-bezier(.16,1,.3,1)_.2s_forwards]">Trust&nbsp;from</span>
            <span className="block font-playfair italic font-normal text-fg-2 opacity-0 animate-[hero-title-in_.9s_cubic-bezier(.16,1,.3,1)_.42s_forwards]">cryptography,</span>
            <span className="block opacity-0 animate-[hero-title-in_.9s_cubic-bezier(.16,1,.3,1)_.64s_forwards]">not chains.</span>
          </h1>

          <p className="max-w-[750px] text-[18px] leading-[1.75] text-fg-2 opacity-0 animate-fade-in-1">
            Pinaivu AI grounds every guarantee in <strong className="text-fg font-medium bg-fg-6 px-1.5 py-0.5 rounded border border-line">Ed25519 signatures</strong> and <strong className="text-fg font-medium bg-fg-6 px-1.5 py-0.5 rounded border border-line">SHA-256 Merkle proofs</strong> — not a coordinator, not a token. Settlement, storage, and anchoring are <strong className="text-fg font-medium">pluggable</strong>. Swap a TOML value, not your stack.
          </p>

          <div className="mt-4 flex flex-wrap justify-center gap-4 opacity-0 animate-fade-in-2">
            <button className="relative inline-flex items-center gap-2.5 overflow-hidden isolate rounded-full bg-inv px-8 py-4 font-mono text-[12px] font-semibold uppercase tracking-[0.14em] text-inv-fg transition-all duration-250ms ease-out hover:scale-105" onClick={() => setShowWaitlist(true)}>
              <span className="relative z-2">Join Waitlist</span>
              <span className="relative z-2">↗</span>
            </button>
            <a className="relative inline-flex items-center gap-2.5 overflow-hidden isolate rounded-full border border-line-2 bg-bg-2/70 px-8 py-4 font-mono text-[12px] font-semibold uppercase tracking-[0.14em] text-fg backdrop-blur-md transition-all duration-250ms ease-out hover:border-fg hover:bg-bg-2 hover:scale-105" href="/PinaivuAI_Whitepaper.pdf" target="_blank" rel="noopener noreferrer">
              <span>Read Whitepaper v2.0</span>
            </a>
          </div>
        </div>

        {/* Floating Ticker Carousel */}
        <div className="absolute bottom-0 left-0 right-0 z-10 border-y border-line bg-bg/80 backdrop-blur-xl">
          {/* <div className="absolute -top-8 left-(--content-pad) font-mono text-[9px] tracking-[0.2em] text-[rgba(255,255,255,0.35)] uppercase hidden md:block">
            v2.0 · April 2026 · Living Document
          </div> */}
          <div className="flex h-[56px] w-full items-center overflow-hidden">
            <div
              className="flex h-full shrink-0 items-center gap-12 pl-12 font-mono text-[12px] font-medium uppercase tracking-[0.2em] text-fg-2 whitespace-nowrap animate-ticker"
              id="ticker"
            >
              <span className="inline-flex items-center gap-2 before:content-['◆'] before:text-fg before:text-[10px]">ED25519 IDENTITY</span>
              <span className="inline-flex items-center gap-2 before:content-['◆'] before:text-fg before:text-[10px]">SHA-256 MERKLE TREE</span>
              <span className="inline-flex items-center gap-2 before:content-['◆'] before:text-fg before:text-[10px]">GOSSIPSUB REPUTATION</span>
              <span className="inline-flex items-center gap-2 before:content-['◆'] before:text-fg before:text-[10px]">AES-256-GCM SESSIONS</span>
              <span className="inline-flex items-center gap-2 before:content-['◆'] before:text-fg before:text-[10px]">X25519 CONTEXT KEYS</span>
              <span className="inline-flex items-center gap-2 before:content-['◆'] before:text-fg before:text-[10px]">SIGNED PROOF OF INFERENCE</span>
              <span className="inline-flex items-center gap-2 before:content-['◆'] before:text-fg before:text-[10px]">SETTLEMENT-AGNOSTIC ESCROW</span>
              <span className="inline-flex items-center gap-2 before:content-['◆'] before:text-fg before:text-[10px]">LIBP2P · QUIC · NOISE</span>
              <span className="inline-flex items-center gap-2 before:content-['◆'] before:text-fg before:text-[10px]">IPFS · WALRUS · LOCAL</span>
              <span className="inline-flex items-center gap-2 before:content-['◆'] before:text-fg before:text-[10px]">FREE · RECEIPT · CHANNEL · SUI · EVM</span>
              <span className="inline-flex items-center gap-2 before:content-['◆'] before:text-fg before:text-[10px]">STANDARD · PRIVATE · FRAGMENTED · MAXIMUM</span>
              <span className="inline-flex items-center gap-2 before:content-['◆'] before:text-fg before:text-[10px]">OFFLINE VERIFIABLE</span>
            </div>
          </div>
        </div>
      </section>

      {/* STATS */}
      <section className="relative z-0 mt-8 p-0">
        <div className="mx-auto grid max-w-(--content-max) grid-cols-1 gap-0 px-(--content-pad) md:grid-cols-2 xl:grid-cols-4" id="stats">
          <div
            className="group relative -ml-px -mt-px border border-line px-8 py-10 transition-[background-color,border-color,box-shadow,transform] duration-400ms ease-[cubic-bezier(.16,1,.3,1)] hover:bg-bg-1 hover:border-line-h hover:shadow-[0_0_60px_var(--fg-5)_inset,0_20px_60px_rgba(0,0,0,.35)] hover:z-1 [&.in-view_.stat-bar]:w-(--pct,60%)"
            style={{ '--pct': '100%' } as React.CSSProperties}
          >
            <div className="mb-4 flex items-baseline gap-1 font-fraunces text-[38px] lg:text-[54px] leading-none tracking-[-2px]"><span data-count="5">0</span><span className="font-mono text-[13px] font-medium tracking-[0.26px] text-fg-2">/5</span></div>
            <div className="flex items-center justify-between font-mono text-[9px] uppercase tracking-[2.25px] text-fg-3">Guarantees met <span className="inline-flex items-center gap-1 font-semibold text-fg">G1–G5</span></div>
            <div className="stat-bar absolute bottom-0 left-0 h-[2px] w-0 bg-fg transition-[width] duration-[1600ms] ease-[cubic-bezier(.2,.8,.2,1)] delay-100"></div>
          </div>
          <div
            className="group relative -ml-px -mt-px border border-line px-8 py-10 transition-[background-color,border-color,box-shadow,transform] duration-400ms ease-[cubic-bezier(.16,1,.3,1)] hover:bg-bg-1 hover:border-line-h hover:shadow-[0_0_60px_var(--fg-5)_inset,0_20px_60px_rgba(0,0,0,.35)] hover:z-1 [&.in-view_.stat-bar]:w-(--pct,60%)"
            style={{ '--pct': '100%' } as React.CSSProperties}
          >
            <div className="mb-4 flex items-baseline gap-1 font-fraunces text-[38px] lg:text-[54px] leading-none tracking-[-2px]"><span data-count="0">0</span></div>
            <div className="flex items-center justify-between font-mono text-[9px] uppercase tracking-[2.25px] text-fg-3">Blockchains required <span className="inline-flex items-center gap-1 font-semibold text-fg">Optional</span></div>
            <div className="stat-bar absolute bottom-0 left-0 h-[2px] w-0 bg-fg transition-[width] duration-[1600ms] ease-[cubic-bezier(.2,.8,.2,1)] delay-100"></div>
          </div>
          <div
            className="group relative -ml-px -mt-px border border-line px-8 py-10 transition-[background-color,border-color,box-shadow,transform] duration-400ms ease-[cubic-bezier(.16,1,.3,1)] hover:bg-bg-1 hover:border-line-h hover:shadow-[0_0_60px_var(--fg-5)_inset,0_20px_60px_rgba(0,0,0,.35)] hover:z-1 [&.in-view_.stat-bar]:w-(--pct,60%)"
            style={{ '--pct': '100%' } as React.CSSProperties}
          >
            <div className="mb-4 flex items-baseline gap-1 font-fraunces text-[38px] lg:text-[54px] leading-none tracking-[-2px]"><span data-count="6">0</span></div>
            <div className="flex items-center justify-between font-mono text-[9px] uppercase tracking-[2.25px] text-fg-3">Stack layers <span className="inline-flex items-center gap-1 font-semibold text-fg">Swappable</span></div>
            <div className="stat-bar absolute bottom-0 left-0 h-[2px] w-0 bg-fg transition-[width] duration-[1600ms] ease-[cubic-bezier(.2,.8,.2,1)] delay-100"></div>
          </div>
          <div
            className="group relative -ml-px -mt-px border border-line px-8 py-10 transition-[background-color,border-color,box-shadow,transform] duration-400ms ease-[cubic-bezier(.16,1,.3,1)] hover:bg-bg-1 hover:border-line-h hover:shadow-[0_0_60px_var(--fg-5)_inset,0_20px_60px_rgba(0,0,0,.35)] hover:z-1 [&.in-view_.stat-bar]:w-(--pct,60%)"
            style={{ '--pct': '100%' } as React.CSSProperties}
          >
            <div className="mb-4 flex items-baseline gap-1 font-fraunces text-[38px] lg:text-[54px] leading-none tracking-[-2px]"><span data-count="128">0</span><span className="font-mono text-[13px] font-medium tracking-[0.26px] text-fg-2">-bit</span></div>
            <div className="flex items-center justify-between font-mono text-[9px] uppercase tracking-[2.25px] text-fg-3">Ed25519 security <span className="inline-flex items-center gap-1 font-semibold text-fg">RFC 8032</span></div>
            <div className="stat-bar absolute bottom-0 left-0 h-[2px] w-0 bg-fg transition-[width] duration-[1600ms] ease-[cubic-bezier(.2,.8,.2,1)] delay-100"></div>
          </div>
        </div>
      </section>

      {/* MANIFESTO */}
      <section
        id="manifesto"
        className="manifesto mx-auto max-w-(--content-max) px-(--content-pad) py-14"
      >
        {/* Header (same vibe as before, cleaner) */}
        <div className="reveal mb-6 flex items-center gap-4 border-b border-line pb-4">
          <div className="font-mono text-[10px] uppercase tracking-[0.32em] text-fg-3">
            <span className="font-semibold text-fg">§ 001</span> Thesis{" "}
            <span className="ml-2 text-fg-3">· Drafted for the open network · v2.0</span>
          </div>
          <div className="ml-auto hidden h-px flex-1 bg-line md:block" />
        </div>

        {/* Collage grid (similar to original, but readable) */}
        <div className="grid grid-cols-2 gap-0 border border-line bg-bg md:grid-cols-6">
          {/* Big thesis */}
          <div className="reveal col-span-2 md:col-span-4 md:row-span-3 -ml-px -mt-px border border-line bg-bg p-7 md:p-10">
            <div className="flex items-center justify-between gap-4 font-mono text-[10px] uppercase tracking-[0.25em] text-fg-3">
              <span>Abstract · Line 01</span>
              <span className="rounded-full border border-line-2 bg-fg-6 px-3 py-1 text-fg-2">Self-sufficient</span>
            </div>
            <h2 className="mt-4 font-fraunces text-[22px] lg:text-[36px] leading-[1.14] tracking-[-0.022em] text-fg [font-variation-settings:'opsz'_144]">
              Every prior inference marketplace grounds trust in a{" "}
              <span className="text-fg-2 italic font-light font-playfair">coordinator</span> or a{" "}
              <span className="text-fg-2 italic font-light font-playfair">specific chain</span>. Pinaivu AI takes a third path: trust is grounded{" "}
              <span className="rounded bg-inv px-1.5 text-inv-fg font-playfair">exclusively in cryptography</span> — Ed25519 identity, SHA-256 Merkle proofs,
              AES-256-GCM sessions. Any chain becomes an{" "}
              <span className="text-fg-2 italic font-light font-playfair">optional anchor</span> on a system that already works.
            </h2>
            <div className="mt-6 flex flex-wrap gap-4 font-mono text-[10px] uppercase tracking-[0.3em] text-fg-3">
              <span className="before:content-['◆_'] before:text-fg">Offline verifiable</span>
              <span className="before:content-['◆_'] before:text-fg">No coordinator</span>
              <span className="before:content-['◆_'] before:text-fg">Chain-optional</span>
            </div>
          </div>

          {/* Primitive */}
          <div className="reveal reveal-d1 col-span-2 md:col-span-2 md:row-span-2 -ml-px -mt-px border border-line bg-bg-1 p-6">
            <div className="flex items-start justify-between gap-4">
              <div>
                <div className="font-mono text-[9px] uppercase tracking-[0.22em] text-fg-3">Primitive · 01</div>
                <div className="mt-2 font-playfair text-[28px] leading-none tracking-[-0.02em] text-fg [font-variation-settings:'opsz'_144]">
                  Proof <span className="text-fg-2 italic font-light">of Inference</span>
                </div>
              </div>
              <div className="rounded-full border border-line-2 px-3 py-1 font-mono text-[10px] uppercase tracking-[0.2em] text-fg-3">
                π
              </div>
            </div>
            <p className="mt-4 text-[14px] leading-[1.65] text-fg-2">
              A signed execution receipt verifiable offline with only the producing node&apos;s public key.
            </p>
            <div className="mt-4 font-mono text-[10px] uppercase tracking-[0.22em] text-fg-3">
              π = (req, model, tᵢ, tₒ, Δ, H_in, H_out, pk, σ)
            </div>
          </div>

          {/* Code */}
          <div className="reveal reveal-d2 col-span-2 md:col-span-2 md:row-span-2 -ml-px -mt-px border border-line bg-[#0d0d0d] p-6">
            <div className="mb-3 font-mono text-[9px] uppercase tracking-[0.22em] text-white/40">Verify (offline)</div>
            <div className="space-y-2 font-mono text-[14px] leading-[1.7] text-white/60">
              <div className="text-white/35"># verify π offline — no network, no chain</div>
              <div><span className="font-semibold text-white">let</span> msg = canonical(π)</div>
              <div><span className="font-semibold text-white">let</span> vk  = VerifyingKey::<span className="text-white">from_bytes</span>(π.pk_N)</div>
              <div><span className="font-semibold text-white">assert</span> EdDSA::<span className="text-white">verify</span>(vk, msg, π.σ)</div>
              <div className="text-white/35"># O(1) — constant time</div>
            </div>
          </div>

          {/* G1 */}
          <div className="reveal col-span-1 md:col-span-1 -ml-px -mt-px border border-line bg-bg-1 p-5">
            <div className="font-mono text-[9px] uppercase tracking-[0.22em] text-fg-3">G1</div>
            <div className="mt-2 font-playfair text-[18px] tracking-[-0.015em] text-fg [font-variation-settings:'opsz'_144]">
              Session <span className="text-fg-2 italic font-light">privacy</span>
            </div>
            <div className="mt-3 font-mono text-[10px] uppercase tracking-[0.22em] text-fg-3">
              Client-held K · X25519 DH
            </div>
          </div>

          {/* G2 */}
          <div className="reveal reveal-d1 col-span-1 md:col-span-1 -ml-px -mt-px border border-line bg-bg-1 p-5">
            <div className="font-mono text-[9px] uppercase tracking-[0.22em] text-fg-3">G2</div>
            <div className="mt-2 font-playfair text-[18px] tracking-[-0.015em] text-fg [font-variation-settings:'opsz'_144]">
              Node <span className="text-fg-2 italic font-light">accountability</span>
            </div>
            <div className="mt-3 font-mono text-[10px] uppercase tracking-[0.22em] text-fg-3">
              Ed25519 σ · Merkle π
            </div>
          </div>

          {/* Zero chain */}
          <div className="reveal reveal-d2 col-span-1 md:col-span-1 grid place-items-center -ml-px -mt-px border border-line bg-bg-1 p-5 text-center">
            <div className="font-mono text-[9.5px] uppercase tracking-[0.22em] text-fg-3">
              <div className="mb-1 text-fg tracking-[0.15em] text-2xl">∅</div>
              Zero blockchain required
            </div>
          </div>

          {/* Dot matrix */}
          {/* <div className="col-span-1 md:col-span-1 relative overflow-hidden -ml-px -mt-px border border-line bg-bg-1 p-0">
            <div className="absolute inset-4 bg-[radial-gradient(circle,rgba(255,255,255,.5)_1px,transparent_1.2px)] bg-[length:10px_10px]" />
            <div className="absolute bottom-2 right-3 font-mono text-[10px] uppercase tracking-[0.2em] text-fg-3">n × n</div>
          </div> */}


          {/* Ring */}
          <div className="reveal reveal-d3 col-span-1 md:col-span-1 grid place-items-center -ml-px -mt-px border border-line bg-bg-1 p-5">
            <div className="relative grid h-[64px] w-[64px] place-items-center">
              <svg viewBox="0 0 70 70" className="h-full w-full -rotate-90 text-fg">
                <circle cx="35" cy="35" r="30" fill="none" stroke="var(--fg-5)" strokeWidth="2" />
                <circle cx="35" cy="35" r="30" fill="none" stroke="currentColor" strokeWidth="2" strokeDasharray="188" strokeDashoffset="47" />
              </svg>
              <div className="absolute font-mono text-[9px] uppercase tracking-[0.15em] text-fg">5/5</div>
            </div>
          </div>

          {/* G3 */}
          <div className="reveal col-span-1 md:col-span-2 -ml-px -mt-px border border-line bg-bg-1 p-5">
            <div className="font-mono text-[9px] uppercase tracking-[0.22em] text-fg-3">G3</div>
            <div className="mt-2 font-playfair text-[18px] tracking-[-0.015em] text-fg [font-variation-settings:'opsz'_144]">
              Settlement <span className="text-fg-2 italic font-light">neutrality</span>
            </div>
            <div className="mt-3 font-mono text-[10px] uppercase tracking-[0.22em] text-fg-3">
              free · receipt · channel · sui · evm
            </div>
          </div>

          {/* G4 */}
          <div className="reveal reveal-d1 col-span-1 md:col-span-2 -ml-px -mt-px border border-line bg-bg-1 p-5">
            <div className="font-mono text-[9px] uppercase tracking-[0.22em] text-fg-3">G4</div>
            <div className="mt-2 font-playfair text-[18px] tracking-[-0.015em] text-fg [font-variation-settings:'opsz'_144]">
              Storage <span className="text-fg-2 italic font-light">neutrality</span>
            </div>
            <div className="mt-3 font-mono text-[10px] uppercase tracking-[0.22em] text-fg-3">
              local · ipfs · walrus
            </div>
          </div>


          {/* G5 */}
          <div className="reveal reveal-d2 col-span-1 md:col-span-2 -ml-px -mt-px border border-line bg-bg-1 p-5">
            <div className="font-mono text-[9px] uppercase tracking-[0.22em] text-fg-3">G5</div>
            <div className="mt-2 font-playfair text-[18px] tracking-[-0.015em] text-fg [font-variation-settings:'opsz'_144]">
              Permissionless <span className="text-fg-2 italic font-light">participation</span>
            </div>
            <div className="mt-3 font-mono text-[10px] uppercase tracking-[0.22em] text-fg-3">
              libp2p PeerId = pk_N
            </div>
          </div>

          {/* Barchart */}
          <div className="reveal reveal-d3 col-span-2 md:col-span-2 flex items-end gap-1 -ml-px -mt-px border border-line bg-bg-1 p-5">
            {[40, 65, 30, 80, 55, 72, 48, 88, 62, 35].map((h, i) => (
              <div key={i} className="flex-1 rounded-sm bg-fg/35" style={{ height: `${h}%` }} />
            ))}
          </div>

          {/* Reputation formula */}
          <div className="reveal col-span-2 md:col-span-2 -ml-px -mt-px border border-line bg-bg-1 p-5">
            <div className="font-mono text-[9px] uppercase tracking-[0.22em] text-fg-3">Reputation</div>
            <div className="mt-2 font-playfair text-[18px] tracking-[-0.015em] text-fg [font-variation-settings:'opsz'_144]">
              score(N) <span className="text-fg-2 italic font-light">= α·ṡ + β·ℓ</span>
            </div>
            <div className="mt-3 font-mono text-[10px] uppercase tracking-[0.22em] text-fg-3">α=0.6 · β=0.4 · L_max=5s</div>
          </div>

          {/* Gossip */}
          <div className="reveal reveal-d1 col-span-2 md:col-span-2 -ml-px -mt-px border border-line bg-bg-1 p-5">
            <div className="font-mono text-[9px] uppercase tracking-[0.22em] text-fg-3">Gossip</div>
            <div className="mt-2 font-playfair text-[18px] tracking-[-0.015em] text-fg [font-variation-settings:'opsz'_144]">
              600s <span className="text-fg-2 italic font-light">· broadcast root</span>
            </div>
            <div className="mt-3 font-mono text-[10px] uppercase tracking-[0.22em] text-fg-3">/pinaivu/reputation/1.0.0</div>
          </div>
        </div>
      </section>

      {/* PROBLEMS */}
      <section id="problem">
        <div className="grid grid-cols-2 gap-14 items-end px-(--content-pad) max-w-(--content-max) mx-auto mb-[60px] max-lg:grid-cols-1 max-lg:gap-4 max-[720px]:mb-7">
          <div>
            <SecLabel><b className="text-fg font-semibold">002</b> · The Failure Mode</SecLabel>
            <h2 className="sec-title reveal font-fraunces text-[32px] lg:text-[52px] leading-[1.05] tracking-[-0.03em] font-medium [font-variation-settings:'opsz'_144] overflow-visible [&_em]:italic [&_em]:font-light [&_em]:text-fg-2 [&_em]:font-playfair">Cloud AI bakes in <em>three consequences</em><br/>that aren&apos;t technical requirements.</h2>
          </div>
          <p className="reveal reveal-d1 text-[16px] text-fg-2 leading-[1.72] max-w-[500px] justify-self-end pb-2.5 max-lg:justify-self-start">For every turn (P, C, R), today&apos;s provider observes all three, sets price ρ unilaterally, and revokes access at will. None of this is forced by the maths &mdash; only by the architecture.</p>
        </div>
        <div className="grid grid-cols-3 gap-0 px-(--content-pad) max-w-(--content-max) mx-auto max-[1280px]:grid-cols-2 max-[720px]:grid-cols-1">
          <div data-card data-prob className="group reveal relative -ml-px -mt-px flex min-h-[380px] flex-col justify-between border border-line px-9 pb-9 pt-11 transition-[border-color,background] duration-400ms ease-out hover:border-line-h hover:bg-bg-2">
            <div>
              <div className="mb-6 flex items-center justify-between font-mono text-[10px] tracking-[0.25em] text-fg-3">
                <span>01 — Context exposure</span>
                <span className="rounded-full border border-line-2 px-2 py-0.5 text-[10px] tracking-[0.2em] text-fg animate-breathe-4">G1</span>
              </div>
              <div className="relative my-3 h-[140px] grid place-items-center">
                <div className="viz-choke">
                  <svg viewBox="0 0 200 140" preserveAspectRatio="xMidYMid meet"><line x1="30" y1="20" x2="100" y2="70"/><line x1="30" y1="70" x2="100" y2="70"/><line x1="30" y1="120" x2="100" y2="70"/><line x1="170" y1="20" x2="100" y2="70"/><line x1="170" y1="70" x2="100" y2="70"/><line x1="170" y1="120" x2="100" y2="70"/></svg>
                  <div className="node" style={{top:'14%',left:'15%'}}></div><div className="node" style={{top:'50%',left:'15%'}}></div><div className="node" style={{top:'86%',left:'15%'}}></div>
                  <div className="node" style={{top:'14%',left:'85%'}}></div><div className="node" style={{top:'50%',left:'85%'}}></div><div className="node" style={{top:'86%',left:'85%'}}></div>
                  <div className="node center"></div>
                </div>
              </div>
              <h3 className="mb-3 font-fraunces font-normal text-[26px] leading-[1.1] tracking-[-0.02em] [font-variation-settings:'opsz'_144]">Provider sees (P, C, R)</h3>
              <p className="text-[14px] text-fg-2 leading-[1.65]">Every prompt, every accumulated context, every response flows through one party. Pinaivu AI keeps the full session C encrypted under a client-held key K; the GPU node sees only the decrypted context window for the current turn.</p>
            </div>
            <Corner pos="tl"/><Corner pos="tr"/><Corner pos="bl"/><Corner pos="br"/>
          </div>
          <div data-card data-prob className="group reveal reveal-d1 relative -ml-px -mt-px flex min-h-[380px] flex-col justify-between border border-line px-9 pb-9 pt-11 transition-[border-color,background] duration-400ms ease-out hover:border-line-h hover:bg-bg-2">
            <div>
              <div className="mb-6 flex items-center justify-between font-mono text-[10px] tracking-[0.25em] text-fg-3">
                <span>02 — Chain dependence</span>
                <span className="rounded-full border border-line-2 px-2 py-0.5 text-[10px] tracking-[0.2em] text-fg animate-breathe-4">G3</span>
              </div>
              <div className="relative my-3 h-[140px] grid place-items-center"><div className="viz-lock"><div className="viz-lock-shape"></div></div></div>
              <h3 className="mb-3 font-fraunces font-normal text-[26px] leading-[1.1] tracking-[-0.02em] [font-variation-settings:'opsz'_144]">One token, one ecosystem</h3>
              <p className="text-[14px] text-fg-2 leading-[1.65]">Bittensor collapses without TAO. Every prior decentralised inference system grounds trust in a specific chain, token and validator set. Pinaivu AI&apos;s trust model is self-sufficient; any chain is an optional settlement adapter selected in a TOML file.</p>
            </div>
            <Corner pos="tl"/><Corner pos="tr"/><Corner pos="bl"/><Corner pos="br"/>
          </div>
          <div data-card data-prob className="group reveal reveal-d2 relative -ml-px -mt-px flex min-h-[380px] flex-col justify-between border border-line px-9 pb-9 pt-11 transition-[border-color,background] duration-400ms ease-out hover:border-line-h hover:bg-bg-2">
            <div>
              <div className="mb-6 flex items-center justify-between font-mono text-[10px] tracking-[0.25em] text-fg-3">
                <span>03 — Unverifiable work</span>
                <span className="rounded-full border border-line-2 px-2 py-0.5 text-[10px] tracking-[0.2em] text-fg animate-breathe-4">G2</span>
              </div>
              <div className="relative my-3 h-[140px] grid place-items-center"><div className="viz-down"><div className="bar"></div><div className="bar"></div><div className="bar"></div><div className="bar"></div><div className="bar"></div><div className="bar"></div><div className="bar"></div><div className="bar"></div></div></div>
              <h3 className="mb-3 font-fraunces font-normal text-[26px] leading-[1.1] tracking-[-0.02em] [font-variation-settings:'opsz'_144]">No receipt, no recourse</h3>
              <p className="text-[14px] text-fg-2 leading-[1.65]">Batch marketplaces (io.net, Akash) and routers (Fortytwo) can&apos;t prove node N ran job J at the claimed parameters. Pinaivu AI ships every response with a self-verifiable ProofOfInference — Ed25519-signed, offline checkable, binding on (model, tokens, Δ, H_in, H_out).</p>
            </div>
            <Corner pos="tl"/><Corner pos="tr"/><Corner pos="bl"/><Corner pos="br"/>
          </div>
        </div>
      </section>

      {/* FEATURES */}
      <section id="features">
        <div className="grid grid-cols-2 gap-14 items-end px-(--content-pad) max-w-(--content-max) mx-auto mb-[60px] max-lg:grid-cols-1 max-lg:gap-4 max-[720px]:mb-7">
          <div>
            <SecLabel><b className="text-fg font-semibold">003</b> · Six Layers</SecLabel>
            <h2 className="sec-title reveal font-fraunces text-[32px] lg:text-[52px] leading-[1.05] tracking-[-0.03em] font-medium [font-variation-settings:'opsz'_144] overflow-visible [&_em]:italic [&_em]:font-light [&_em]:text-fg-2 [&_em]:font-playfair">Every layer is <em>independently replaceable.</em></h2>
          </div>
          <p className="reveal reveal-d1 text-[16px] text-fg-2 leading-[1.72] max-w-[500px] justify-self-end pb-2.5 max-lg:justify-self-start">Layers interact only through trait interfaces. Layer 0 (Crypto) has no external deps. Every layer above it may use external infra, but none is required.</p>
        </div>
        <div className="grid grid-cols-3 gap-0 px-(--content-pad) max-w-(--content-max) mx-auto max-[1280px]:grid-cols-2 max-[720px]:grid-cols-1">
          {[
            { delay: '', layer: 'L - 06 · Application', icon: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 9h18M9 21V9"/></svg>, title: 'OpenAI-compatible surface', body: 'TypeScript SDK, drop-in HTTP API, Web UI. Change the base URL; keep your code. Streaming, sessions and proof retrieval are native.', foot: 'TS SDK · HTTP · Web UI' },
            { delay: 'reveal-d1', layer: 'L - 05 · Session', icon: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><rect x="4" y="10" width="16" height="10" rx="2"/><path d="M8 10V7a4 4 0 018 0v3"/></svg>, title: 'E2E encrypted memory', body: 'Full history C is AES-256-GCM encrypted under a client-held K. The GPU node decrypts only the active context window — never C, never K.', foot: 'AES-GCM · X25519 · Portable' },
            { delay: 'reveal-d2', layer: 'L - 04 · Reputation', icon: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><circle cx="6" cy="18" r="2"/><circle cx="18" cy="18" r="2"/><circle cx="6" cy="6" r="2"/><circle cx="18" cy="6" r="2"/><circle cx="12" cy="12" r="2"/><path d="M12 10V8M12 16v-2M10 12H8M16 12h-2"/></svg>, title: 'Merkle tree, gossiped', body: 'Every node keeps a Merkle tree of its signed proofs. The root is broadcast over libp2p gossipsub every 10 min. Chain anchoring is optional.', foot: 'SHA-256 · Gossipsub · O(log n)' },
            { delay: '', layer: 'L - 03 · Marketplace', icon: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><path d="M3 12l9-9 9 9-9 9z"/><path d="M8 12h8M12 8v8"/></svg>, title: '200ms sealed-bid auction', body: 'Client broadcasts request; nodes pass six cheap-to-expensive checks and submit a bid. Composite score (0.4×price + 0.3×latency + 0.3×rep) picks the winner.', foot: 'libp2p · Sealed-bid · First-price' },
            { delay: 'reveal-d1', layer: 'L - 02 · Settlement', icon: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><rect x="3" y="6" width="18" height="12" rx="2"/><circle cx="12" cy="12" r="2.2"/><path d="M6 10v4M18 10v4"/></svg>, title: 'Pluggable escrow', body: 'Five adapters: free, signed-receipt, off-chain channel, Sui, EVM. Pick in TOML; same binary. Payment channels amortise gas 50× over 100 requests.', foot: 'free · receipt · channel · sui · evm' },
            { delay: 'reveal-d2', layer: 'L - 01 · Storage', icon: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><ellipse cx="12" cy="5" rx="8" ry="2.5"/><path d="M4 5v14c0 1.4 3.6 2.5 8 2.5s8-1.1 8-2.5V5"/><path d="M4 12c0 1.4 3.6 2.5 8 2.5s8-1.1 8-2.5"/></svg>, title: 'Content-addressed, agnostic', body: 'Three-method interface: put/get/delete. Local, IPFS, Walrus, Memory — same protocol. SHA-256 IDs mean put(b)=put(b) deduplicates for free.', foot: 'local · ipfs · walrus' },
          ].map(({ delay, layer, icon, title, body, foot }) => (
            <div key={title} data-card className={`group reveal ${delay} relative -ml-px -mt-px flex min-h-[320px] flex-col justify-between overflow-hidden border border-line px-9 pt-10 pb-9 transition-[border-color,background] duration-400ms ease-out hover:border-line-h hover:bg-bg-2`}>
              <div>
                <div className="mb-[30px] flex items-start justify-between">
                  <div className="font-mono text-[10px] tracking-[0.25em] text-fg-3">{layer}</div>
                  <div className="grid size-11 place-items-center border border-line-2 animate-breathe-5 transition-all duration-400ms ease-out will-change-[transform,opacity] group-hover:border-fg group-hover:bg-inv">
                    {isValidElement(icon)
                      ? cloneElement(icon as React.ReactElement<React.SVGProps<SVGSVGElement>>, {
                          className:
                            "size-5 text-fg transition-transform duration-500 ease-out group-hover:rotate-[8deg] group-hover:scale-110 group-hover:text-inv-fg",
                        })
                      : icon}
                  </div>
                </div>
                <h3 className="mb-2.5 font-playfair font-normal text-[24px] leading-[1.15] tracking-[-0.02em] [font-variation-settings:'opsz'_144]">{title}</h3>
                <p className="mb-5 text-[14px] text-fg-2 leading-[1.65]">{body}</p>
              </div>
              <div className="flex items-center justify-between border-t border-dashed border-line-2 pt-[18px] font-mono text-[9px] uppercase tracking-[0.2em] text-fg-3">
                {foot}
                <span className="-translate-x-1.5 text-fg opacity-0 transition-all duration-400ms ease-out group-hover:translate-x-0 group-hover:opacity-100">→</span>
              </div>
              <Corner pos="tl"/><Corner pos="tr"/><Corner pos="bl"/><Corner pos="br"/>
            </div>
          ))}
        </div>
      </section>

      {/* FLOW */}
      <section className="flow py-[72px] px-(--content-pad) max-w-(--content-max) mx-auto" id="flow">
        <div className="grid grid-cols-2 gap-14 items-end px-(--content-pad) max-w-(--content-max) mx-auto mb-[60px] max-lg:grid-cols-1 max-lg:gap-4 max-[720px]:mb-7">
          <div>
            <SecLabel><b className="text-fg font-semibold">004</b> · Request Flow</SecLabel>
            <h2 className="sec-title reveal font-fraunces text-[32px] lg:text-[52px] leading-[1.05] tracking-[-0.03em] font-medium [font-variation-settings:'opsz'_144] overflow-visible [&_em]:italic [&_em]:font-light [&_em]:text-fg-2 [&_em]:font-playfair">From prompt to proof, <em>in under a second.</em></h2>
          </div>
          <p className="reveal reveal-d1 text-[16px] text-fg-2 leading-[1.72] max-w-[500px] justify-self-end pb-2.5 max-lg:justify-self-start">Four stages. Each one cryptographically verifiable — from the sealed-bid auction through Ed25519-signed proof delivery.</p>
        </div>
        <div className="relative min-h-[560px] overflow-hidden border border-line bg-bg-1 px-12 py-[88px] before:absolute before:inset-0 before:pointer-events-none before:bg-[linear-gradient(var(--fg-6)_1px,transparent_1px),linear-gradient(90deg,var(--fg-6)_1px,transparent_1px)] before:[background-size:40px_40px] before:opacity-40 max-[720px]:px-[18px] max-[720px]:py-11">
          <div className="relative z-1 mx-auto grid max-w-[1100px] grid-cols-4 gap-0 max-xl:grid-cols-2 max-xl:gap-y-8 max-[720px]:grid-cols-1 max-[720px]:gap-0" id="flowStage">
            {[
              { n: 1, step: 'Step 01 · ~5ms', title: 'Broadcast', body: "Client broadcasts an InferenceRequest on the gossipsub topic for the required model, carrying model ID, budget, and privacy level — not the context (that stays client-side until a winner is chosen).", svg: <><path d="M12 2v14"/><path d="M6 12l6 6 6-6"/><rect x="4" y="18" width="16" height="4"/></> },
              { n: 2, step: 'Step 02 · 200ms', title: 'Sealed-bid Auction', body: 'GPU nodes pass six checks (model, capacity, queue, budget, privacy, throttle) and submit bids. Client picks winner by composite score: 0.4×price + 0.3×latency + 0.3×reputation.', svg: <><circle cx="12" cy="12" r="3"/><path d="M12 2v4M12 18v4M2 12h4M18 12h4"/><circle cx="6" cy="6" r="2"/><circle cx="18" cy="6" r="2"/><circle cx="6" cy="18" r="2"/><circle cx="18" cy="18" r="2"/></> },
              { n: 3, step: 'Step 03 · ~620ms', title: 'Inference', body: "Client encrypts the context window W for the winning node via X25519 DH and sends it directly to that node's API. Node decrypts W in RAM, runs inference, streams tokens back, then zeroes W.", svg: <><rect x="3" y="4" width="18" height="12" rx="1"/><path d="M8 20h8M12 16v4"/><line x1="7" y1="8" x2="7" y2="12"/><line x1="11" y1="8" x2="11" y2="12"/><line x1="15" y1="8" x2="15" y2="12"/></> },
              { n: 4, step: 'Step 04 · ~20ms', title: 'Proof + Settle', body: 'Node signs ProofOfInference π binding (model, tokens, Δ, H_in, H_out) with Ed25519. π is appended to the node\'s Merkle tree. Settlement adapter executes and ships π to the client.', svg: <><path d="M5 12l5 5 9-9"/><circle cx="12" cy="12" r="10"/></> },
            ].map(({ n, step, title, body, svg }) => (
              <div key={n} className="flow-step group relative px-5 py-7 text-center">
                <div className="relative mx-auto mb-[22px] grid size-24 place-items-center border border-line-h bg-bg transition-all duration-500 ease-out before:absolute before:inset-[-6px] before:border before:border-fg-4 before:opacity-0 before:transition-opacity before:duration-500 group-hover:border-inv group-hover:bg-inv group-hover:text-inv-fg group-hover:before:opacity-100 group-hover:before:inset-[-10px]">
                  <span className="absolute -top-2.5 -right-2.5 z-2 grid size-7 place-items-center rounded-full bg-inv font-mono text-[10px] font-bold text-inv-fg">{n}</span>
                  <svg className="size-[34px] transition-transform duration-500 ease-out group-hover:scale-110" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">{svg}</svg>
                </div>
                <div className="mb-2 font-mono text-[10px] uppercase tracking-[0.25em] text-fg-3">{step}</div>
                <h4 className="mb-2 font-fraunces font-medium text-[18px] [font-variation-settings:'opsz'_144]">{title}</h4>
                <p className="text-[13px] text-fg-2 leading-[1.55]">{body}</p>
              </div>
            ))}
          </div>
          <div className="mt-12 grid grid-cols-3 border-t border-line pt-8">
            <div className="px-6 border-r border-line-2 last:border-r-0">
              <div className="mb-1 font-mono text-[19px] font-medium"><span data-count="845">0</span>ms</div>
              <div className="font-mono text-[9.5px] tracking-[0.2em] uppercase text-fg-3">Total · end to end</div>
            </div>
            <div className="px-6 border-r border-line-2 last:border-r-0">
              <div className="mb-1 font-mono text-[19px] font-medium">42<span className="text-fg-3">tok/s</span></div>
              <div className="font-mono text-[9.5px] tracking-[0.2em] uppercase text-fg-3">Throughput · 70B model</div>
            </div>
            <div className="px-6 border-r border-line-2 last:border-r-0">
              <div className="mb-1 font-mono text-[19px] font-medium">0.0003<span className="text-fg-3"> PEER</span></div>
              <div className="font-mono text-[9.5px] tracking-[0.2em] uppercase text-fg-3">Cost · 256 tokens</div>
            </div>
          </div>
        </div>
      </section>

      {/* MODELS */}
      <section id="models">
        <div className="grid grid-cols-2 gap-14 items-end px-(--content-pad) max-w-(--content-max) mx-auto mb-[60px] max-lg:grid-cols-1 max-lg:gap-4 max-[720px]:mb-7">
          <div>
            <SecLabel><b className="text-fg font-semibold">005</b> · Model Catalog</SecLabel>
            <h2 className="sec-title reveal font-fraunces text-[32px] lg:text-[52px] leading-[1.05] tracking-[-0.03em] font-medium [font-variation-settings:'opsz'_144] overflow-visible [&_em]:italic [&_em]:font-light [&_em]:text-fg-2 [&_em]:font-playfair">Run the models you want. <span className='font-playfair text-fg-2 italic'>Not the ones they allow.</span></h2>
          </div>
          <p className="reveal reveal-d1 text-[16px] text-fg-2 leading-[1.72] max-w-[500px] justify-self-end pb-2.5 max-lg:justify-self-start">Every open-weight checkpoint that fits in VRAM. Pre-cached for the popular ones, on-demand for the rest.</p>
        </div>
        <div className="px-(--content-pad) max-w-(--content-max) mx-auto">
          <div className="flex items-end justify-between mb-7 gap-6 flex-wrap">
            <div className="flex rounded-full border border-line-2 bg-bg-1 p-1" id="modelTabs">
              <button className="tab active rounded-full px-[18px] py-[9px] font-mono text-[10px] font-medium uppercase tracking-[0.18em] text-fg-3 transition-colors duration-300 hover:bg-fg-5 hover:text-fg [&.active]:bg-inv [&.active]:text-inv-fg" data-tab="llm">Language</button>
              <button className="tab rounded-full px-[18px] py-[9px] font-mono text-[10px] font-medium uppercase tracking-[0.18em] text-fg-3 transition-colors duration-300 hover:bg-fg-5 hover:text-fg [&.active]:bg-inv [&.active]:text-inv-fg" data-tab="vision">Vision</button>
              <button className="tab rounded-full px-[18px] py-[9px] font-mono text-[10px] font-medium uppercase tracking-[0.18em] text-fg-3 transition-colors duration-300 hover:bg-fg-5 hover:text-fg [&.active]:bg-inv [&.active]:text-inv-fg" data-tab="audio">Audio</button>
            </div>
            <div className="font-mono text-[10px] tracking-[0.22em] uppercase text-fg-3">
              <span className="text-fg">84</span> models live · <span className="text-fg">2,847</span> variants
            </div>
          </div>
          <div className="models-box grid grid-cols-[1.1fr_1fr] min-h-[460px] border border-line bg-bg-1 max-lg:grid-cols-1">

            <div className="model-panel active" id="panel-llm">
              <div className="models-pane p-12 border-r border-line flex flex-col justify-between relative overflow-hidden max-lg:border-r-0 max-lg:border-b max-lg:border-b-(--line)">
                <div>
                  <div className="models-meta font-mono text-[9px] tracking-[0.25em] uppercase text-fg-3 flex gap-3 mb-4"><span className="px-2 py-0.5 border border-line-2 rounded-full text-fg">LLM</span><span className="px-2 py-0.5 border border-line-2 rounded-full text-fg">Text</span><span className="px-2 py-0.5 border border-line-2 rounded-full text-fg">FP16 · INT8 · INT4</span></div>
                  <h3 className="font-fraunces font-normal text-[38px] leading-none tracking-[-0.025em] mb-3 [font-variation-settings:'opsz'_144]">Llama 3.1 · 405B</h3>
                  <div className="font-mono text-[12px] text-fg-2 mb-6">Meta · Open weights · Released Jul 2024</div>
                  <p className="text-[15px] text-fg-2 leading-[1.6] mb-7 max-w-[440px]">The largest open LLM running on the network. Sharded across 16 consumer GPUs via tensor parallel. Competitive with GPT-4 on most benchmarks at a fraction of the cost.</p>
                </div>
                <div className="models-spec grid grid-cols-2 gap-px bg-line border border-line">
                  <div className="bg-bg-1 p-4 flex flex-col gap-1"><div className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3">Parameters</div><div className="font-mono text-[15px] text-fg font-medium">405<span className="text-fg-2 font-normal">B</span></div></div>
                  <div className="bg-bg-1 p-4 flex flex-col gap-1"><div className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3">Context</div><div className="font-mono text-[15px] text-fg font-medium">128<span className="text-fg-2 font-normal">K tokens</span></div></div>
                  <div className="bg-bg-1 p-4 flex flex-col gap-1"><div className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3">Throughput</div><div className="font-mono text-[15px] text-fg font-medium">42<span className="text-fg-2 font-normal"> tok/s</span></div></div>
                  <div className="bg-bg-1 p-4 flex flex-col gap-1"><div className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3">Cost / 1K</div><div className="font-mono text-[15px] text-fg font-medium">$0.003</div></div>
                </div>
              </div>
              <div className="terminal bg-black font-mono text-[14px] leading-[1.75] flex flex-col">
                <div className="flex items-center gap-1.5 border-b border-line px-[18px] py-3.5">
                  <div className="size-2 rounded-full border border-fg-3"></div><div className="size-2 rounded-full border border-fg-3"></div><div className="size-2 rounded-full border border-fg-3"></div>
                  <div className="ml-auto font-mono text-[9px] tracking-[0.25em] uppercase text-fg-3">peer-cli · llama-3.1-405b</div>
                </div>
                <div className="flex-1 overflow-y-auto p-[22px_20px]" data-term="llm"></div>
              </div>
            </div>

            <div className="model-panel" id="panel-vision">
              <div className="models-pane p-12 border-r border-line flex flex-col justify-between relative overflow-hidden max-lg:border-r-0 max-lg:border-b max-lg:border-b-(--line)">
                <div>
                  <div className="models-meta font-mono text-[9px] tracking-[0.25em] uppercase text-fg-3 flex gap-3 mb-4"><span className="px-2 py-0.5 border border-line-2 rounded-full text-fg">Vision</span><span className="px-2 py-0.5 border border-line-2 rounded-full text-fg">Diffusion</span><span className="px-2 py-0.5 border border-line-2 rounded-full text-fg">1024²</span></div>
                  <h3 className="font-fraunces font-normal text-[38px] leading-none tracking-[-0.025em] mb-3 [font-variation-settings:'opsz'_144]">FLUX.1 · Pro</h3>
                  <div className="font-mono text-[12px] text-fg-2 mb-6">Black Forest Labs · Open weights · Aug 2024</div>
                  <p className="text-[15px] text-fg-2 leading-[1.6] mb-7 max-w-[440px]">State-of-the-art text-to-image at 1024² native resolution. Runs on a single consumer GPU. 4-step Turbo variant generates in under 1 second per image.</p>
                </div>
                <div className="models-spec grid grid-cols-2 gap-px bg-line border border-line">
                  <div className="bg-bg-1 p-4 flex flex-col gap-1"><div className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3">Resolution</div><div className="font-mono text-[15px] text-fg font-medium">1024<span className="text-fg-2 font-normal">×1024</span></div></div>
                  <div className="bg-bg-1 p-4 flex flex-col gap-1"><div className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3">Steps</div><div className="font-mono text-[15px] text-fg font-medium">4<span className="text-fg-2 font-normal"> (turbo)</span></div></div>
                  <div className="bg-bg-1 p-4 flex flex-col gap-1"><div className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3">Latency</div><div className="font-mono text-[15px] text-fg font-medium">2.1<span className="text-fg-2 font-normal">s</span></div></div>
                  <div className="bg-bg-1 p-4 flex flex-col gap-1"><div className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3">Cost / img</div><div className="font-mono text-[15px] text-fg font-medium">$0.004</div></div>
                </div>
              </div>
              <div className="terminal bg-black font-mono text-[14px] leading-[1.75] flex flex-col">
                <div className="flex items-center gap-1.5 border-b border-line px-[18px] py-3.5">
                  <div className="size-2 rounded-full border border-fg-3"></div><div className="size-2 rounded-full border border-fg-3"></div><div className="size-2 rounded-full border border-fg-3"></div>
                  <div className="ml-auto font-mono text-[9px] tracking-[0.25em] uppercase text-fg-3">peer-cli · flux-1-pro</div>
                </div>
                <div className="flex-1 overflow-y-auto p-[22px_20px]" data-term="vision"></div>
              </div>
            </div>

            <div className="model-panel" id="panel-audio">
              <div className="models-pane p-12 border-r border-line flex flex-col justify-between relative overflow-hidden max-lg:border-r-0 max-lg:border-b max-lg:border-b-(--line)">
                <div>
                  <div className="models-meta font-mono text-[9px] tracking-[0.25em] uppercase text-fg-3 flex gap-3 mb-4"><span className="px-2 py-0.5 border border-line-2 rounded-full text-fg">Audio</span><span className="px-2 py-0.5 border border-line-2 rounded-full text-fg">STT</span><span className="px-2 py-0.5 border border-line-2 rounded-full text-fg">Streaming</span></div>
                  <h3 className="font-fraunces font-normal text-[38px] leading-none tracking-[-0.025em] mb-3 [font-variation-settings:'opsz'_144]">Whisper · Large v3</h3>
                  <div className="font-mono text-[12px] text-fg-2 mb-6">OpenAI · Open weights · MIT license</div>
                  <p className="text-[15px] text-fg-2 leading-[1.6] mb-7 max-w-[440px]">99-language speech-to-text with automatic language detection. Runs 52× realtime on an RTX 3090. Native WebSocket streaming for voice applications.</p>
                </div>
                <div className="models-spec grid grid-cols-2 gap-px bg-line border border-line">
                  <div className="bg-bg-1 p-4 flex flex-col gap-1"><div className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3">Languages</div><div className="font-mono text-[15px] text-fg font-medium">99</div></div>
                  <div className="bg-bg-1 p-4 flex flex-col gap-1"><div className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3">Speed</div><div className="font-mono text-[15px] text-fg font-medium">52×<span className="text-fg-2 font-normal"> realtime</span></div></div>
                  <div className="bg-bg-1 p-4 flex flex-col gap-1"><div className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3">TTFT</div><div className="font-mono text-[15px] text-fg font-medium">&lt;300<span className="text-fg-2 font-normal">ms</span></div></div>
                  <div className="bg-bg-1 p-4 flex flex-col gap-1"><div className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3">Cost / min</div><div className="font-mono text-[15px] text-fg font-medium">$0.001</div></div>
                </div>
              </div>
              <div className="terminal bg-black font-mono text-[14px] leading-[1.75] flex flex-col">
                <div className="flex items-center gap-1.5 border-b border-line px-[18px] py-3.5">
                  <div className="size-2 rounded-full border border-fg-3"></div><div className="size-2 rounded-full border border-fg-3"></div><div className="size-2 rounded-full border border-fg-3"></div>
                  <div className="ml-auto font-mono text-[9px] tracking-[0.25em] uppercase text-fg-3">peer-cli · whisper-v3-large</div>
                </div>
                <div className="flex-1 overflow-y-auto p-[22px_20px]" data-term="audio"></div>
              </div>
            </div>

          </div>
        </div>
      </section>

      {/* COMPARE */}
      <section id="compare">
        <div className="grid grid-cols-2 gap-14 items-end px-(--content-pad) max-w-(--content-max) mx-auto mb-[60px] max-lg:grid-cols-1 max-lg:gap-4 max-[720px]:mb-7">
          <div>
            <SecLabel><b className="text-fg font-semibold">006</b> · Comparison</SecLabel>
            <h2 className="sec-title reveal font-fraunces text-[32px] lg:text-[52px] leading-[1.05] tracking-[-0.03em] font-medium [font-variation-settings:'opsz'_144] overflow-visible [&_em]:italic [&_em]:font-light [&_em]:text-fg-2 [&_em]:font-playfair">Against the <em>incumbents.</em></h2>
          </div>
          <p className="reveal reveal-d1 text-[16px] text-fg-2 leading-[1.72] max-w-[500px] justify-self-end pb-2.5 max-lg:justify-self-start">Every prior system either lacks G2 (no verifiable accountability) or sacrifices G3/G4 (hard-coded chain and storage). Pinaivu AI is the first to satisfy all five guarantees simultaneously.</p>
        </div>
        <div className="px-(--content-pad) max-w-(--content-max) mx-auto">
          <div className="compare-box border border-line bg-bg-1 overflow-hidden reveal">
            <div className="compare-row head grid grid-cols-[1.4fr_1fr_1fr_1fr_1fr_1fr] border-b border-line transition-colors duration-200">
              <div className="compare-cell px-6 py-5 border-r border-line text-[14px] text-fg-2 flex items-center gap-2">Property</div>
              <div className="compare-cell px-6 py-5 border-r border-line text-[14px] text-fg-2 flex items-center gap-2">Pinaivu AI</div>
              <div className="compare-cell px-6 py-5 border-r border-line text-[14px] text-fg-2 flex items-center gap-2">Bittensor</div>
              <div className="compare-cell px-6 py-5 border-r border-line text-[14px] text-fg-2 flex items-center gap-2">QVAC</div>
              <div className="compare-cell px-6 py-5 border-r border-line text-[14px] text-fg-2 flex items-center gap-2">io.net</div>
              <div className="compare-cell px-6 py-5 border-r border-line text-[14px] text-fg-2 flex items-center gap-2">Fortytwo</div>
            </div>
            <div className="compare-row grid grid-cols-[1.4fr_1fr_1fr_1fr_1fr_1fr] border-b border-line transition-colors duration-200">
              <div className="compare-cell px-6 py-5 border-r border-line text-[14px] text-fg-2 flex items-center gap-2"><span className="text-fg font-medium text-[15px]">G1 — Session privacy</span></div>
              <div className="compare-cell yes px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12l5 5 9-9"/></svg> AES-256-GCM</div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> Validators see all</div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> Not addressed</div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3">N/A · batch only</div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> Centralised</div>
            </div>
            <div className="compare-row grid grid-cols-[1.4fr_1fr_1fr_1fr_1fr_1fr] border-b border-line transition-colors duration-200">
              <div className="compare-cell px-6 py-5 border-r border-line text-[14px] text-fg-2 flex items-center gap-2"><span className="text-fg font-medium text-[15px]">G2 — Node accountability</span></div>
              <div className="compare-cell yes px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12l5 5 9-9"/></svg> Ed25519 + Merkle</div>
              <div className="compare-cell px-6 py-5 border-r border-line text-[14px] text-fg-2 flex items-center gap-2">Partial · validators</div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> No receipts</div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> No receipts</div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> No receipts</div>
            </div>
            <div className="compare-row grid grid-cols-[1.4fr_1fr_1fr_1fr_1fr_1fr] border-b border-line transition-colors duration-200">
              <div className="compare-cell px-6 py-5 border-r border-line text-[14px] text-fg-2 flex items-center gap-2"><span className="text-fg font-medium text-[15px]">G3 — Settlement neutrality</span></div>
              <div className="compare-cell yes px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12l5 5 9-9"/></svg> 5 adapters</div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> TAO only</div>
              <div className="compare-cell px-6 py-5 border-r border-line text-[14px] text-fg-2 flex items-center gap-2">No payment</div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> IO token</div>
              <div className="compare-cell px-6 py-5 border-r border-line text-[14px] text-fg-2 flex items-center gap-2">N/A · centralised</div>
            </div>
            <div className="compare-row grid grid-cols-[1.4fr_1fr_1fr_1fr_1fr_1fr] border-b border-line transition-colors duration-200">
              <div className="compare-cell px-6 py-5 border-r border-line text-[14px] text-fg-2 flex items-center gap-2"><span className="text-fg font-medium text-[15px]">G5 — Permissionless</span></div>
              <div className="compare-cell yes px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12l5 5 9-9"/></svg> PeerId = pk_N</div>
              <div className="compare-cell yes px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12l5 5 9-9"/></svg></div>
              <div className="compare-cell yes px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12l5 5 9-9"/></svg></div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> KYC required</div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> Centralised</div>
            </div>
            <div className="compare-row grid grid-cols-[1.4fr_1fr_1fr_1fr_1fr_1fr] border-b border-line transition-colors duration-200">
              <div className="compare-cell px-6 py-5 border-r border-line text-[14px] text-fg-2 flex items-center gap-2"><span className="text-fg font-medium text-[15px]">Persistent sessions</span></div>
              <div className="compare-cell yes px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12l5 5 9-9"/></svg> E2E encrypted</div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg></div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg></div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg></div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg></div>
            </div>
            <div className="compare-row grid grid-cols-[1.4fr_1fr_1fr_1fr_1fr_1fr] border-b border-line transition-colors duration-200">
              <div className="compare-cell px-6 py-5 border-r border-line text-[14px] text-fg-2 flex items-center gap-2"><span className="text-fg font-medium text-[15px]">Streaming responses</span></div>
              <div className="compare-cell yes px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12l5 5 9-9"/></svg> Native WebSocket</div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg></div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg></div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg></div>
              <div className="compare-cell no px-6 py-5 border-r border-line text-[14px] flex items-center gap-2 text-fg-3"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg></div>
            </div>
          </div>
        </div>
      </section>

      {/* TECH */}
      <section id="tech">
        <div className="grid grid-cols-2 gap-14 items-end px-(--content-pad) max-w-(--content-max) mx-auto mb-[60px] max-lg:grid-cols-1 max-lg:gap-4 max-[720px]:mb-7">
          <div>
            <SecLabel><b className="text-fg font-semibold">007</b> · Stack</SecLabel>
            <h2 className="sec-title reveal font-fraunces text-[32px] lg:text-[52px] leading-[1.05] tracking-[-0.03em] font-medium [font-variation-settings:'opsz'_144] overflow-visible [&_em]:italic [&_em]:font-light [&_em]:text-fg-2 [&_em]:font-playfair">Built on <em>proven primitives.</em></h2>
          </div>
          <p className="reveal reveal-d1 text-[16px] text-fg-2 leading-[1.72] max-w-[500px] justify-self-end pb-2.5 max-lg:justify-self-start">No reinvention for its own sake. Every layer is a battle-tested open-source component, assembled specifically for GPU compute coordination.</p>
        </div>
        <div className="grid grid-cols-3 gap-0 px-(--content-pad) max-w-(--content-max) mx-auto max-[1280px]:grid-cols-2 max-[720px]:grid-cols-1">
          {([
            { delay: '', id: 'T · 01', title: 'libp2p Transport', body: 'TCP + QUIC dual-stack with Noise authenticated encryption and Yamux stream multiplexing. AutoNAT traversal means any home node can participate without port-forwarding.', tags: ['TCP','QUIC','Noise','Yamux'] },
            { delay: 'reveal-d1', id: 'T · 02', title: 'Kademlia DHT + Gossipsub', body: 'Kademlia DHT for peer routing and mDNS for local discovery. Five gossipsub topics carry inference requests, bids, announcements and Merkle root broadcasts.', tags: ['Kademlia','mDNS','Gossipsub','5 topics'] },
            { delay: 'reveal-d2', id: 'T · 03', title: 'Ed25519 Identity', body: 'Every node is an Ed25519 keypair. The libp2p PeerId is derived from pk_N — no separate account or wallet needed. 128-bit security per RFC 8032.', tags: ['Ed25519','RFC 8032','128-bit security'] },
            { delay: '', id: 'T · 04', title: 'ProofOfInference', body: "A signed execution receipt bound to (model, tokens, latency, H_in, H_out). Verifiable offline with only the node's public key. Constant-time O(1) verification, no network call.", tags: ['Ed25519 σ','SHA-256 H_in/H_out','Offline'] },
            { delay: 'reveal-d1', id: 'T · 05', title: 'AES-256-GCM Sessions', body: 'Session context encrypted under a client-held key K derived from X25519 DH. The GPU node never sees K — only the current-turn context window, zeroed from RAM after inference.', tags: ['AES-256-GCM','X25519','96-bit nonce'] },
            { delay: 'reveal-d2', id: 'T · 06', title: 'Settlement Adapters', body: 'Five adapters behind one interface: free, signed-receipt, off-chain payment channel, Sui (Phase D), EVM (Phase E). All selected by a single TOML key — same binary, zero code changes.', tags: ['free','receipt','channel','sui','evm'] },
          ] as const).map(({ delay, id, title, body, tags }) => (
            <div key={id} data-card className={`group reveal ${delay} relative -ml-px -mt-px min-h-[260px] overflow-hidden border border-line px-9 pt-10 pb-9 transition-all duration-400ms ease-out after:absolute after:top-0 after:left-0 after:right-0 after:h-0.5 after:bg-fg after:scale-x-0 after:origin-left after:transition-transform after:duration-500 after:ease-out hover:border-line-h hover:bg-bg-2 hover:after:scale-x-100`}>
              <div className="mb-5 flex items-start justify-between gap-4">
                <h4 className="font-playfair font-medium text-[20px] tracking-[-0.015em] [font-variation-settings:'opsz'_144]">{title}</h4>
                <div className="font-mono text-[9px] tracking-[0.25em] text-fg-3 whitespace-nowrap">{id}</div>
              </div>
              <p className="mb-[18px] text-[13px] text-fg-2 leading-[1.65]">{body}</p>
              <div className="flex flex-wrap gap-1.5">
                {tags.map(t => (
                  <span key={t} className="border border-line-2 px-2 py-1 font-mono text-[9px] uppercase tracking-[0.18em] text-fg-2 transition-[border-color] duration-300 group-hover:border-fg-3">{t}</span>
                ))}
              </div>
              <Corner pos="tl"/><Corner pos="tr"/><Corner pos="bl"/><Corner pos="br"/>
            </div>
          ))}
        </div>
      </section>

      {/* HARDWARE */}
      <section id="hardware">
        <div className="grid grid-cols-2 gap-14 items-end px-(--content-pad) max-w-(--content-max) mx-auto mb-[60px] max-lg:grid-cols-1 max-lg:gap-4 max-[720px]:mb-7">
          <div>
            <SecLabel><b className="text-fg font-semibold">008</b> · Fleet</SecLabel>
            <h2 className="sec-title reveal font-fraunces text-[32px] lg:text-[52px] leading-[1.05] tracking-[-0.03em] font-medium [font-variation-settings:'opsz'_144] overflow-visible [&_em]:italic [&_em]:font-light [&_em]:text-fg-2 [&_em]:font-playfair">The GPUs <em>behind the mesh.</em></h2>
          </div>
          <p className="reveal reveal-d1 text-[16px] text-fg-2 leading-[1.72] max-w-[500px] justify-self-end pb-2.5 max-lg:justify-self-start">A live breakdown of the hardware running inference right now. Consumer cards dominate the network — by design.</p>
        </div>
        <div className="grid grid-cols-4 gap-0 px-(--content-pad) max-w-(--content-max) mx-auto max-[1024px]:grid-cols-2 max-[720px]:grid-cols-1" id="hwGrid">
          {([
            { delay: '', pct: '68%', label: 'RTX 4090', spec: '24GB · 82.6 TFLOPS', share: '68%' },
            { delay: 'reveal-d1', pct: '18%', label: 'RTX 3090', spec: '24GB · 35.6 TFLOPS', share: '18%' },
            { delay: 'reveal-d2', pct: '9%', label: 'A100 · 80GB', spec: '80GB HBM2e · 312 TFLOPS', share: '9%' },
            { delay: 'reveal-d3', pct: '5%', label: 'Other', spec: '4080 · 4070 · M-series · more', share: '5%' },
          ] as const).map(({ delay, pct, label, spec, share }) => (
            <div key={label} data-card className={`group reveal ${delay} relative -ml-px -mt-px flex min-h-[320px] flex-col overflow-hidden border border-line px-8 pt-10 pb-8 transition-all duration-400ms ease-out hover:border-line-h hover:bg-bg-2`} style={{'--pct': pct} as React.CSSProperties}>
              <div className="relative mb-6 grid h-[100px] place-items-center border border-dashed border-line transition-[border-color] duration-400ms group-hover:border-fg-3">
                <div className="relative grid size-14 place-items-center border border-fg transition-transform duration-[600ms] ease-out group-hover:rotate-45 group-hover:scale-110
                  before:absolute before:-top-2 before:bottom-[-8px] before:left-1/2 before:w-px before:bg-fg before:-translate-x-1/2
                  after:absolute after:-left-2 after:right-[-8px] after:top-1/2 after:h-px after:bg-fg after:-translate-y-1/2">
                  <span className="size-2 rounded-sm bg-fg"></span>
                </div>
              </div>
              <div className="mb-1.5 font-fraunces font-medium text-[20px] tracking-[-0.015em] [font-variation-settings:'opsz'_144]">{label}</div>
              <div className="mb-5 font-mono text-[10px] tracking-[0.05em] text-fg-2">{spec}</div>
              <div className="mt-auto">
                <div className="mb-2 flex justify-between font-mono text-[9px] uppercase tracking-[0.2em] text-fg-3"><span>Network share</span><span>{share}</span></div>
                <div className="relative h-1 overflow-hidden bg-fg-5"><div className="hw-bar-fill"></div></div>
              </div>
              <Corner pos="tl"/><Corner pos="tr"/><Corner pos="bl"/><Corner pos="br"/>
            </div>
          ))}
        </div>
      </section>

      {/* ROADMAP */}
      <section id="roadmap">
        <div className="grid grid-cols-2 gap-14 items-end px-(--content-pad) max-w-(--content-max) mx-auto mb-[60px] max-lg:grid-cols-1 max-lg:gap-4 max-[720px]:mb-7">
          <div>
            <SecLabel><b className="text-fg font-semibold">009</b> · Timeline</SecLabel>
            <h2 className="sec-title reveal font-fraunces text-[32px] lg:text-[52px] leading-[1.05] tracking-[-0.03em] font-medium [font-variation-settings:'opsz'_144] overflow-visible [&_em]:italic [&_em]:font-light [&_em]:text-fg-2 [&_em]:font-playfair">From testnet <em>to full mesh.</em></h2>
          </div>
          <p className="reveal reveal-d1 text-[16px] text-fg-2 leading-[1.72] max-w-[500px] justify-self-end pb-2.5 max-lg:justify-self-start">Four phases. Shipping cadence tied to node-count milestones, not marketing dates.</p>
        </div>
        <div className="grid grid-cols-4 gap-0 px-(--content-pad) max-w-(--content-max) mx-auto max-[1024px]:grid-cols-2 max-[720px]:grid-cols-1">
          {([
            { delay: '', active: true, p: '1', tag: 'Live', date: 'Phase C · April 2026', title: 'Cryptographic Core', items: ['Ed25519 identity + ProofOfInference','Merkle reputation tree + gossip','Free + signed-receipt settlement','Local + IPFS + Walrus storage'] },
            { delay: 'reveal-d1', active: false, p: '0', tag: 'Queued', date: 'Phase D · H2 2026', title: 'Sui Settlement', items: ['Move escrow smart contract','SuiSettlement adapter live','On-chain proof verification','Reputation anchoring on Sui'] },
            { delay: 'reveal-d2', active: false, p: '0', tag: 'Queued', date: 'Phase E · H1 2027', title: 'EVM Settlement', items: ['Solidity escrow contract · Base L2','EvmSettlement adapter live','Multi-chain settlement matrix','TOML-selectable chains'] },
            { delay: 'reveal-d3', active: false, p: '0', tag: 'Queued', date: 'Phase F · H2 2027', title: 'On-Chain Channels', items: ['Payment channels — on-chain close','50× gas amortisation at 100 req/session','Full gossip protocol live','Governance parameterisation'] },
          ] as const).map(({ delay, active, p, tag, date, title, items }) => (
            <div key={title} data-card className={`group reveal ${delay} relative -ml-px -mt-px flex min-h-[360px] flex-col overflow-hidden border px-9 pt-10 pb-9 transition-all duration-400ms ease-out hover:border-line-h hover:bg-bg-2 ${active ? 'border-white bg-bg-2 animate-phase-glow' : 'border-line'}`} style={{'--p': p} as React.CSSProperties}>
              <span className={`mb-6 inline-flex items-center gap-1.5 font-mono text-[9px] uppercase tracking-[0.25em] ${active ? 'text-fg' : 'text-fg-3'}`}>
                {active && <span className="size-1.5 rounded-full bg-fg animate-pulse-dot"></span>}
                {tag}
              </span>
              <div className="mb-1.5 font-mono text-[9px] tracking-[0.25em] text-fg-3">{date}</div>
              <div className="mb-[22px] font-fraunces font-normal text-[25px] tracking-[-0.02em] [font-variation-settings:'opsz'_144]">{title}</div>
              <ul className="mt-auto flex flex-col gap-2.5 list-none">
                {items.map(item => (
                  <li key={item} className="relative pl-[18px] font-mono text-[12px] text-fg-2 leading-[1.5] before:absolute before:left-0 before:top-2 before:h-px before:w-2 before:bg-fg-3 before:transition-[width,background] before:duration-300 group-hover:before:w-3 group-hover:before:bg-fg">{item}</li>
                ))}
              </ul>
              <div className="absolute bottom-0 left-0 right-0 h-0.5 bg-fg-5 after:absolute after:inset-0 after:bg-fg after:origin-left after:transition-transform after:duration-[1400ms] after:ease-out after:delay-200" style={{'--p': p, transform: `scaleX(${p})`} as React.CSSProperties}></div>
              <Corner pos="tl"/><Corner pos="tr"/><Corner pos="bl"/><Corner pos="br"/>
            </div>
          ))}
        </div>
      </section>

      {/* FINAL CTA */}
      <div className="py-[120px] px-(--content-pad) max-w-(--content-max) mx-auto max-[720px]:py-[68px]" id="cta">
        <div className="relative overflow-hidden border border-line py-[108px] px-[68px] text-center max-[720px]:px-6 max-[720px]:py-[60px]">
          <div className="absolute inset-0 pointer-events-none opacity-50 bg-[linear-gradient(var(--fg-6)_1px,transparent_1px),linear-gradient(90deg,var(--fg-6)_1px,transparent_1px)] [background-size:50px_50px]"></div>
          <div className="absolute inset-0 pointer-events-none [background:radial-gradient(ellipse_at_50%_120%,var(--fg-5),transparent_60%)]"></div>
          <div className="relative mb-7 font-mono text-[10px] tracking-[0.35em] uppercase text-fg-3">— 010 · Start Here</div>
          <h2 className="relative mb-5 font-fraunces font-normal text-[38px] lg:text-[64px] leading-[0.95] tracking-[-0.035em] [font-variation-settings:'opsz'_144] [&_em]:font-light [&_em]:italic [&_em]:text-fg-2">Be first on the network.<br/><em>Join the waitlist.</em></h2>
          <p className="relative mx-auto mb-9 max-w-[500px] text-base leading-[1.6] text-fg-2">No credit card. No token. No permission. Phase C is live — Ed25519 identity, Merkle reputation and signed-receipt settlement work today, with zero blockchain required.</p>
          <div className="relative flex flex-wrap justify-center gap-2.5">
            <button className="relative inline-flex items-center gap-2.5 overflow-hidden isolate rounded-full bg-inv px-6 py-3.5 font-mono text-[11px] font-semibold uppercase tracking-[0.14em] text-inv-fg [transition:background_.25s,color_.25s,border-color_.25s]" onClick={() => setShowWaitlist(true)}>
              <span className="relative z-2">Join Waitlist</span>
              <span className="relative z-2">↗</span>
            </button>
            <a className="relative inline-flex items-center gap-2.5 overflow-hidden isolate rounded-full border border-line-2 bg-bg-2/60 px-6 py-3.5 font-mono text-[11px] font-semibold uppercase tracking-[0.14em] text-fg backdrop-blur-md [transition:background_.25s,color_.25s,border-color_.25s] hover:border-fg hover:bg-bg-2" href="/PinaivuAI_Whitepaper.pdf" target="_blank" rel="noopener noreferrer">
              <span>Read Whitepaper</span>
            </a>
          </div>
        </div>
      </div>

      {/* FOOTER */}
      <footer className="flex flex-wrap items-center justify-between gap-5 border-t border-line px-(--content-pad) py-12 max-w-(--content-max) mx-auto">
        <div className="flex items-center gap-2.5 font-mono text-[12px] font-medium tracking-[0.08em] uppercase">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
            <circle cx="12" cy="12" r="10"/><circle cx="12" cy="12" r="5"/>
            <circle cx="12" cy="12" r="1.5" fill="currentColor"/>
          </svg>
          Pinaivu AI
        </div>
        <div className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3">The Inference Network · Est. 2026 · Licensed MIT</div>
        <ul className="flex list-none gap-5">
          <li><a href="#" className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3 transition-colors duration-300 hover:text-fg">Docs</a></li>
          <li><a href="#" className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3 transition-colors duration-300 hover:text-fg">GitHub</a></li>
          <li><a href="#" className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3 transition-colors duration-300 hover:text-fg">Discord</a></li>
          <li><a href="#" className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3 transition-colors duration-300 hover:text-fg">Twitter</a></li>
          <li><a href="/PinaivuAI_Whitepaper.pdf" target="_blank" rel="noopener noreferrer" className="font-mono text-[9px] tracking-[0.22em] uppercase text-fg-3 transition-colors duration-300 hover:text-fg">Whitepaper</a></li>
        </ul>
      </footer>
    </div>
  );
}
