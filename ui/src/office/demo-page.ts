// Dev page for the office (office-demo.html): mounts the scene and plays the
// recorded demo in a loop. Never imported by the app.

import { playDemo } from "./demo";
import { mountOfficeScene } from "./scene";

const container = document.getElementById("office") as HTMLElement;
const status = document.getElementById("status") as HTMLElement;
const scene = await mountOfficeScene(container);
scene.snapshot({ v: 1, sessions: [] });

// ?speed=4 plays faster (also handy for screenshots).
const initialSpeed = Number(new URLSearchParams(location.search).get("speed")) || 1;
let stop = playDemo((event) => scene.event(event), { loop: true, speed: initialSpeed });
let visible = true;

document.getElementById("toggle")?.addEventListener("click", (e) => {
  visible = !visible;
  scene.setVisible(visible);
  (e.target as HTMLButtonElement).textContent = visible ? "hide" : "show";
  status.textContent = visible ? "" : "hidden: no frames";
});
document.getElementById("wide")?.addEventListener("click", () => {
  container.style.width = container.style.width === "640px" ? "320px" : "640px";
});
document.getElementById("speed")?.addEventListener("change", (e) => {
  stop();
  stop = playDemo((event) => scene.event(event), { loop: true, speed: Number((e.target as HTMLSelectElement).value) });
});
