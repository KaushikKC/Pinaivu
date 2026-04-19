'use client';

import { useEffect, useState } from 'react';

const CornerSVG = () => (
  <svg viewBox="0 0 10 10"><path d="M0,10 L0,0 L10,0" stroke="#fff" fill="none"/></svg>
);

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
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal-box" onClick={e => e.stopPropagation()}>
        <button className="modal-close" onClick={onClose} aria-label="Close">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg>
        </button>
        <div className="modal-label">Join the waitlist</div>
        {status === 'success' ? (
          <div className="modal-success">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><path d="M5 12l5 5 9-9"/><circle cx="12" cy="12" r="10"/></svg>
            <h3>You&apos;re on the list.</h3>
            <p>We&apos;ll reach out when early access opens. Phase C is live — Ed25519 identity and signed-receipt settlement work today.</p>
          </div>
        ) : (
          <>
            <h3>Early access to Pinaivu AI</h3>
            <p>Be among the first to run a node or use the network. No token, no chain required.</p>
            <form onSubmit={submit} className="modal-form">
              <div className="modal-input-wrap">
                <input
                  type="email"
                  placeholder="your@email.com"
                  value={email}
                  onChange={e => setEmail(e.target.value)}
                  required
                  autoFocus
                  className="modal-input"
                />
                <button type="submit" className="modal-submit" disabled={status === 'loading'}>
                  {status === 'loading' ? <span className="modal-spinner"></span> : <><span>Request Access</span><span className="arrow"> ↗</span></>}
                </button>
              </div>
              {status === 'error' && <div className="modal-error">{msg}</div>}
            </form>
            <div className="modal-fine">No spam. Unsubscribe any time.</div>
          </>
        )}
      </div>
    </div>
  );
}

