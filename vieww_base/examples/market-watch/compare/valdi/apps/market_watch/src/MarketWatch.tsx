// Market Watch — the Valdi side of the Valdi-vs-Vieww comparison.
//
// Copied into a Valdi checkout by vieww's CI (examples/market-watch/ci/jobs).
// Same screen, same data generator, same tick rule and same schedule as
// vieww_base/examples/market-watch/src/lib.rs, so both frameworks draw the same
// pixels from the same numbers under the same load.
//
// Pacing: a continuous phase applies one change per drawn frame, chained on the
// renderer's `onNextDraw` (Valdi's platform draw callback), which is the
// closest Valdi has to Vieww's `before_frame`. A free-running 16 ms timer would
// measure the timer, not the framework.
//
// Prints one line per event, `__MW__ {json}`, through console.log: logcat tag
// `Valdi` on Android. That holds in a release build too — Valdi's Android
// DefaultLogger enables every level, so the App Store gate on console output
// in JavaScriptRuntime::runtimeOutputLog lets these through.

import { StatefulComponent, Component } from 'valdi_core/src/Component';
import { ElementRef } from 'valdi_core/src/ElementRef';
import { GeometricPathBuilder } from 'valdi_core/src/GeometricPath';
import { Style } from 'valdi_core/src/Style';
import { View, Label, ScrollViewInteractive } from 'valdi_tsx/src/NativeTemplateElements';

export interface MarketWatchContext {
  /** Skip the 60 s thermal pass. */
  noThermal?: boolean;
}

const ROW_H = 80;
const POINTS = 32;
const LIVE_ROWS = 3;
const SCROLL_STEP = 8;
const PHASE_SECONDS = 8;
const THERMAL_SECONDS = 60;
const UP = '#16A36A';
const DOWN = '#DC4048';
const PALETTE = ['#5D78F6', '#EF6C60', '#F6AA3C', '#40B496', '#A060E6'];

// ---------------------------------------------------------------- data

interface Quote {
  symbol: string;
  name: string;
  hue: string;
  hist: readonly number[]; // replaced, never mutated, on a tick
}

const price = (q: Quote) => q.hist[q.hist.length - 1];
const change = (q: Quote) => ((price(q) - q.hist[0]) / q.hist[0]) * 100;

/** The LCG both frameworks use, in exact 32-bit arithmetic. */
function lcg() {
  let s = 0x9e3779b9 >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return (s % 10000) / 10000;
  };
}

function makeQuotes(rows: number): Quote[] {
  const rnd = lcg();
  const L = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ';
  const out: Quote[] = new Array(rows);
  for (let i = 0; i < rows; i++) {
    let p = 20 + rnd() * 400;
    const hist: number[] = new Array(POINTS);
    for (let k = 0; k < POINTS; k++) hist[k] = p *= 1 + (rnd() - 0.48) * 0.04;
    let symbol = '';
    for (let k = 0; k < 4; k++) symbol += L[(i * 7 + k * 11 + Math.floor(i / 26)) % 26];
    out[i] = { symbol, name: `Company ${String(i).padStart(4, '0')} Holdings`, hue: PALETTE[i % 5], hist };
  }
  return out;
}

/** Same rule as Vieww's `tick_quote`: reverse the last move, 6% up or 5% down. */
function tickQuote(q: Quote, index: number): Quote {
  const h = q.hist;
  const n = h.length;
  const rose = h[n - 1] >= h[n - 2];
  const last = h[n - 1];
  const size = 1 + (index % 3) * 0.01;
  return { ...q, hist: [...h.slice(1), rose ? last * (0.95 / size) : last * (1.06 * size)] };
}

// ------------------------------------------- interned styles (bridge sends one id)

const card = new Style<View>({
  flexDirection: 'row',
  alignItems: 'center',
  backgroundColor: 'white',
  borderRadius: 14,
  boxShadow: '0 3 8 rgba(20, 26, 38, 0.12)',
  paddingLeft: 14,
  paddingRight: 14,
  paddingTop: 12,
  paddingBottom: 12,
  marginTop: 5,
  marginBottom: 5,
  height: 70,
});
const avatar = new Style<View>({ width: 40, height: 40, borderRadius: 20, alignItems: 'center', justifyContent: 'center' });
const pill = new Style<View>({ borderRadius: 8, paddingLeft: 8, paddingRight: 8, paddingTop: 3, paddingBottom: 3, marginTop: 4 });

function sparkPath(hist: readonly number[], w: number, h: number) {
  let lo = Infinity;
  let hi = -Infinity;
  for (const v of hist) {
    if (v < lo) lo = v;
    if (v > hi) hi = v;
  }
  const span = Math.max(hi - lo, 1e-3);
  const step = w / (hist.length - 1);
  const b = new GeometricPathBuilder(w, h);
  hist.forEach((v, i) => {
    const x = i * step;
    const y = h - ((v - lo) / span) * (h - 2) - 1;
    i === 0 ? b.moveTo(x, y) : b.lineTo(x, y);
  });
  return b.build();
}

