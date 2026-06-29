// Small formatting helpers shared across views.

export function fmtMin(min: number): string {
  const m = Math.max(0, Math.round(min));
  const h = Math.floor(m / 60);
  const r = m % 60;
  if (h > 0) return r > 0 ? `${h}h ${r}m` : `${h}h`;
  return `${r}m`;
}

export function fmtClock(minFromMidnight: number): string {
  const h = Math.floor(minFromMidnight / 60) % 24;
  const m = minFromMidnight % 60;
  const ampm = h < 12 ? "am" : "pm";
  const h12 = h % 12 === 0 ? 12 : h % 12;
  return `${h12}:${String(m).padStart(2, "0")}${ampm}`;
}

const CAT_FALLBACK = "#9aa0aa";
export function catColor(c: string | null | undefined): string {
  return c || CAT_FALLBACK;
}

// Wayland app_ids are messy ("org.mozilla.firefox", "google-chrome", "Code").
// Map the common ones to clean names; otherwise prettify the last segment.
const APP_NAMES: Record<string, string> = {
  firefox: "Firefox",
  "org.mozilla.firefox": "Firefox",
  "firefox-esr": "Firefox",
  zen: "Zen Browser",
  "google-chrome": "Chrome",
  "google-chrome-stable": "Chrome",
  chromium: "Chromium",
  "brave-browser": "Brave",
  code: "VS Code",
  "code-oss": "VS Code",
  "code-url-handler": "VS Code",
  cursor: "Cursor",
  kitty: "Kitty",
  alacritty: "Alacritty",
  "org.wezfurlong.wezterm": "WezTerm",
  foot: "Foot",
  ghostty: "Ghostty",
  "com.mitchellh.ghostty": "Ghostty",
  slack: "Slack",
  "com.slack.Slack": "Slack",
  discord: "Discord",
  "org.telegram.desktop": "Telegram",
  "org.gnome.nautilus": "Files",
  "org.gnome.Nautilus": "Files",
  spotify: "Spotify",
  "com.obsproject.studio": "OBS",
  obsidian: "Obsidian",
  "md.obsidian.obsidian": "Obsidian",
  zoom: "Zoom",
  thunderbird: "Thunderbird",
};

export function appName(appId: string | null | undefined): string {
  if (!appId) return "Unknown";
  const key = appId.trim();
  if (APP_NAMES[key]) return APP_NAMES[key];
  if (APP_NAMES[key.toLowerCase()]) return APP_NAMES[key.toLowerCase()];
  const last = key.split(".").pop() ?? key;
  return last
    .replace(/[-_]+/g, " ")
    .replace(/\b\w/g, (m) => m.toUpperCase())
    .trim();
}

// Stable, pleasant color per app (light-theme palette), chosen by a hash so the
// same app always reads the same color across the dashboard.
const APP_PALETTE = [
  "#0a84ff", "#30b0a8", "#ff9f0a", "#bf5af2", "#ff375f",
  "#34c759", "#5e5ce6", "#ff9500", "#64d2ff", "#ac8e68",
];
export function appColor(appId: string | null | undefined): string {
  const s = appId ?? "";
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) >>> 0;
  return APP_PALETTE[h % APP_PALETTE.length];
}