export default function Home() {
  const [showWaitlist, setShowWaitlist] = useState(false);
  useEffect(() => {
    /* ═══════════ 2. SCROLL PROGRESS ═══════════ */
    (function(){
      const bar = document.getElementById('progBar') as HTMLElement|null;
      const nav = document.getElementById('nav');
      if(!bar) return;
      function tick(){
        const h = document.documentElement;
        const scrolled = h.scrollTop / (h.scrollHeight - h.clientHeight);
        bar!.style.transform = `scaleX(${scrolled})`;
        if(h.scrollTop > 40) nav?.classList.add('scrolled'); else nav?.classList.remove('scrolled');
      }
      window.addEventListener('scroll', tick, { passive: true });
      tick();
    })();

    /* ═══════════ 3. REVEAL ON SCROLL ═══════════ */
    (function(){
      const targets = document.querySelectorAll('.reveal, .stat, .flow, .manifesto, .hw, #hwGrid');
      const io = new IntersectionObserver((ents)=>{
        ents.forEach(e=>{
          if(e.isIntersecting){
            e.target.classList.add('in-view');
            io.unobserve(e.target);
          }
        });
      }, { threshold: 0.12 });
      targets.forEach(t=>io.observe(t));
    })();

    /* ═══════════ 4. NUMBER COUNTERS ═══════════ */
    (function(){
      const nodes = document.querySelectorAll('[data-count]');
      const io = new IntersectionObserver((ents)=>{
        ents.forEach(e=>{
          if(!e.isIntersecting) return;
          const el = e.target as HTMLElement;
          const target = parseFloat((el as HTMLElement).dataset.count!);
          const isFloat = (el as HTMLElement).dataset.float === '1';
          const dur = 1800;
          const start = performance.now();
          function step(now: number){
            const p = Math.min((now - start)/dur, 1);
            const eased = 1 - Math.pow(1 - p, 3);
            const v = target * eased;
            el.textContent = isFloat ? v.toFixed(1) : Math.floor(v).toLocaleString();
            if(p < 1) requestAnimationFrame(step);
          }
          requestAnimationFrame(step);
          io.unobserve(el);
        });
      }, { threshold: 0.1, rootMargin: '0px 0px -10% 0px' });
      nodes.forEach(n=>io.observe(n));
    })();

    /* ═══════════ 5. MODEL TABS + TERMINAL TYPE-OUT ═══════════ */
    (function(){
      const tabs = document.querySelectorAll('#modelTabs .tab');
      const panels = document.querySelectorAll('.model-panel');
      const termBodies: Record<string, Array<{c:string,d:number}>> = {
        llm: [
          { c:'$ peer infer --model llama-3.1-405b \\', d:60 },
          { c:'    --prompt "The future of decentralized AI is" \\', d:40 },
          { c:'    --max-tokens 128 --stream', d:40 },
          { c:'', d:200 },
          { c:'<span class="k">[route]</span> <span class="o">Selected 16 nodes · eu-west × 8, us-east × 8</span>', d:300 },
          { c:'<span class="k">[shard]</span> <span class="o">Model split: layers 0-25 · 26-50 · 51-75 · 76-100</span>', d:260 },
          { c:'<span class="k">[shard]</span> <span class="o">Tensor parallel 16-way · NCCL-over-TCP</span>', d:260 },
          { c:'<span class="k">[infer]</span> <span class="o">First token: 87ms · Throughput: 42 tok/s</span>', d:300 },
          { c:'<span class="k">[stream]</span> <span class="p">› not about replacing centralised systems,</span>', d:400 },
          { c:'           <span class="p">but about giving every developer the same</span>', d:400 },
          { c:'           <span class="p">capabilities without permission or gatekeepers.</span>', d:500 },
          { c:'<span class="k">[proof]</span> <span class="o">Ed25519 σ verified · π valid · offline</span>', d:260 },
          { c:'<span class="k">[done]</span> <span class="s">128 tokens · 3.04s · 0.000384 PEER</span>', d:0 }
        ],
        vision: [
          { c:'$ peer generate --model flux-1-pro \\', d:60 },
          { c:'    --prompt "a quiet city at dawn, film grain" \\', d:40 },
          { c:'    --size 1024x1024 --steps 4 --turbo', d:40 },
          { c:'', d:200 },
          { c:'<span class="k">[route]</span> <span class="o">Selected node eu-west-a100-17 · RTX 4090</span>', d:260 },
          { c:'<span class="k">[diff]</span>  <span class="o">Step 1/4 · 0.5s</span>', d:300 },
          { c:'<span class="k">[diff]</span>  <span class="o">Step 2/4 · 1.0s</span>', d:300 },
          { c:'<span class="k">[diff]</span>  <span class="o">Step 3/4 · 1.5s</span>', d:300 },
          { c:'<span class="k">[diff]</span>  <span class="o">Step 4/4 · 2.0s</span>', d:300 },
          { c:'<span class="k">[out]</span>   <span class="o">1024×1024 PNG · 1.8 MB</span>', d:260 },
          { c:'<span class="k">[proof]</span>  <span class="o">Ed25519 σ verified · π valid · offline</span>', d:260 },
          { c:'<span class="k">[done]</span>  <span class="s">Total 2.1s · 0.004 PEER</span>', d:0 }
        ],
        audio: [
          { c:'$ peer transcribe --model whisper-v3-large \\', d:60 },
          { c:'    --input meeting_q2.mp3 --language auto \\', d:40 },
          { c:'    --format json --stream', d:40 },
          { c:'', d:200 },
          { c:'<span class="k">[route]</span>   <span class="o">Selected node ap-east-14 · RTX 3090</span>', d:260 },
          { c:'<span class="k">[detect]</span>  <span class="o">Language: English · 98.2% confidence</span>', d:260 },
          { c:'<span class="k">[stt]</span>     <span class="o">Processing 47:12 of audio…</span>', d:300 },
          { c:'<span class="k">[speed]</span>   <span class="o">52.3× realtime · 54.2s elapsed</span>', d:300 },
          { c:'<span class="k">[out]</span>     <span class="o">transcript.json · 12,847 words</span>', d:260 },
          { c:'<span class="k">[proof]</span>    <span class="o">Ed25519 σ verified · π valid · offline</span>', d:260 },
          { c:'<span class="k">[done]</span>    <span class="s">Total 54.2s · 0.047 PEER</span>', d:0 }
        ]
      };
      const typers: Record<string, ReturnType<typeof setTimeout>|null> = { llm:null, vision:null, audio:null };

      function runTerminal(kind: string){
        const body = document.querySelector(`[data-term="${kind}"]`) as HTMLElement;
        if(!body) return;
        if(typers[kind]) { clearTimeout(typers[kind]!); typers[kind] = null; }
        body.innerHTML = '';
        const lines = termBodies[kind];
        let i = 0;
        function addLine(){
          if(i >= lines.length){
            body.innerHTML += '<div><span class="c">$</span> <span class="cursor-blink"></span></div>';
            body.scrollTop = body.scrollHeight;
            return;
          }
          const line = lines[i++];
          const div = document.createElement('div');
          div.innerHTML = line.c || '&nbsp;';
          body.appendChild(div);
          body.scrollTop = body.scrollHeight;
          typers[kind] = setTimeout(addLine, line.d || 60);
        }
        addLine();
      }

      function activate(kind: string){
        tabs.forEach(t => (t as HTMLElement).classList.toggle('active', (t as HTMLElement).dataset.tab === kind));
        panels.forEach(p => p.classList.toggle('active', p.id === `panel-${kind}`));
        runTerminal(kind);
      }

      tabs.forEach(t => t.addEventListener('click', ()=> activate((t as HTMLElement).dataset.tab!)));

      const modelsSec = document.getElementById('models');
      if(modelsSec){
        const io = new IntersectionObserver((ents)=>{
          ents.forEach(e=>{
            if(e.isIntersecting){ runTerminal('llm'); io.unobserve(e.target); }
          });
        }, { threshold: 0.25 });
        io.observe(modelsSec);
      }
    })();

    /* ═══════════ 6. MAGNETIC BUTTONS ═══════════ */
    (function(){
      const btns = document.querySelectorAll('.btn, .nav-cta');
      btns.forEach(btn=>{
        btn.addEventListener('mousemove', (e: Event)=>{
          const me = e as MouseEvent;
          const rect = (btn as HTMLElement).getBoundingClientRect();
          const x = me.clientX - rect.left - rect.width/2;
          const y = me.clientY - rect.top - rect.height/2;
          (btn as HTMLElement).style.transform = `translate(${x*0.15}px, ${y*0.25}px)`;
        });
        btn.addEventListener('mouseleave', ()=>{
          (btn as HTMLElement).style.transform = '';
        });
      });
    })();

    /* ═══════════ 7. FLOW DIAGRAM LINES ═══════════ */
    (function(){
      function positionLines(){
        const stage = document.getElementById('flowStage');
        if(!stage) return;
        stage.querySelectorAll('.flow-line').forEach(l=>l.remove());
        const steps = stage.querySelectorAll('.flow-step');
        if(steps.length < 2) return;
        const stageRect = stage.getBoundingClientRect();
        for(let i=0;i<steps.length-1;i++){
          const a = steps[i].querySelector('.flow-node')!.getBoundingClientRect();
          const b = steps[i+1].querySelector('.flow-node')!.getBoundingClientRect();
          if(a.top > stageRect.top + 200 && b.top > a.top + 50) continue;
          if(Math.abs(a.top - b.top) > 30) continue;
          const line = document.createElement('div');
          line.className = 'flow-line';
          line.style.left = `${a.right - stageRect.left - 10}px`;
          line.style.width = `${b.left - a.right + 20}px`;
          line.style.top = `${a.top + a.height/2 - stageRect.top}px`;
          stage.appendChild(line);
        }
      }
      setTimeout(positionLines, 200);
      window.addEventListener('resize', ()=>{ setTimeout(positionLines, 80); });
      const flow = document.querySelector('.flow');
      if(flow){
        const io = new IntersectionObserver((ents)=>{
          ents.forEach(e=>{ if(e.isIntersecting) setTimeout(positionLines, 100); });
        }, { threshold: 0.1 });
        io.observe(flow);
      }
    })();

    /* ═══════════ 8. HOVER PARALLAX ON BOXES ═══════════ */
    (function(){
      const boxes = document.querySelectorAll('.feat, .prob, .tech-item, .hw-card, .phase');
      boxes.forEach(box=>{
        box.addEventListener('mousemove', (e: Event)=>{
          const me = e as MouseEvent;
          const rect = (box as HTMLElement).getBoundingClientRect();
          const x = (me.clientX - rect.left) / rect.width - 0.5;
          const y = (me.clientY - rect.top) / rect.height - 0.5;
          const inner = (box as HTMLElement).firstElementChild as HTMLElement|null;
          if(inner){
            inner.style.transform = `translate(${x*4}px, ${y*4}px)`;
            inner.style.transition = 'transform .2s ease-out';
          }
        });
        box.addEventListener('mouseleave', ()=>{
          const inner = (box as HTMLElement).firstElementChild as HTMLElement|null;
          if(inner){
            inner.style.transform = '';
            inner.style.transition = 'transform .5s cubic-bezier(.2,.8,.2,1)';
          }
        });
      });
    })();

    /* ═══════════ 9. SPOTLIGHT CURSOR ON BOXES ═══════════ */
    (function(){
      const boxes = document.querySelectorAll('.feat, .prob, .tech-item, .hw-card, .phase, .stat');
      boxes.forEach(box=>{
        box.addEventListener('mousemove', (e: Event)=>{
          const me = e as MouseEvent;
          const rect = (box as HTMLElement).getBoundingClientRect();
          const x = ((me.clientX - rect.left)/rect.width)*100;
          const y = ((me.clientY - rect.top)/rect.height)*100;
          (box as HTMLElement).style.backgroundImage =
            `radial-gradient(circle 200px at ${x}% ${y}%, rgba(255,255,255,.045), transparent 70%)`;
        });
        box.addEventListener('mouseleave', ()=>{
          (box as HTMLElement).style.backgroundImage = '';
        });
      });
    })();

    /* ═══════════ 10. TICKER DUPLICATE (seamless) ═══════════ */
    (function(){
      const ticker = document.getElementById('ticker');
      if(!ticker) return;
      ticker.innerHTML += ticker.innerHTML;
    })();

    /* ═══════════ 11. CURSOR-TRACKING GRADIENT ═══════════ */
    (function(){
      const hero = document.querySelector('.hero') as HTMLElement|null;
      const grad = document.getElementById('heroGradient') as HTMLElement|null;
      if(!hero || !grad) return;
      let targetX = 50, targetY = 50, curX = 50, curY = 50;
      let raf: number|null = null;
      hero.addEventListener('mousemove', (e: MouseEvent)=>{
        const r = hero.getBoundingClientRect();
        targetX = ((e.clientX - r.left) / r.width) * 100;
        targetY = ((e.clientY - r.top) / r.height) * 100;
        if(!raf) raf = requestAnimationFrame(tick);
      });
      hero.addEventListener('mouseleave', ()=>{
        targetX = 50; targetY = 50;
        if(!raf) raf = requestAnimationFrame(tick);
      });
      function tick(){
        curX += (targetX - curX) * 0.12;
        curY += (targetY - curY) * 0.12;
        grad!.style.setProperty('--mx', curX.toFixed(2) + '%');
        grad!.style.setProperty('--my', curY.toFixed(2) + '%');
        if(Math.abs(targetX - curX) > 0.1 || Math.abs(targetY - curY) > 0.1){
          raf = requestAnimationFrame(tick);
        } else { raf = null; }
      }
      grad!.style.setProperty('--mx', '50%');
      grad!.style.setProperty('--my', '50%');
    })();

    /* ═══════════ 12. THEME TOGGLE ═══════════ */
    (function(){
      const btn = document.getElementById('themeToggle');
      if(!btn) return;
      function apply(theme: string){
        document.documentElement.setAttribute('data-theme', theme);
        try{ localStorage.setItem('pinaivu-theme', theme); }catch(e){}
      }
      btn.addEventListener('click', ()=>{
        const cur = document.documentElement.getAttribute('data-theme') || 'dark';
        apply(cur === 'dark' ? 'light' : 'dark');
      });
    })();

    /* ═══════════ 13. MAGNETIC BUTTONS ═══════════ */
    (function(){
      const magnets = document.querySelectorAll('.btn, .nav-cta');
      magnets.forEach(m=>{
        m.addEventListener('mousemove', (e: Event)=>{
          const me = e as MouseEvent;
          const r = (m as HTMLElement).getBoundingClientRect();
          const x = (me.clientX - r.left - r.width/2) * 0.3;
          const y = (me.clientY - r.top - r.height/2) * 0.3;
          (m as HTMLElement).style.transform = `translate(${x}px, ${y}px)`;
        });
        m.addEventListener('mouseleave', ()=>{
          (m as HTMLElement).style.transform = '';
        });
      });
    })();

    /* ═══════════ 14. TILT ON CARDS ═══════════ */
    (function(){
      const cards = document.querySelectorAll('.prob, .feat, .m-cell, .tech-item, .hw-card, .phase, .stat');
      cards.forEach(card=>{
        card.addEventListener('mousemove', (e: Event)=>{
          const me = e as MouseEvent;
          const r = (card as HTMLElement).getBoundingClientRect();
          const cx = (me.clientX - r.left) / r.width - 0.5;
          const cy = (me.clientY - r.top) / r.height - 0.5;
          (card as HTMLElement).style.transform = `perspective(1000px) rotateX(${-cy*3}deg) rotateY(${cx*3}deg) translateZ(0)`;
        });
        card.addEventListener('mouseleave', ()=>{
          (card as HTMLElement).style.transform = '';
        });
      });
    })();

    /* ═══════════ 15. WORD REVEAL ON SECTION TITLES ═══════════ */
    (function(){
      const titles = document.querySelectorAll('.sec-title');
      titles.forEach(t=>{
        const el = t as HTMLElement;
        if(el.dataset.split) return;
        const html = el.innerHTML;
        const parts = html.split(/(<[^>]+>|\s+)/).filter(p=>p.length);
        el.innerHTML = parts.map(p=>{
          if(p.startsWith('<') || /^\s+$/.test(p)) return p;
          return `<span class="sec-word"><span>${p}</span></span>`;
        }).join(' ');
        el.dataset.split = '1';
      });
      const io = new IntersectionObserver((ents)=>{
        ents.forEach(e=>{
          if(!e.isIntersecting) return;
          const words = e.target.querySelectorAll('.sec-word > span');
          words.forEach((w,i)=>{
            (w as HTMLElement).style.transitionDelay = (i*0.06) + 's';
            requestAnimationFrame(()=>w.classList.add('in'));
          });
          io.unobserve(e.target);
        });
      }, { threshold: 0.15, rootMargin: '0px 0px -8% 0px' });
      titles.forEach(t=>io.observe(t));
    })();

    /* ═══════════ 16. FLOATING LABELS ═══════════ */
    (function(){
      const floats = document.querySelectorAll('.hero-marker, .hero-kicker, .hero-tag');
      floats.forEach((el, i)=>{
        (el as HTMLElement).style.animation = `float-sub ${5 + i*0.4}s ease-in-out ${i*0.2}s infinite alternate`;
      });
    })();
  }, []);

  useEffect(() => {
    function onKey(e: KeyboardEvent) { if(e.key === 'Escape') setShowWaitlist(false); }
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, []);

  return (
    <div className="page">
      {showWaitlist && <WaitlistModal onClose={() => setShowWaitlist(false)} />}
      {/* Scroll Progress */}
      <div className="prog"><div className="prog-bar" id="progBar"></div></div>

      {/* NAV */}
      <nav className="nav" id="nav">
        <div className="nav-inner">
          <a href="#top" className="nav-logo">
            <span className="nav-logo-mark">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
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
          <ul className="nav-links">
            <li><a href="#problem">Problem</a></li>
            <li><a href="#features">Features</a></li>
            <li><a href="#flow">Flow</a></li>
            <li><a href="#models">Models</a></li>
            <li><a href="#tech">Tech</a></li>
            <li><a href="#roadmap">Roadmap</a></li>
          </ul>
          <button className="theme-toggle" id="themeToggle" aria-label="Toggle theme">
            <svg className="sun" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/></svg>
            <svg className="moon" viewBox="0 0 24 24" fill="currentColor" stroke="none"><path d="M21 12.79A9 9 0 1111.21 3 7 7 0 0021 12.79z"/></svg>
          </button>
          <button className="nav-cta" onClick={() => setShowWaitlist(true)}>
            <span>Join Waitlist <span className="arrow">↗</span></span>
          </button>
        </div>
      </nav>

      {/* HERO */}
      <section className="hero" id="top">
        <canvas id="hero-canvas"></canvas>
        <div className="hero-gradient" id="heroGradient"></div>
        <div className="hero-grid-sm"></div>
        <div className="hero-grid"></div>
        <div className="hero-crosshair"></div>
        <div className="hero-vignette"></div>
        <div className="scanlines"></div>
        <div className="hero-corners"><span className="hc-bl"></span><span className="hc-br"></span></div>
        <div className="hero-glyphs" id="heroGlyphs"></div>
        <div className="hero-marker tl"><span className="dot"></span> 0x<span id="heroHash">3f2a9b…c417</span></div>
        <div className="hero-marker tr">Ed25519 / Merkle / libp2p <span className="bar"></span></div>
        <div className="hero-marker bl">v2.0 · April 2026 · Living Document</div>
        <div className="hero-content">
          <div className="hero-center">
            <div className="hero-tag"><span className="live"></span> Phase C · Protocol v2.0 · Zero blockchain required</div>
            <div className="hero-kicker">A P2P Inference Protocol · 2026</div>
            <h1 className="hero-title">
              <span className="word"><span>Trust</span></span>
              <span className="word"><span>from</span></span>
              <span className="word"><span><em>cryptography,</em></span></span>
              <span className="word"><span>not chains.</span></span>
            </h1>
            <p className="hero-sub">Pinaivu AI grounds every guarantee in <strong>Ed25519 signatures</strong> and <strong>SHA-256 Merkle proofs</strong> — not a coordinator, not a token. Settlement, storage and anchoring are <strong>pluggable</strong>. Swap a TOML value, not your stack.</p>
            <div className="hero-ctas">
              <button className="btn btn-primary" onClick={() => setShowWaitlist(true)}><span>Join Waitlist</span> <span className="arrow">↗</span></button>
              <a className="btn btn-ghost" href="/PinaivuAI_Whitepaper.pdf" target="_blank" rel="noopener noreferrer"><span>Read Whitepaper v2.0</span></a>
            </div>
          </div>
          <div className="hero-ticker">
            <div className="track" id="ticker">
              <span>ED25519 IDENTITY</span><span>SHA-256 MERKLE TREE</span><span>GOSSIPSUB REPUTATION</span><span>AES-256-GCM SESSIONS</span><span>X25519 CONTEXT KEYS</span><span>SIGNED PROOF OF INFERENCE</span><span>SETTLEMENT-AGNOSTIC ESCROW</span><span>LIBP2P · QUIC · NOISE</span><span>IPFS · WALRUS · LOCAL</span><span>FREE · RECEIPT · CHANNEL · SUI · EVM</span><span>STANDARD · PRIVATE · FRAGMENTED · MAXIMUM</span><span>OFFLINE VERIFIABLE</span>
            </div>
          </div>
        </div>
        <div className="hero-scroll">Scroll<div className="line"></div>↓</div>
      </section>

      {/* STATS */}
      <section style={{padding:0}}>
        <div className="grid stats" id="stats">
          <div className="stat" style={{'--pct':'100%'} as React.CSSProperties}>
            <div className="stat-val"><span data-count="5">0</span><span className="suf">/5</span></div>
            <div className="stat-lbl">Guarantees met <span className="delta">G1–G5</span></div>
            <div className="stat-bar"></div>
          </div>
          <div className="stat" style={{'--pct':'100%'} as React.CSSProperties}>
            <div className="stat-val"><span data-count="0">0</span></div>
            <div className="stat-lbl">Blockchains required <span className="delta">Optional</span></div>
            <div className="stat-bar"></div>
          </div>
          <div className="stat" style={{'--pct':'82%'} as React.CSSProperties}>
            <div className="stat-val"><span data-count="6">0</span></div>
            <div className="stat-lbl">Stack layers <span className="delta">Swappable</span></div>
            <div className="stat-bar"></div>
          </div>
          <div className="stat" style={{'--pct':'96%'} as React.CSSProperties}>
            <div className="stat-val"><span data-count="128">0</span><span className="suf">-bit</span></div>
            <div className="stat-lbl">Ed25519 security <span className="delta">RFC 8032</span></div>
            <div className="stat-bar"></div>
          </div>
        </div>
      </section>

      {/* MANIFESTO */}
      <section className="manifesto" id="manifesto">
        <div className="m-wrap">
          <div className="m-label"><b>§ 001</b> Thesis <span style={{color:'var(--fg-3)'}}>· Drafted for the open network · v2.0</span></div>
          <div className="m-grid">

            <div className="m-cell m-thesis">
              <div className="m-meta"><span>Abstract · Line 01</span><span><b>Self-sufficient</b></span></div>
              <h2>
                Every prior inference marketplace grounds trust in a <em>coordinator</em> or a <em>specific chain</em>. Pinaivu AI takes a third path: trust is grounded <span className="hl">exclusively in cryptography</span> &mdash; Ed25519 identity, SHA-256 Merkle proofs, AES-256-GCM sessions. Any chain becomes an <em>optional anchor</em> on a system that already works.
              </h2>
              <div className="m-sig"><span>Offline verifiable</span><span>No coordinator</span><span>Chain-optional</span></div>
            </div>

            <div className="m-cell m-spec">
              <div>
                <div className="spec-k">Primitive · 01</div>
                <div className="spec-v">Proof<em> of Inference</em></div>
              </div>
              <div className="spec-d">A signed execution receipt verifiable offline with only the producing node&apos;s public key.</div>
              <div className="m-meta"><span>π = (req, model, tᵢ, tₒ, Δ, H_in, H_out, pk, σ)</span></div>
            </div>

            <div className="m-cell m-code">
              <div><span className="c"># verify π offline — no network, no chain</span></div>
              <div><span className="k">let</span> msg = canonical(π)</div>
              <div><span className="k">let</span> vk  = VerifyingKey::<span className="s">from_bytes</span>(π.pk_N)</div>
              <div><span className="k">assert</span> EdDSA::<span className="s">verify</span>(vk, msg, π.σ)</div>
              <div><span className="c"># O(1) — constant time</span></div>
            </div>

            <div className="m-cell m-kv"><div className="k">G1</div><div className="v">Session<em> privacy</em></div><div className="m-meta"><span>Client-held K</span><span>X25519 DH</span></div></div>
            <div className="m-cell m-kv"><div className="k">G2</div><div className="v">Node<em> accountability</em></div><div className="m-meta"><span>Ed25519 σ</span><span>Merkle π</span></div></div>
            <div className="m-cell m-tick"><div><div className="tv">∅</div>Zero blockchain required</div></div>
            <div className="m-cell m-dot-mtx"></div>

            <div className="m-cell m-kv"><div className="k">G3</div><div className="v">Settlement<em> neutrality</em></div><div className="m-meta"><span>free · receipt · channel · sui · evm</span></div></div>
            <div className="m-cell m-kv"><div className="k">G4</div><div className="v">Storage<em> neutrality</em></div><div className="m-meta"><span>local · ipfs · walrus</span></div></div>

            <div className="m-cell m-ring">
              <svg viewBox="0 0 70 70">
                <circle className="bg" cx="35" cy="35" r="30"/>
                <circle className="fg" cx="35" cy="35" r="30"/>
              </svg>
              <div className="lbl">5/5</div>
            </div>
            <div className="m-cell m-kv"><div className="k">G5</div><div className="v">Permissionless<em> participation</em></div><div className="m-meta"><span>libp2p PeerId = pk_N</span></div></div>

            <div className="m-cell m-barchart"><div className="b"></div><div className="b"></div><div className="b"></div><div className="b"></div><div className="b"></div><div className="b"></div><div className="b"></div><div className="b"></div><div className="b"></div><div className="b"></div></div>
            <div className="m-cell m-kv"><div className="k">Reputation</div><div className="v">score(N)<em> = α·ṡ + β·ℓ</em></div><div className="m-meta"><span>α=0.6</span><span>β=0.4</span><span>L_max=5s</span></div></div>
            <div className="m-cell m-kv"><div className="k">Gossip</div><div className="v">600s<em> · broadcast root</em></div><div className="m-meta"><span>/pinaivu/reputation/1.0.0</span></div></div>
          </div>
        </div>
      </section>

      {/* PROBLEMS */}
      <section id="problem">
        <div className="sec-head">
          <div>
            <div className="sec-label"><b>002</b> · The Failure Mode</div>
            <h2 className="sec-title reveal">Cloud AI bakes in <em>three consequences</em><br/>that aren&apos;t technical requirements.</h2>
          </div>
          <p className="sec-desc reveal reveal-d1">For every turn (P, C, R), today&apos;s provider observes all three, sets price ρ unilaterally, and revokes access at will. None of this is forced by the maths &mdash; only by the architecture.</p>
        </div>
        <div className="grid problems">
          <div className="prob reveal">
            <div>
              <div className="prob-num"><span>01 — Context exposure</span><span className="tag">G1</span></div>
              <div className="prob-visual">
                <div className="viz-choke">
                  <svg viewBox="0 0 200 140" preserveAspectRatio="xMidYMid meet">
                    <line x1="30" y1="20" x2="100" y2="70"/>
                    <line x1="30" y1="70" x2="100" y2="70"/>
                    <line x1="30" y1="120" x2="100" y2="70"/>
                    <line x1="170" y1="20" x2="100" y2="70"/>
                    <line x1="170" y1="70" x2="100" y2="70"/>
                    <line x1="170" y1="120" x2="100" y2="70"/>
                  </svg>
                  <div className="node" style={{top:'14%',left:'15%'}}></div><div className="node" style={{top:'50%',left:'15%'}}></div><div className="node" style={{top:'86%',left:'15%'}}></div>
                  <div className="node" style={{top:'14%',left:'85%'}}></div><div className="node" style={{top:'50%',left:'85%'}}></div><div className="node" style={{top:'86%',left:'85%'}}></div>
                  <div className="node center"></div>
                </div>
              </div>
              <h3>Provider sees (P, C, R)</h3>
              <p>Every prompt, every accumulated context, every response flows through one party. Pinaivu AI keeps the full session `C` encrypted under a client-held key `K`; the GPU node sees only the decrypted context window for the current turn.</p>
            </div>
            <div className="corner tl"><CornerSVG/></div><div className="corner tr"><CornerSVG/></div><div className="corner bl"><CornerSVG/></div><div className="corner br"><CornerSVG/></div>
          </div>
          <div className="prob reveal reveal-d1">
            <div>
              <div className="prob-num"><span>02 — Chain dependence</span><span className="tag">G3</span></div>
              <div className="prob-visual"><div className="viz-lock"><div className="viz-lock-shape"></div></div></div>
              <h3>One token, one ecosystem</h3>
              <p>Bittensor collapses without TAO. Every prior decentralised inference system grounds trust in a specific chain, token and validator set. Pinaivu AI&apos;s trust model is self-sufficient; any chain is an optional settlement adapter selected in a TOML file.</p>
            </div>
            <div className="corner tl"><CornerSVG/></div><div className="corner tr"><CornerSVG/></div><div className="corner bl"><CornerSVG/></div><div className="corner br"><CornerSVG/></div>
          </div>
          <div className="prob reveal reveal-d2">
            <div>
              <div className="prob-num"><span>03 — Unverifiable work</span><span className="tag">G2</span></div>
              <div className="prob-visual"><div className="viz-down"><div className="bar"></div><div className="bar"></div><div className="bar"></div><div className="bar"></div><div className="bar"></div><div className="bar"></div><div className="bar"></div><div className="bar"></div></div></div>
              <h3>No receipt, no recourse</h3>
              <p>Batch marketplaces (io.net, Akash) and routers (Fortytwo) can&apos;t prove node N ran job J at the claimed parameters. Pinaivu AI ships every response with a self-verifiable ProofOfInference &mdash; Ed25519-signed, offline checkable, binding on (model, tokens, Δ, H_in, H_out).</p>
            </div>
            <div className="corner tl"><CornerSVG/></div><div className="corner tr"><CornerSVG/></div><div className="corner bl"><CornerSVG/></div><div className="corner br"><CornerSVG/></div>
          </div>
        </div>
      </section>

      {/* FEATURES */}
      <section id="features">
        <div className="sec-head">
          <div>
            <div className="sec-label"><b>003</b> · Six Layers</div>
            <h2 className="sec-title reveal">Every layer is <em>independently replaceable.</em></h2>
          </div>
          <p className="sec-desc reveal reveal-d1">Layers interact only through trait interfaces. Layer 0 (Crypto) has no external deps. Every layer above it may use external infra, but none is required.</p>
        </div>
        <div className="grid features">
          <div className="feat reveal">
            <div>
              <div className="feat-hd"><div className="feat-idx">L · 06 · Application</div>
                <div className="feat-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 9h18M9 21V9"/></svg></div>
              </div>
              <h3>OpenAI-compatible surface</h3>
              <p>TypeScript SDK, drop-in HTTP API, Web UI. Change the base URL; keep your code. Streaming, sessions and proof retrieval are native.</p>
            </div>
            <div className="feat-foot">TS SDK · HTTP · Web UI <span className="arrow">→</span></div>
          </div>
          <div className="feat reveal reveal-d1">
            <div>
              <div className="feat-hd"><div className="feat-idx">L · 05 · Session</div>
                <div className="feat-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><rect x="4" y="10" width="16" height="10" rx="2"/><path d="M8 10V7a4 4 0 018 0v3"/></svg></div>
              </div>
              <h3>E2E encrypted memory</h3>
              <p>Full history `C` is AES-256-GCM encrypted under a client-held `K`. The GPU node decrypts only the active context window &mdash; never `C`, never `K`.</p>
            </div>
            <div className="feat-foot">AES-GCM · X25519 · Portable <span className="arrow">→</span></div>
          </div>
          <div className="feat reveal reveal-d2">
            <div>
              <div className="feat-hd"><div className="feat-idx">L · 04 · Reputation</div>
                <div className="feat-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><circle cx="6" cy="18" r="2"/><circle cx="18" cy="18" r="2"/><circle cx="6" cy="6" r="2"/><circle cx="18" cy="6" r="2"/><circle cx="12" cy="12" r="2"/><path d="M12 10V8M12 16v-2M10 12H8M16 12h-2"/></svg></div>
              </div>
              <h3>Merkle tree, gossiped</h3>
              <p>Every node keeps a Merkle tree of its signed proofs. The root is broadcast over libp2p gossipsub every 10 min. Chain anchoring is optional.</p>
            </div>
            <div className="feat-foot">SHA-256 · Gossipsub · O(log n) <span className="arrow">→</span></div>
          </div>
          <div className="feat reveal">
            <div>
              <div className="feat-hd"><div className="feat-idx">L · 03 · Marketplace</div>
                <div className="feat-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><path d="M3 12l9-9 9 9-9 9z"/><path d="M8 12h8M12 8v8"/></svg></div>
              </div>
              <h3>200ms sealed-bid auction</h3>
              <p>Client broadcasts request; nodes pass six cheap-to-expensive checks and submit a bid. Composite score (0.4×price + 0.3×latency + 0.3×rep) picks the winner.</p>
            </div>
            <div className="feat-foot">libp2p · Sealed-bid · First-price <span className="arrow">→</span></div>
          </div>
          <div className="feat reveal reveal-d1">
            <div>
              <div className="feat-hd"><div className="feat-idx">L · 02 · Settlement</div>
                <div className="feat-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><rect x="3" y="6" width="18" height="12" rx="2"/><circle cx="12" cy="12" r="2.2"/><path d="M6 10v4M18 10v4"/></svg></div>
              </div>
              <h3>Pluggable escrow</h3>
              <p>Five adapters: free, signed-receipt, off-chain channel, Sui, EVM. Pick in TOML; same binary. Payment channels amortise gas 50× over 100 requests.</p>
            </div>
            <div className="feat-foot">free · receipt · channel · sui · evm <span className="arrow">→</span></div>
          </div>
          <div className="feat reveal reveal-d2">
            <div>
              <div className="feat-hd"><div className="feat-idx">L · 01 · Storage</div>
                <div className="feat-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><ellipse cx="12" cy="5" rx="8" ry="2.5"/><path d="M4 5v14c0 1.4 3.6 2.5 8 2.5s8-1.1 8-2.5V5"/><path d="M4 12c0 1.4 3.6 2.5 8 2.5s8-1.1 8-2.5"/></svg></div>
              </div>
              <h3>Content-addressed, agnostic</h3>
              <p>Three-method interface: put/get/delete. Local, IPFS, Walrus, Memory &mdash; same protocol. SHA-256 IDs mean put(b)=put(b) deduplicates for free.</p>
            </div>
            <div className="feat-foot">local · ipfs · walrus <span className="arrow">→</span></div>
          </div>
        </div>
      </section>

      {/* FLOW */}
      <section className="flow" id="flow">
        <div className="sec-head">
          <div>
            <div className="sec-label"><b>004</b> · Request Flow</div>
            <h2 className="sec-title reveal">From prompt to proof, <em>in under a second.</em></h2>
          </div>
          <p className="sec-desc reveal reveal-d1">Four stages. Each one cryptographically verifiable — from the sealed-bid auction through Ed25519-signed proof delivery.</p>
        </div>
        <div className="flow-diagram">
          <div className="flow-stage" id="flowStage">
            <div className="flow-step">
              <div className="flow-node">
                <span className="flow-num">1</span>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><path d="M12 2v14"/><path d="M6 12l6 6 6-6"/><rect x="4" y="18" width="16" height="4"/></svg>
              </div>
              <div className="flow-label">Step 01 · ~5ms</div>
              <h4>Broadcast</h4>
              <p>Client broadcasts an InferenceRequest on the gossipsub topic for the required model, carrying model ID, budget, and privacy level — not the context (that stays client-side until a winner is chosen).</p>
            </div>
            <div className="flow-step">
              <div className="flow-node">
                <span className="flow-num">2</span>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><circle cx="12" cy="12" r="3"/><path d="M12 2v4M12 18v4M2 12h4M18 12h4"/><circle cx="6" cy="6" r="2"/><circle cx="18" cy="6" r="2"/><circle cx="6" cy="18" r="2"/><circle cx="18" cy="18" r="2"/></svg>
              </div>
              <div className="flow-label">Step 02 · 200ms</div>
              <h4>Sealed-bid Auction</h4>
              <p>GPU nodes pass six checks (model, capacity, queue, budget, privacy, throttle) and submit bids. Client picks winner by composite score: 0.4×price + 0.3×latency + 0.3×reputation.</p>
            </div>
            <div className="flow-step">
              <div className="flow-node">
                <span className="flow-num">3</span>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><rect x="3" y="4" width="18" height="12" rx="1"/><path d="M8 20h8M12 16v4"/><line x1="7" y1="8" x2="7" y2="12"/><line x1="11" y1="8" x2="11" y2="12"/><line x1="15" y1="8" x2="15" y2="12"/></svg>
              </div>
              <div className="flow-label">Step 03 · ~620ms</div>
              <h4>Inference</h4>
              <p>Client encrypts the context window W for the winning node via X25519 DH and sends it directly to that node&apos;s API. Node decrypts W in RAM, runs inference, streams tokens back, then zeroes W.</p>
            </div>
            <div className="flow-step">
              <div className="flow-node">
                <span className="flow-num">4</span>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><path d="M5 12l5 5 9-9"/><circle cx="12" cy="12" r="10"/></svg>
              </div>
              <div className="flow-label">Step 04 · ~20ms</div>
              <h4>Proof + Settle</h4>
              <p>Node signs ProofOfInference π binding (model, tokens, Δ, H_in, H_out) with Ed25519. π is appended to the node&apos;s Merkle tree. Settlement adapter executes and ships π to the client.</p>
            </div>
          </div>
          <div className="flow-readout">
            <div className="flow-readout-item">
              <div className="flow-readout-val"><span data-count="845">0</span>ms</div>
              <div className="flow-readout-lbl">Total · end to end</div>
            </div>
            <div className="flow-readout-item">
              <div className="flow-readout-val">42<span style={{color:'var(--fg-3)'}}>tok/s</span></div>
              <div className="flow-readout-lbl">Throughput · 70B model</div>
            </div>
            <div className="flow-readout-item">
              <div className="flow-readout-val">0.0003<span style={{color:'var(--fg-3)'}}> PEER</span></div>
              <div className="flow-readout-lbl">Cost · 256 tokens</div>
            </div>
          </div>
        </div>
      </section>

      {/* MODELS */}
      <section id="models">
        <div className="sec-head">
          <div>
            <div className="sec-label"><b>005</b> · Model Catalog</div>
            <h2 className="sec-title reveal">Run the models you want. <em>Not the ones they allow.</em></h2>
          </div>
          <p className="sec-desc reveal reveal-d1">Every open-weight checkpoint that fits in VRAM. Pre-cached for the popular ones, on-demand for the rest.</p>
        </div>
        <div className="models-shell">
          <div className="models-head">
            <div className="tabs" id="modelTabs">
              <button className="tab active" data-tab="llm">Language</button>
              <button className="tab" data-tab="vision">Vision</button>
              <button className="tab" data-tab="audio">Audio</button>
            </div>
            <div style={{fontFamily:'var(--mono)',fontSize:'.58rem',letterSpacing:'.22em',textTransform:'uppercase',color:'var(--fg-3)'}}>
              <span style={{color:'#fff'}}>84</span> models live · <span style={{color:'#fff'}}>2,847</span> variants
            </div>
          </div>
          <div className="models-box">

            <div className="model-panel active" id="panel-llm">
              <div className="models-pane">
                <div>
                  <div className="models-meta"><span className="pill">LLM</span><span className="pill">Text</span><span className="pill">FP16 · INT8 · INT4</span></div>
                  <h3>Llama 3.1 · 405B</h3>
                  <div className="author">Meta · Open weights · Released Jul 2024</div>
                  <p>The largest open LLM running on the network. Sharded across 16 consumer GPUs via tensor parallel. Competitive with GPT-4 on most benchmarks at a fraction of the cost.</p>
                </div>
                <div className="models-spec">
                  <div className="spec"><div className="spec-lbl">Parameters</div><div className="spec-val">405<span className="dim">B</span></div></div>
                  <div className="spec"><div className="spec-lbl">Context</div><div className="spec-val">128<span className="dim">K tokens</span></div></div>
                  <div className="spec"><div className="spec-lbl">Throughput</div><div className="spec-val">42<span className="dim"> tok/s</span></div></div>
                  <div className="spec"><div className="spec-lbl">Cost / 1K</div><div className="spec-val">$0.003</div></div>
                </div>
              </div>
              <div className="terminal">
                <div className="term-bar">
                  <div className="td"></div><div className="td"></div><div className="td"></div>
                  <div className="term-title">peer-cli · llama-3.1-405b</div>
                </div>
                <div className="term-body" data-term="llm"></div>
              </div>
            </div>

            <div className="model-panel" id="panel-vision">
              <div className="models-pane">
                <div>
                  <div className="models-meta"><span className="pill">Vision</span><span className="pill">Diffusion</span><span className="pill">1024²</span></div>
                  <h3>FLUX.1 · Pro</h3>
                  <div className="author">Black Forest Labs · Open weights · Aug 2024</div>
                  <p>State-of-the-art text-to-image at 1024² native resolution. Runs on a single consumer GPU. 4-step Turbo variant generates in under 1 second per image.</p>
                </div>
                <div className="models-spec">
                  <div className="spec"><div className="spec-lbl">Resolution</div><div className="spec-val">1024<span className="dim">×1024</span></div></div>
                  <div className="spec"><div className="spec-lbl">Steps</div><div className="spec-val">4<span className="dim"> (turbo)</span></div></div>
                  <div className="spec"><div className="spec-lbl">Latency</div><div className="spec-val">2.1<span className="dim">s</span></div></div>
                  <div className="spec"><div className="spec-lbl">Cost / img</div><div className="spec-val">$0.004</div></div>
                </div>
              </div>
              <div className="terminal">
                <div className="term-bar">
                  <div className="td"></div><div className="td"></div><div className="td"></div>
                  <div className="term-title">peer-cli · flux-1-pro</div>
                </div>
                <div className="term-body" data-term="vision"></div>
              </div>
            </div>

            <div className="model-panel" id="panel-audio">
              <div className="models-pane">
                <div>
                  <div className="models-meta"><span className="pill">Audio</span><span className="pill">STT</span><span className="pill">Streaming</span></div>
                  <h3>Whisper · Large v3</h3>
                  <div className="author">OpenAI · Open weights · MIT license</div>
                  <p>99-language speech-to-text with automatic language detection. Runs 52× realtime on an RTX 3090. Native WebSocket streaming for voice applications.</p>
                </div>
                <div className="models-spec">
                  <div className="spec"><div className="spec-lbl">Languages</div><div className="spec-val">99</div></div>
                  <div className="spec"><div className="spec-lbl">Speed</div><div className="spec-val">52×<span className="dim"> realtime</span></div></div>
                  <div className="spec"><div className="spec-lbl">TTFT</div><div className="spec-val">&lt;300<span className="dim">ms</span></div></div>
                  <div className="spec"><div className="spec-lbl">Cost / min</div><div className="spec-val">$0.001</div></div>
                </div>
              </div>
              <div className="terminal">
                <div className="term-bar">
                  <div className="td"></div><div className="td"></div><div className="td"></div>
                  <div className="term-title">peer-cli · whisper-v3-large</div>
                </div>
                <div className="term-body" data-term="audio"></div>
              </div>
            </div>

          </div>
        </div>
      </section>

      {/* COMPARE */}
      <section id="compare">
        <div className="sec-head">
          <div>
            <div className="sec-label"><b>006</b> · Comparison</div>
            <h2 className="sec-title reveal">Against the <em>incumbents.</em></h2>
          </div>
          <p className="sec-desc reveal reveal-d1">Every prior system either lacks G2 (no verifiable accountability) or sacrifices G3/G4 (hard-coded chain and storage). Pinaivu AI is the first to satisfy all five guarantees simultaneously.</p>
        </div>
        <div className="compare-shell">
          <div className="compare-box reveal">
            <div className="compare-row head">
              <div className="compare-cell">Property</div>
              <div className="compare-cell">Pinaivu AI</div>
              <div className="compare-cell">Bittensor</div>
              <div className="compare-cell">QVAC</div>
              <div className="compare-cell">io.net</div>
              <div className="compare-cell">Fortytwo</div>
            </div>
            <div className="compare-row">
              <div className="compare-cell"><span className="rowtitle">G1 — Session privacy</span></div>
              <div className="compare-cell yes"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12l5 5 9-9"/></svg> AES-256-GCM</div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> Validators see all</div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> Not addressed</div>
              <div className="compare-cell no">N/A · batch only</div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> Centralised</div>
            </div>
            <div className="compare-row">
              <div className="compare-cell"><span className="rowtitle">G2 — Node accountability</span></div>
              <div className="compare-cell yes"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12l5 5 9-9"/></svg> Ed25519 + Merkle</div>
              <div className="compare-cell">Partial · validators</div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> No receipts</div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> No receipts</div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> No receipts</div>
            </div>
            <div className="compare-row">
              <div className="compare-cell"><span className="rowtitle">G3 — Settlement neutrality</span></div>
              <div className="compare-cell yes"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12l5 5 9-9"/></svg> 5 adapters</div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> TAO only</div>
              <div className="compare-cell">No payment</div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> IO token</div>
              <div className="compare-cell">N/A · centralised</div>
            </div>
            <div className="compare-row">
              <div className="compare-cell"><span className="rowtitle">G5 — Permissionless</span></div>
              <div className="compare-cell yes"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12l5 5 9-9"/></svg> PeerId = pk_N</div>
              <div className="compare-cell yes"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12l5 5 9-9"/></svg></div>
              <div className="compare-cell yes"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12l5 5 9-9"/></svg></div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> KYC required</div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg> Centralised</div>
            </div>
            <div className="compare-row">
              <div className="compare-cell"><span className="rowtitle">Persistent sessions</span></div>
              <div className="compare-cell yes"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12l5 5 9-9"/></svg> E2E encrypted</div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg></div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg></div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg></div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg></div>
            </div>
            <div className="compare-row">
              <div className="compare-cell"><span className="rowtitle">Streaming responses</span></div>
              <div className="compare-cell yes"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12l5 5 9-9"/></svg> Native WebSocket</div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg></div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg></div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg></div>
              <div className="compare-cell no"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M6 18l12-12"/></svg></div>
            </div>
          </div>
        </div>
      </section>

      {/* TECH */}
      <section id="tech">
        <div className="sec-head">
          <div>
            <div className="sec-label"><b>007</b> · Stack</div>
            <h2 className="sec-title reveal">Built on <em>proven primitives.</em></h2>
          </div>
          <p className="sec-desc reveal reveal-d1">No reinvention for its own sake. Every layer is a battle-tested open-source component, assembled specifically for GPU compute coordination.</p>
        </div>
        <div className="grid tech">
          <div className="tech-item reveal">
            <div className="tech-hd"><h4>libp2p Transport</h4><div className="tech-ord">T · 01</div></div>
            <p>TCP + QUIC dual-stack with Noise authenticated encryption and Yamux stream multiplexing. AutoNAT traversal means any home node can participate without port-forwarding.</p>
            <div className="tech-tags"><span className="tech-tag">TCP</span><span className="tech-tag">QUIC</span><span className="tech-tag">Noise</span><span className="tech-tag">Yamux</span></div>
          </div>
          <div className="tech-item reveal reveal-d1">
            <div className="tech-hd"><h4>Kademlia DHT + Gossipsub</h4><div className="tech-ord">T · 02</div></div>
            <p>Kademlia DHT for peer routing and mDNS for local discovery. Five gossipsub topics carry inference requests, bids, announcements and Merkle root broadcasts.</p>
            <div className="tech-tags"><span className="tech-tag">Kademlia</span><span className="tech-tag">mDNS</span><span className="tech-tag">Gossipsub</span><span className="tech-tag">5 topics</span></div>
          </div>
          <div className="tech-item reveal reveal-d2">
            <div className="tech-hd"><h4>Ed25519 Identity</h4><div className="tech-ord">T · 03</div></div>
            <p>Every node is an Ed25519 keypair. The libp2p PeerId is derived from pk_N — no separate account or wallet needed. 128-bit security per RFC 8032.</p>
            <div className="tech-tags"><span className="tech-tag">Ed25519</span><span className="tech-tag">RFC 8032</span><span className="tech-tag">128-bit security</span></div>
          </div>
          <div className="tech-item reveal">
            <div className="tech-hd"><h4>ProofOfInference</h4><div className="tech-ord">T · 04</div></div>
            <p>A signed execution receipt bound to (model, tokens, latency, H_in, H_out). Verifiable offline with only the node&apos;s public key. Constant-time O(1) verification, no network call.</p>
            <div className="tech-tags"><span className="tech-tag">Ed25519 σ</span><span className="tech-tag">SHA-256 H_in/H_out</span><span className="tech-tag">Offline</span></div>
          </div>
          <div className="tech-item reveal reveal-d1">
            <div className="tech-hd"><h4>AES-256-GCM Sessions</h4><div className="tech-ord">T · 05</div></div>
            <p>Session context encrypted under a client-held key K derived from X25519 DH. The GPU node never sees K — only the current-turn context window, zeroed from RAM after inference.</p>
            <div className="tech-tags"><span className="tech-tag">AES-256-GCM</span><span className="tech-tag">X25519</span><span className="tech-tag">96-bit nonce</span></div>
          </div>
          <div className="tech-item reveal reveal-d2">
            <div className="tech-hd"><h4>Settlement Adapters</h4><div className="tech-ord">T · 06</div></div>
            <p>Five adapters behind one interface: free, signed-receipt, off-chain payment channel, Sui (Phase D), EVM (Phase E). All selected by a single TOML key — same binary, zero code changes.</p>
            <div className="tech-tags"><span className="tech-tag">free</span><span className="tech-tag">receipt</span><span className="tech-tag">channel</span><span className="tech-tag">sui</span><span className="tech-tag">evm</span></div>
          </div>
        </div>
      </section>

      {/* HARDWARE */}
      <section id="hardware">
        <div className="sec-head">
          <div>
            <div className="sec-label"><b>008</b> · Fleet</div>
            <h2 className="sec-title reveal">The GPUs <em>behind the mesh.</em></h2>
          </div>
          <p className="sec-desc reveal reveal-d1">A live breakdown of the hardware running inference right now. Consumer cards dominate the network — by design.</p>
        </div>
        <div className="grid hw" id="hwGrid">
          <div className="hw-card reveal" style={{'--pct':'68%'} as React.CSSProperties}>
            <div className="hw-visual"><div className="hw-chip"><span className="dot"></span></div></div>
            <div className="hw-name">RTX 4090</div>
            <div className="hw-spec">24GB · 82.6 TFLOPS</div>
            <div className="hw-bar">
              <div className="hw-bar-lbl"><span>Network share</span><span>68%</span></div>
              <div className="hw-bar-track"><div className="hw-bar-fill"></div></div>
            </div>
          </div>
          <div className="hw-card reveal reveal-d1" style={{'--pct':'18%'} as React.CSSProperties}>
            <div className="hw-visual"><div className="hw-chip"><span className="dot"></span></div></div>
            <div className="hw-name">RTX 3090</div>
            <div className="hw-spec">24GB · 35.6 TFLOPS</div>
            <div className="hw-bar">
              <div className="hw-bar-lbl"><span>Network share</span><span>18%</span></div>
              <div className="hw-bar-track"><div className="hw-bar-fill"></div></div>
            </div>
          </div>
          <div className="hw-card reveal reveal-d2" style={{'--pct':'9%'} as React.CSSProperties}>
            <div className="hw-visual"><div className="hw-chip"><span className="dot"></span></div></div>
            <div className="hw-name">A100 · 80GB</div>
            <div className="hw-spec">80GB HBM2e · 312 TFLOPS</div>
            <div className="hw-bar">
              <div className="hw-bar-lbl"><span>Network share</span><span>9%</span></div>
              <div className="hw-bar-track"><div className="hw-bar-fill"></div></div>
            </div>
          </div>
          <div className="hw-card reveal reveal-d3" style={{'--pct':'5%'} as React.CSSProperties}>
            <div className="hw-visual"><div className="hw-chip"><span className="dot"></span></div></div>
            <div className="hw-name">Other</div>
            <div className="hw-spec">4080 · 4070 · M-series · more</div>
            <div className="hw-bar">
              <div className="hw-bar-lbl"><span>Network share</span><span>5%</span></div>
              <div className="hw-bar-track"><div className="hw-bar-fill"></div></div>
            </div>
          </div>
        </div>
      </section>

      {/* ROADMAP */}
      <section id="roadmap">
        <div className="sec-head">
          <div>
            <div className="sec-label"><b>009</b> · Timeline</div>
            <h2 className="sec-title reveal">From testnet <em>to full mesh.</em></h2>
          </div>
          <p className="sec-desc reveal reveal-d1">Four phases. Shipping cadence tied to node-count milestones, not marketing dates.</p>
        </div>
        <div className="grid road">
          <div className="phase active reveal" style={{'--p':'1'} as React.CSSProperties}>
            <span className="phase-tag">Live</span>
            <div className="phase-label">Phase C · April 2026</div>
            <div className="phase-name">Cryptographic Core</div>
            <ul className="phase-list">
              <li>Ed25519 identity + ProofOfInference</li>
              <li>Merkle reputation tree + gossip</li>
              <li>Free + signed-receipt settlement</li>
              <li>Local + IPFS + Walrus storage</li>
            </ul>
            <div className="phase-progress"></div>
          </div>
          <div className="phase reveal reveal-d1" style={{'--p':'0'} as React.CSSProperties}>
            <span className="phase-tag">Queued</span>
            <div className="phase-label">Phase D · H2 2026</div>
            <div className="phase-name">Sui Settlement</div>
            <ul className="phase-list">
              <li>Move escrow smart contract</li>
              <li>SuiSettlement adapter live</li>
              <li>On-chain proof verification</li>
              <li>Reputation anchoring on Sui</li>
            </ul>
            <div className="phase-progress"></div>
          </div>
          <div className="phase reveal reveal-d2" style={{'--p':'0'} as React.CSSProperties}>
            <span className="phase-tag">Queued</span>
            <div className="phase-label">Phase E · H1 2027</div>
            <div className="phase-name">EVM Settlement</div>
            <ul className="phase-list">
              <li>Solidity escrow contract · Base L2</li>
              <li>EvmSettlement adapter live</li>
              <li>Multi-chain settlement matrix</li>
              <li>TOML-selectable chains</li>
            </ul>
            <div className="phase-progress"></div>
          </div>
          <div className="phase reveal reveal-d3" style={{'--p':'0'} as React.CSSProperties}>
            <span className="phase-tag">Queued</span>
            <div className="phase-label">Phase F · H2 2027</div>
            <div className="phase-name">On-Chain Channels</div>
            <ul className="phase-list">
              <li>Payment channels — on-chain close</li>
              <li>50× gas amortisation at 100 req/session</li>
              <li>Full gossip protocol live</li>
              <li>Governance parameterisation</li>
            </ul>
            <div className="phase-progress"></div>
          </div>
        </div>
      </section>

      {/* FINAL CTA */}
      <div className="final" id="cta">
        <div className="final-inner">
          <div className="final-grid-bg"></div>
          <div className="final-radial"></div>
          <div className="final-eyebrow">— 010 · Start Here</div>
          <h2>Be first on the network.<br/><em>Join the waitlist.</em></h2>
          <p>No credit card. No token. No permission. Phase C is live — Ed25519 identity, Merkle reputation and signed-receipt settlement work today, with zero blockchain required.</p>
          <div className="final-ctas">
            <button className="btn btn-primary" onClick={() => setShowWaitlist(true)}><span>Join Waitlist</span><span className="arrow">↗</span></button>
            <a className="btn btn-ghost" href="/PinaivuAI_Whitepaper.pdf" target="_blank" rel="noopener noreferrer"><span>Read Whitepaper</span></a>
          </div>
        </div>
      </div>

      {/* FOOTER */}
      <footer>
        <div className="brand">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
            <circle cx="12" cy="12" r="10"/><circle cx="12" cy="12" r="5"/>
            <circle cx="12" cy="12" r="1.5" fill="currentColor"/>
          </svg>
          Pinaivu AI
        </div>
        <div className="meta">The Inference Network · Est. 2026 · Licensed MIT</div>
        <ul className="footer-links">
          <li><a href="#">Docs</a></li>
          <li><a href="#">GitHub</a></li>
          <li><a href="#">Discord</a></li>
          <li><a href="#">Twitter</a></li>
          <li><a href="/PinaivuAI_Whitepaper.pdf" target="_blank" rel="noopener noreferrer">Whitepaper</a></li>
        </ul>
      </footer>
    </div>
  );
}
