// Gentle synthesized chimes for app events (breaks, task completion). Uses the
// Web Audio API so there are no binary assets to bundle: each cue is a short
// bell-like blend of sine partials with a soft envelope, run through a lowpass
// for warmth. Designed to be calm and unobtrusive, never jarring.

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

/** Wire up mute state and unlock audio on the first user interaction (some
 *  webviews start the AudioContext suspended until a gesture). */
export function initSound(): void {
  if (typeof window === "undefined") return;
  try {
    muted = localStorage.getItem("achieve.sound") === "off";
  } catch {
    /* private mode / no storage */
  }
  const unlock = () => context();
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

/** Calm descending chime: time to step away and rest. */
export function breakStart(): void {
  play([
    { f: 659.25, t: 0, d: 0.95, g: 0.13 }, // E5
    { f: 493.88, t: 0.18, d: 1.15, g: 0.16 }, // B4
    { f: 987.77, t: 0.18, d: 1.0, g: 0.035 }, // soft octave shimmer
  ]);
}

/** Bright ascending arpeggio: rested, back to work. */
export function breakOver(): void {
  play([
    { f: 523.25, t: 0, d: 0.5, g: 0.12 }, // C5
    { f: 659.25, t: 0.12, d: 0.5, g: 0.12 }, // E5
    { f: 783.99, t: 0.24, d: 0.75, g: 0.15 }, // G5
  ]);
}

/** Subtle, satisfying tick for completing a task. */
export function taskDone(): void {
  play([
    { f: 880, t: 0, d: 0.16, g: 0.09 }, // A5
    { f: 1318.5, t: 0.07, d: 0.32, g: 0.1 }, // E6
  ]);
}