// --------------------------------------------------------------- row
// A plain Component: when its view model is the same object as last time,
// Valdi skips onRender() entirely, so only ticked rows re-render.

class Row extends Component<{ quote: Quote; font: string }> {
  onRender() {
    const { quote: q, font } = this.viewModel;
    const ch = change(q);
    const tone = ch >= 0 ? UP : DOWN;
    <view style={card}>
      <view style={avatar} backgroundColor={q.hue}>
        <label value={q.symbol.slice(0, 2)} color='white' font={`${font} 14`} />
      </view>
      <view flexGrow={1} flexShrink={1} marginLeft={12} marginRight={12}>
        <label value={q.symbol} color='#141A26' font={`${font} 16`} />
        <label value={q.name} color='#707A8C' font={`${font} 12`} marginTop={2} numberOfLines={1} />
      </view>
      <shape width={120} height={36} path={sparkPath(q.hist, 120, 36)} strokeColor={tone} strokeWidth={1.6} strokeJoin='round' />
      <view alignItems='flex-end' width={120} marginLeft={12}>
        <label value={`$${price(q).toFixed(2)}`} color='#141A26' font={`${font} 16`} />
        <view style={pill} backgroundColor={tone}>
          <label value={`${ch >= 0 ? '+' : ''}${ch.toFixed(2)}%`} color='white' font={`${font} 12`} />
        </view>
      </view>
    </view>;
  }
}

class Header extends Component<{ total: number; rows: number; font: string }> {
  onRender() {
    const { total, rows, font } = this.viewModel;
    <view background='linear-gradient(#3A5CF6, #8C48E6)' borderRadius={20} padding={22}>
      <label value='Portfolio value' color='rgba(255,255,255,0.75)' font={`${font} 13`} />
      <label value={`$${total.toFixed(2)}`} color='white' font={`${font} 32`} marginTop={6} />
      <label value={`${rows} live tickers · virtualised`} color='rgba(255,255,255,0.75)' font={`${font} 12`} marginTop={6} />
    </view>;
  }
}

// ---------------------------------------------------------- schedule

type Load = 'load' | 'idle' | 'tick' | 'jump' | 'scroll' | 'live' | 'storm';
interface Phase {
  name: string;
  load: Load;
  rows?: number;
  seconds: number;
}

/** Mirrors `market_watch::schedule`. These names are the protocol. */
function schedule(thermal: boolean): Phase[] {
  const s: Phase[] = [];
  for (const rows of [2000, 20000]) {
    const k = `${rows / 1000}k`;
    s.push({ name: `load${k}`, load: 'load', rows, seconds: 2 });
    s.push({ name: `idle${k}`, load: 'idle', seconds: 2 });
    s.push({ name: `tick${k}`, load: 'tick', seconds: 1 });
    s.push({ name: `jump${k}`, load: 'jump', seconds: 1 });
    s.push({ name: `scroll${k}`, load: 'scroll', seconds: PHASE_SECONDS });
    s.push({ name: `live${k}`, load: 'live', seconds: PHASE_SECONDS });
    s.push({ name: `storm${k}`, load: 'storm', seconds: PHASE_SECONDS });
  }
  if (thermal) {
    s.push({ name: 'load2k-thermal', load: 'load', rows: 2000, seconds: 2 });
    s.push({ name: 'thermal', load: 'live', seconds: THERMAL_SECONDS });
  }
  return s;
}
const continuous = (l: Load) => l === 'scroll' || l === 'live' || l === 'storm';

function stats(v: number[]): string {
  if (!v.length) return '[0,0,0,0]';
  const s = [...v].sort((a, b) => a - b);
  const at = (p: number) => s[Math.round((s.length - 1) * p)];
  return `[${[at(0.5), at(0.95), at(0.99), s[s.length - 1]].map(x => x.toFixed(3)).join(',')}]`;
}

// ------------------------------------------------------------ screen

interface State {
  quotes: Quote[];
  total: number;
}

export class MarketWatch extends StatefulComponent<{}, State, MarketWatchContext> {
  state: State = { quotes: [], total: 0 };

  private scrollRef = new ElementRef<ScrollViewInteractive>();
  private offset = 0;
  private ticks = 0;
  private t0 = performance.now();
  private phases: Phase[] = [];
  private phase = -1;
  private viewport = 600;
  // Per-phase samples
  private renderMs: number[] = [];
  private drawGaps: number[] = [];
  private lastDraw = 0;
  private done = false;

  private emit(obj: object) {
    const line = JSON.stringify({ fw: 'valdi', ...obj });
    console.log(`__MW__ ${line}`);
  }
  private t() {
    return Math.round((performance.now() - this.t0) * 10) / 10;
  }

