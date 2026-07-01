// App event cues. Break and warning cues use real audio assets (bundled via
// Vite, so their URLs are hashed and resolved at build time). Task completion
// stays a tiny synthesized tick (no asset supplied for it) via the Web Audio
// API. Everything respects the shared mute flag and unlocks on first gesture.

import preBreakUrl from "../assets/sounds/on_pre_break.wav";
import stopBreakUrl from "../assets/sounds/on_stop_break.wav";
import warningUrl from "../assets/sounds/warning.mp3";

let ctx: AudioContext | null = null;
let muted = false;

function context(): AudioContext | null {
  if (typeof window === "undefined") return null;
  try {
    const Ctor = window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
    ctx ??= new Ctor();
    if (ctx.state === "suspended") void ctx.resume();
    return ctx;
  } catch {
    return null;
  }
}

// File cues are decoded into the AudioContext and played as buffers, NOT via
// <audio> elements. WebKitGTK (the Tauri webview on Linux) blocks
// HTMLAudioElement.play() outside a direct click handler, so timer/effect-driven
// cues get silently dropped. Web Audio playback is exempt once the context has
// been resumed by a gesture, which is exactly what initSound() arranges.
const buffers = new Map<string, AudioBuffer>();
async function decode(url: string): Promise<void> {
  const c = context();
  if (!c || buffers.has(url)) return;
  try {
    const res = await fetch(url);
    const data = await res.arrayBuffer();
    buffers.set(url, await c.decodeAudioData(data));
  } catch {
    /* asset missing / decode failure: leave uncached, playFile no-ops */
  }
}

/** Play a decoded cue from its start, honoring mute. Decodes on demand if the
 *  buffer wasn't primed yet (first play may be a beat late, then it's instant). */
function playFile(url: string, volume = 1): void {
  if (muted) return;
  const c = context();
  if (!c) return;
  const buf = buffers.get(url);
  if (!buf) {
    void decode(url).then(() => {
      if (!muted && buffers.has(url)) playFile(url, volume);
    });
    return;
  }
  const src = c.createBufferSource();
  src.buffer = buf;
  const g = c.createGain();
  g.gain.value = volume;
  src.connect(g);
  g.connect(c.destination);
  src.start();
}

/** Wire up mute state and unlock audio on the first user interaction (WebKitGTK
 *  starts the AudioContext suspended until a gesture). Also decodes the file
 *  cues up front so the first real cue plays without delay. */
export function initSound(): void {
  if (typeof window === "undefined") return;
  try {
    muted = localStorage.getItem("achieve.sound") === "off";
  } catch {
    /* private mode / no storage */
  }
  const unlock = () => {
    context();
    for (const url of [preBreakUrl, stopBreakUrl, warningUrl]) void decode(url);
  };
  window.addEventListener("pointerdown", unlock, { once: true, passive: true });
  window.addEventListener("keydown", unlock, { once: true });
}

export function isMuted(): boolean {
  return muted;
}
export function setMuted(v: boolean): void {
  muted = v;
  try {
    localStorage.setItem("achieve.sound", v ? "off" : "on");
  } catch {
    /* ignore */
  }
  if (!v) context(); // resume/unlock when turning sound back on
}

type Note = { f: number; t: number; d: number; g?: number; type?: OscillatorType };

function play(notes: Note[]): void {
  if (muted) return;
  const c = context();
  if (!c) return;
  const now = c.currentTime;

  const master = c.createGain();
  master.gain.value = 0.9;
  const lp = c.createBiquadFilter();
  lp.type = "lowpass";
  lp.frequency.value = 3000;
  master.connect(lp);
  lp.connect(c.destination);

  for (const n of notes) {
    const o = c.createOscillator();
    const g = c.createGain();
    o.type = n.type ?? "sine";
    o.frequency.value = n.f;
    o.connect(g);
    g.connect(master);
    const t0 = now + n.t;
    const peak = n.g ?? 0.16;
    g.gain.setValueAtTime(0.0001, t0);
    g.gain.exponentialRampToValueAtTime(peak, t0 + 0.012);
    g.gain.exponentialRampToValueAtTime(0.0001, t0 + n.d);
    o.start(t0);
    o.stop(t0 + n.d + 0.05);
  }
}

/** Time to step away and rest: the break prompt has appeared. */
export function breakStart(): void {
  playFile(preBreakUrl);
}

/** Rested, back to work: the break has finished. */
export function breakOver(): void {
  playFile(stopBreakUrl);
}

/** Nudge when no task is being tracked. */
export function noTaskWarning(): void {
  playFile(warningUrl);
}

/** Subtle, satisfying tick for completing a task. */
export function taskDone(): void {
  play([
    { f: 880, t: 0, d: 0.16, g: 0.09 }, // A5
    { f: 1318.5, t: 0.07, d: 0.32, g: 0.1 }, // E6
  ]);
}
