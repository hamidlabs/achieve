// App event cues. Break and warning cues are played by the Rust backend through
// the system audio: WebKitGTK (the Tauri webview on Linux) does not reliably
// output in-page audio (neither <audio> nor Web Audio), so anything played in
// the webview is silent. Task completion stays a tiny synthesized Web Audio tick
// (no asset supplied for it, and it's non-essential). Everything respects the
// shared mute flag.

import { invoke } from "@tauri-apps/api/core";

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

/** Ask the Rust backend to play a bundled cue, honoring mute. */
function playCue(name: "pre_break" | "stop_break" | "warning"): void {
  if (muted) return;
  void invoke("play_sound", { name }).catch(() => {
    /* backend unavailable (dev/browser): ignore */
  });
}

/** Push the mute flag to the backend, which plays the untracked cue on its own
 *  timer and so can't read localStorage. */
function syncMuted(): void {
  void invoke("set_sound_muted", { muted }).catch(() => {
    /* backend unavailable (dev/browser): ignore */
  });
}

/** Wire up mute state and unlock the Web Audio context (used by taskDone) on the
 *  first user interaction; some webviews start it suspended until a gesture. */
export function initSound(): void {
  if (typeof window === "undefined") return;
  try {
    muted = localStorage.getItem("achieve.sound") === "off";
  } catch {
    /* private mode / no storage */
  }
  syncMuted();
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
  syncMuted();
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
  playCue("pre_break");
}

/** Rested, back to work: the break has finished. */
export function breakOver(): void {
  playCue("stop_break");
}

// The "nothing is tracking" cue is NOT played here. It belongs to a timer, not
// to a render: the engine (src-tauri/src/engine.rs) plays it on a backing-off
// cadence for as long as the untracked stretch lasts, whether the window is
// open or hidden. Playing it on mount made it fire on every re-render and never
// once the window was already up.

/** Subtle, satisfying tick for completing a task. */
export function taskDone(): void {
  play([
    { f: 880, t: 0, d: 0.16, g: 0.09 }, // A5
    { f: 1318.5, t: 0.07, d: 0.32, g: 0.1 }, // E6
  ]);
}
