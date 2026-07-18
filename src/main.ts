import { mount } from "svelte";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./app.css";
import App from "./App.svelte";
import VeilView from "./lib/views/VeilView.svelte";

// The app runs in two windows: the main hub ("main") and a passive full-screen
// dimming overlay ("veil") that covers the OTHER monitor during a rest break, so
// you can't just glance at the second screen and keep working. The veil renders
// a stripped-down view (no controls), chosen here by the window's label.
function windowLabel(): string {
  try {
    return getCurrentWindow().label;
  } catch {
    return "main";
  }
}

const target = document.getElementById("app")!;
const app =
  windowLabel() === "veil"
    ? mount(VeilView, { target })
    : mount(App, { target });

export default app;
