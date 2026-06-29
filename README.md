<div align="center">

<img src=".github/logo.png" alt="Achieve" width="116" height="116" />

# Achieve

**A proactive desktop day-companion that plans your day, nudges you through it, and tracks where your time actually goes — automatically.**

[![Tauri](https://img.shields.io/badge/Tauri-2-FFC131?logo=tauri&logoColor=black)](https://tauri.app)
[![Svelte](https://img.shields.io/badge/Svelte-5-FF3E00?logo=svelte&logoColor=white)](https://svelte.dev)
[![Rust](https://img.shields.io/badge/Rust-backend-000000?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![SQLite](https://img.shields.io/badge/SQLite-append--only%20ledger-003B57?logo=sqlite&logoColor=white)](https://sqlite.org)
[![Platform](https://img.shields.io/badge/platform-Linux%20%C2%B7%20Wayland%20%C2%B7%20niri-1793D1?logo=linux&logoColor=white)](#platform)
[![License: MIT](https://img.shields.io/badge/license-MIT-3FB950.svg)](LICENSE)

</div>

---

Ordinary time trackers fail at one thing: you have to remember to open and feed
them. **Achieve takes the initiative instead.** It surfaces a calm popup to plan
your day, nudges you when you drift, captures where the time goes with near-zero
effort, and reconciles what you intended against what actually happened — so you
finish the day knowing it was spent on what mattered.

## Why

You can spend a whole day busy and achieve nothing. The fix isn't another tool to
maintain; it's software that takes initiative. Achieve plans the day _with_ you,
nudges _through_ it, and records the truth in the background, then shows you the
gap between intention and reality.

## Features

- **🗓 Proactive planning & nudges** — surfaces itself when you're active but not
  tracking anything, and stays quiet when you're idle or done for the day.
- **⏱ Automatic, scientific tracking** — every focused app while a task runs is
  attributed to that task from the compositor's focus stream. No timers to start.
- **🧠 Pause-at-estimate** — when a task reaches its estimate the clock _stops_ and
  waits for your call (+15m / +30m / done), so an unanswered popup can never bleed
  phantom hours.
- **😴 Idle & suspend aware** — an idle watcher and a sleep guard cap open segments
  the moment you step away or the machine suspends, so overnight never accrues.
- **☕ Rest breaks (ultradian)** — a gentle break timer after a work stretch; while
  a break runs the UI locks to the break window so nothing else pulls you back in.
- **📊 Browsable history** — a Day / Week / Month dashboard with a focus-vs-untracked
  activity chart and Category / Application / Task breakdowns you can page through.
- **♻️ Append-only ledger** — completion isn't terminal: reopen a finished task and
  it simply accrues more time; totals and history stay intact.
- **🔁 Recurring tasks & streaks** — daily tasks roll forward automatically.
- **🔒 Private by design** — everything is a local SQLite file; the popup can show
  counts only ("3 pending") so it's safe on a shared screen.

## How it works — triangulated tracking

Achieve combines three signals so the picture is both effortless and honest:

1. **Auto ground-truth.** A [niri](https://github.com/YaLTeR/niri) focus
   event-stream stamps each app/window span with the currently-active task.
2. **Light self-report.** You pick the active task and can pause with a reason —
   a single tap, never a form.
3. **Reconciliation & guards.** An idle watcher (swayidle) and a logind sleep
   guard cap segments so away/asleep time is never counted, and the
   pause-at-estimate rule stops the clock at the boundary you set.

Time is stored as **append-only `segments`** tied to a task; a task's total is the
sum of its segments, independent of status. Focus that isn't attributed to any task
becomes the "untracked" (distraction) bucket on the dashboard.

## The two surfaces

| Tasks hub | History dashboard |
| --- | --- |
| A day summary (tracked / done / buffer), a circular ring timer for the active task, and a tabbed **Today / Upcoming / Done** list with category and tracked-vs-estimate badges. | Day / Week / Month reports with a stacked focus-vs-untracked area chart and ranked Category / Application / Task breakdowns. |

## Tech stack

- **[Tauri v2](https://tauri.app)** — native shell, tray, window management
- **[Svelte 5](https://svelte.dev) + [Tailwind v4](https://tailwindcss.com)** — a light, native-feeling UI
- **Rust** — engine loop, SQLite (`rusqlite`), niri IPC, idle/suspend guards (D-Bus / zbus)
- **SQLite** — the append-only time ledger

The backend follows a clean separation: a pure engine + a thin DB layer + a thin
UI, so the scheduling and accounting logic is headless-testable.

## Getting started

> Achieve is currently built and tested on **Linux / Wayland with the niri
> compositor** (see [Platform](#platform)).

```sh
# System deps (Fedora example)
sudo dnf install -y webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel \
  librsvg2-devel openssl-devel patchelf

# Runtime deps used for tracking
sudo dnf install -y niri swayidle

npm install
npm run tauri dev
```

On first run a managed window-rule is added to `~/.config/niri/config.kdl`
(backed up first) so the popup floats, centers, and stays chrome-free.

## Build a release (AppImage)

```sh
APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=1 npm run tauri build
```

The self-contained AppImage lands in
`src-tauri/target/release/bundle/appimage/`. The two env vars are required on
hosts without FUSE.

## Data & privacy

- All data is local: `~/.local/share/com.hamidlabs.achieve/achieve.db` (SQLite, WAL).
- Nothing leaves your machine — there is no network or telemetry.
- The popup can render counts only, keeping task contents off a shared screen.

## Platform

Achieve leans on Wayland/niri primitives for its core value (focus event-stream
attribution, idle detection, window centering and float rules) and on logind for
suspend awareness. Today that means **Linux + niri + swayidle**. The engine, ledger,
and UI are portable; the compositor integration is the Linux-specific part.

## Project status

A working personal experiment, actively iterated. Foundations, automatic tracking,
the two-window redesign, rest breaks, and the browsable history dashboard are
implemented and in daily use.

## License

[MIT](LICENSE) © Hamid