  onCreate() {
    this.phases = schedule(!this.context?.noThermal);
    this.emit({ ev: 'start', path: 'native-views', phases: this.phases.length });
    // Same 1.5 s settle as the Vieww app, then the clock starts.
    this.setTimeoutDisposable(() => this.advance(), 1500);
  }

  /** The clock: enter the next phase, schedule the one after. */
  private advance() {
    this.flush();
    this.phase++;
    if (this.phase >= this.phases.length) {
      this.done = true;
      this.emit({ ev: 'done', t: this.t() });
      return;
    }
    const p = this.phases[this.phase];
    this.emit({ ev: 'phase', phase: p.name, t: this.t() });
    this.lastDraw = 0;
    this.enter(p);
    if (continuous(p.load)) this.pump(this.phase);
    this.setTimeoutDisposable(() => this.advance(), p.seconds * 1000);
  }

  private flush() {
    if (this.phase < 0 || this.phase >= this.phases.length) return;
    const p = this.phases[this.phase];
    this.emit({
      ev: 'stats',
      phase: p.name,
      frames: this.drawGaps.length + (this.lastDraw ? 1 : 0),
      // JS-thread cost of a change: setState through render and diff, to the
      // point the native tree has its updates. Layout and raster follow on
      // the platform's threads, and the device runner reads those from HWUI.
      render_ms: JSON.parse(stats(this.renderMs)),
      interval_ms: JSON.parse(stats(this.drawGaps)),
      t: this.t(),
    });
    this.renderMs = [];
    this.drawGaps = [];
  }

  /** One frame of a continuous phase, chained on the platform draw callback. */
  private pump(phase: number) {
    if (this.done || phase !== this.phase) return;
    this.frame(this.phases[phase]);
    this.renderer.onNextDraw(hookMs => {
      if (this.lastDraw) this.drawGaps.push(hookMs - this.lastDraw);
      this.lastDraw = hookMs;
      this.pump(phase);
    });
  }

  private timed(apply: () => void) {
    const t = performance.now();
    apply();
    this.renderMs.push(performance.now() - t);
  }

  private visible(): [number, number] {
    const first = Math.max(0, Math.floor(this.offset / ROW_H));
    return [first, Math.min(first + Math.ceil(this.viewport / ROW_H) + 1, this.state.quotes.length)];
  }

  private tickRows(indices: number[]) {
    const quotes = this.state.quotes.slice(); // new array, same row objects…
    const n = quotes.length;
    if (!n) return;
    let delta = 0;
    for (const raw of indices) {
      const i = raw % n;
      const before = price(quotes[i]);
      quotes[i] = tickQuote(quotes[i], i); // …except these: a new view model, a re-render
      delta += price(quotes[i]) - before;
    }
    this.ticks++;
    this.setState({ quotes, total: this.state.total + delta });
  }

  private tickLive() {
    const [first] = this.visible();
    const k = this.ticks * LIVE_ROWS;
    this.tickRows([0, 1, 2].map(j => first + 1 + ((k + j * 3) % 9)));
  }

  private scrollBy(px: number) {
    const max = Math.max(0, this.state.quotes.length * ROW_H - 1);
    this.offset = Math.min(max, Math.max(0, this.offset + px));
    this.scrollRef.setAttribute('contentOffsetY', this.offset);
  }

  private enter(p: Phase) {
    switch (p.load) {
      case 'load':
        this.timed(() => {
          const quotes = makeQuotes(p.rows!);
          this.offset = 0;
          this.ticks = 0;
          this.setState({ quotes, total: quotes.reduce((s, q) => s + price(q), 0) });
          this.scrollRef.setAttribute('contentOffsetY', 0);
        });
        this.renderer.onNextDraw(hookMs => (this.lastDraw = hookMs));
        break;
      case 'tick':
        this.timed(() => this.tickLive());
        break;
      case 'jump':
        this.timed(() => this.scrollBy(4000));
        break;
      default:
        break;
    }
  }

  private frame(p: Phase) {
    this.timed(() => {
      if (p.load === 'scroll') this.scrollBy(SCROLL_STEP);
      if (p.load === 'live') {
        this.scrollBy(SCROLL_STEP);
        this.tickLive();
      }
      if (p.load === 'storm') {
        const [a, b] = this.visible();
        const rows: number[] = [];
        for (let i = a; i < b; i++) rows.push(i);
        this.tickRows(rows);
      }
    });
  }

  onRender() {
    const font = 'system';
    const { quotes, total } = this.state;
    <view backgroundColor='#F3F5F9' padding={20} width='100%' height='100%'>
      <Header total={total} rows={quotes.length} font={font} />
      <scroll
        ref={this.scrollRef}
        marginTop={16}
        flexGrow={1}
        onLayout={frame => (this.viewport = frame.height)}
      >
        {quotes.forEach((q, i) => {
          <Row key={String(i)} quote={q} font={font} />;
        })}
      </scroll>
    </view>;
  }
}
