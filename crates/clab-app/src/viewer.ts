import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { LineMaterial } from "three/examples/jsm/lines/LineMaterial.js";
import { LineSegments2 } from "three/examples/jsm/lines/LineSegments2.js";
import { LineSegmentsGeometry } from "three/examples/jsm/lines/LineSegmentsGeometry.js";
import { colorFor } from "./colors";
import { element } from "./sections";

type Vec3 = [number, number, number];

/** One time step of the scene, as `clab inspect --json` returns it. */
export interface ViewFrame {
  time: number;
  bodies: { label: string; origin: Vec3; segments: [Vec3, Vec3][] }[];
  joints: { label: string; i: Vec3; j: Vec3 }[];
}

export interface Viewer {
  destroy(): void;
}

/** The terminal viewer's playback rules, so both viewers feel the same. */
const FPS = { min: 1, initial: 10, floor: 100 };
/** The speed slider is logarithmic, so slow speeds keep fine control. */

const ICONS = {
  play: '<svg viewBox="0 0 12 12" width="12" height="12" fill="currentColor"><path d="M2 1l9 5-9 5z"/></svg>',
  pause: '<svg viewBox="0 0 12 12" width="12" height="12" fill="currentColor"><path d="M2 1h3v10H2zM7 1h3v10H7z"/></svg>',
};
const LOOPS = ["restart", "bounce", "once"] as const;
const PADDING = 1.1;
/** A click selects the body drawn within this many pixels. */
const PICK_PX = 8;
/** Bright enough to stand out from the body colors on light and dark backgrounds. */
const HIGHLIGHT = "#ffd400";
/** The camera sits this many scene radii from its target; the scene always fits in front of it. */
const REACH = 4;

/**
 * Directions from the target to the camera; the solver's z axis is up. The XY view is
 * tilted a hair off the pole, where "up" would be undefined.
 */
const VIEWS = [
  { key: "1", name: "XY", direction: [0, -0.001, 1] },
  { key: "2", name: "XZ", direction: [0, -1, 0] },
  { key: "3", name: "YZ", direction: [1, 0, 0] },
  { key: "4", name: "Iso", direction: [-1, -1, 1] },
] as const;

const HINT = "Space play · ←/→ step · +/- speed · l loop · v labels · f/F fit (this step/all) · 1–4 view · drag orbit · right-drag pan · wheel zoom";

/** Every position a frame draws. */
function* positions({ bodies, joints }: ViewFrame): Generator<Vec3> {
  for (const body of bodies) {
    yield body.origin;
    for (const segment of body.segments) yield* segment;
  }
  for (const joint of joints) yield* [joint.i, joint.j];
}

function boundingSphere(frames: ViewFrame[]): THREE.Sphere {
  const box = new THREE.Box3();
  for (const frame of frames) for (const [x, y, z] of positions(frame)) box.expandByPoint(new THREE.Vector3(x, y, z));
  const sphere = box.getBoundingSphere(new THREE.Sphere());
  sphere.radius = Math.max(sphere.radius, 1e-6);
  return sphere;
}

function button(text: string, title: string, onClick: () => void) {
  const node = element("button", text);
  node.type = "button";
  node.title = title;
  node.addEventListener("click", onClick);
  return node;
}

/** Segments with one color each, whose end points are rewritten with `write`. */
function segmentLines(colors: THREE.Color[]) {
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(colors.length * 6), 3));
  const rgb = colors.flatMap((color) => [...color.toArray(), ...color.toArray()]);
  geometry.setAttribute("color", new THREE.BufferAttribute(new Float32Array(rgb), 3));
  const lines = new THREE.LineSegments(geometry, new THREE.LineBasicMaterial({ vertexColors: true }));
  lines.frustumCulled = false;
  return lines;
}

function write({ geometry }: THREE.Object3D & { geometry: THREE.BufferGeometry }, points: Vec3[]) {
  const attribute = geometry.getAttribute("position") as THREE.BufferAttribute;
  for (let index = 0; index < points.length; index++) attribute.setXYZ(index, ...points[index]);
  attribute.needsUpdate = true;
}

/** Draws `frames` into `parent` with playback controls and mouse orbit, pan and zoom. */
export function createViewer(parent: HTMLElement, frames: ViewFrame[], onSelect?: (body: number | null) => void): Viewer {
  const last = frames.length - 1;
  /** The fastest playback shows the whole run in a second, but never less than 100 fps. */
  const maxFps = Math.max(FPS.floor, frames.length);
  const fpsAt = (position: number) => Math.round(FPS.min * (maxFps / FPS.min) ** (position / 100));
  const positionOf = (value: number) => (100 * Math.log(value / FPS.min)) / Math.log(maxFps / FPS.min);
  // Figure spaces keep every field at a constant width, so nothing beside the text moves.
  const pad = (text: string | number, width: number) => String(text).padStart(width, "\u2007");
  const digits = String(frames.length).length;
  const timeWidth = Math.max(...[frames[0], frames[last]].map(({ time }) => time.toFixed(3).length));
  const fpsDigits = String(maxFps).length;
  let index = 0;
  let playing = last > 0;
  let fps = FPS.initial;
  let loop = 0;
  let direction = 1;
  let showLabels = false;
  let viewRadius = 1;
  let dirty = true;

  const root = element("div", "", "viewer");
  root.tabIndex = 0;
  const stage = element("div", "", "viewer-stage");
  const timeline = document.createElement("input");
  timeline.type = "range";
  timeline.max = String(last);
  timeline.ariaLabel = "Time step";
  const status = element("span", "", "viewer-status");
  const fpsText = element("span", "", "viewer-status");
  const modeText = element("span", "", "viewer-status");
  const speedSlider = document.createElement("input");
  speedSlider.type = "range";
  speedSlider.className = "viewer-speed";
  speedSlider.max = "100";
  speedSlider.ariaLabel = "Frames per second";
  speedSlider.title = "Frames per second (+/- keys)";
  const playButton = button("", "Play/pause (Space)", () => togglePlay());
  const viewButtons = VIEWS.map(({ name, key }, view) => button(name, `${name} view (${key})`, () => look(view)));
  const playbackBar = element("div", "", "viewer-bar");
  playbackBar.append(playButton, timeline, status);
  const viewBar = element("div", "", "viewer-bar");
  const views = element("span", "", "viewer-views");
  views.append(...viewButtons);
  viewBar.append(fpsText, speedSlider, modeText, views);
  root.append(stage, playbackBar, viewBar, element("small", HINT, "viewer-hint"));
  parent.replaceChildren(root);

  // No multisampling: it is what makes software-rendered WebGL (WSL) slow, and thin lines gain little.
  const renderer = new THREE.WebGLRenderer({ antialias: false, alpha: true });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  stage.append(renderer.domElement);
  const camera = new THREE.OrthographicCamera(-1, 1, 1, -1);
  camera.up.set(0, 0, 1);
  const controls = new OrbitControls(camera, renderer.domElement);
  controls.addEventListener("change", () => {
    dirty = true;
    flush();
  });

  const scene = new THREE.Scene();
  const bodyLines = segmentLines(
    frames[0].bodies.flatMap(({ segments }, body) => segments.map(() => new THREE.Color(colorFor(body + 1)))),
  );
  // Body `b` owns segments `first[b]` up to `first[b + 1]` of `bodyLines`.
  const first = frames[0].bodies.reduce((starts, { segments }) => [...starts, starts[starts.length - 1] + segments.length], [0]);
  let selected = -1;
  function paintBodies() {
    const colors = bodyLines.geometry.getAttribute("color") as THREE.BufferAttribute;
    frames[0].bodies.forEach((_, body) => {
      const color = new THREE.Color(colorFor(body + 1));
      for (let vertex = 2 * first[body]; vertex < 2 * first[body + 1]; vertex++) colors.setXYZ(vertex, color.r, color.g, color.b);
    });
    colors.needsUpdate = true;
    dirty = true;
  }
  // WebGL lines are one pixel wide, so the selected body is redrawn as a thick bright line on top.
  const highlight = new LineSegments2(
    new LineSegmentsGeometry(),
    new LineMaterial({ color: HIGHLIGHT, linewidth: 5, depthTest: false }),
  );
  highlight.frustumCulled = false;
  highlight.renderOrder = 1;
  highlight.visible = false;
  // A segment seen end-on has no length to draw, so the ends are marked with dots.
  // The buffer fits the largest body; only the selected body's ends are in the draw range.
  const mostEnds = 2 * Math.max(...frames[0].bodies.map(({ segments }) => segments.length));
  const highlightEnds = new THREE.Points(
    new THREE.BufferGeometry().setAttribute("position", new THREE.BufferAttribute(new Float32Array(mostEnds * 3), 3)),
    new THREE.PointsMaterial({ color: HIGHLIGHT, size: 8, sizeAttenuation: false, depthTest: false }),
  );
  highlightEnds.frustumCulled = false;
  highlightEnds.renderOrder = 1;
  highlightEnds.visible = false;
  const jointLines = segmentLines(frames[0].joints.map(() => new THREE.Color()));
  const origins = new THREE.Points(
    new THREE.BufferGeometry().setAttribute("position", new THREE.BufferAttribute(new Float32Array(frames[0].bodies.length * 3), 3)),
    new THREE.PointsMaterial({ size: 6, sizeAttenuation: false }),
  );
  // The page's colors follow the system theme, so the scene's neutral colors are re-read when it changes.
  const dark = window.matchMedia("(prefers-color-scheme: dark)");
  function applyTheme() {
    const style = getComputedStyle(root);
    (origins.material as THREE.PointsMaterial).color.set(style.color);
    const rgb = new THREE.Color(style.getPropertyValue("--muted").trim() || "#888").toArray();
    const colors = jointLines.geometry.getAttribute("color") as THREE.BufferAttribute;
    for (let i = 0; i < colors.count; i++) colors.setXYZ(i, rgb[0], rgb[1], rgb[2]);
    colors.needsUpdate = true;
    paintBodies();
  }
  dark.addEventListener("change", applyTheme);
  applyTheme();
  origins.frustumCulled = false;
  scene.add(bodyLines, jointLines, origins, highlight, highlightEnds);

  // The axes live in their own scene, drawn in a corner with the main camera's orientation.
  const TRIAD_SIZE = 90;
  const triadScene = new THREE.Scene().add(new THREE.AxesHelper(1));
  const triadCamera = new THREE.OrthographicCamera(-1.3, 1.3, 1.3, -1.3, 0.1, 10);
  triadCamera.up.copy(camera.up);
  renderer.autoClear = false;
  const AXES = [
    { name: "x", tip: new THREE.Vector3(1.15, 0, 0), color: "#e03131" },
    { name: "y", tip: new THREE.Vector3(0, 1.15, 0), color: "#2f9e44" },
    { name: "z", tip: new THREE.Vector3(0, 0, 1.15), color: "#1c7ed6" },
  ].map((axis) => {
    const node = element("span", axis.name, "viewer-axis");
    node.style.color = axis.color;
    stage.append(node);
    return { ...axis, node };
  });

  const labelLayer = element("div", "", "viewer-labels");
  stage.append(labelLayer);
  const labels = [
    ...frames[0].bodies.map((body, i) => ({ text: body.label, at: (frame: ViewFrame): Vec3 => frame.bodies[i].origin })),
    ...frames[0].joints.map((joint, i) => ({
      text: joint.label,
      at: ({ joints }: ViewFrame): Vec3 => joints[i].i.map((n, axis) => (n + joints[i].j[axis]) / 2) as Vec3,
    })),
  ].map((label) => ({ ...label, node: labelLayer.appendChild(element("span", label.text)) }));

  const all = boundingSphere(frames);
  const target = (at: THREE.Vector3, direction: THREE.Vector3) => {
    controls.target.copy(at);
    camera.position.copy(at).addScaledVector(direction.normalize(), REACH * all.radius);
    camera.near = 0.001;
    camera.far = 2 * REACH * all.radius;
    controls.update();
    dirty = true;
  };

  function look(view: number) {
    target(controls.target.clone(), new THREE.Vector3(...VIEWS[view].direction));
    viewButtons.forEach((node, i) => node.setAttribute("aria-pressed", String(i === view)));
  }

  function fit(everything: boolean) {
    const sphere = everything ? all : boundingSphere([frames[index]]);
    viewRadius = sphere.radius * PADDING;
    camera.zoom = 1;
    target(sphere.center, camera.position.clone().sub(controls.target));
    resize();
  }

  function resize() {
    const { clientWidth: width, clientHeight: height } = stage;
    if (!width || !height) return;
    renderer.setSize(width, height);
    (highlight.material as LineMaterial).resolution.set(width, height);
    const aspect = width / height;
    Object.assign(camera, { left: -viewRadius * aspect, right: viewRadius * aspect, top: viewRadius, bottom: -viewRadius });
    camera.updateProjectionMatrix();
    dirty = true;
  }

  function sync() {
    timeline.value = String(index);
    timeline.style.setProperty("--p", String(last ? index / last : 0));
    status.textContent = `${pad(index + 1, digits)}/${frames.length}  t=${pad(frames[index].time.toFixed(3), timeWidth)}`;
    fpsText.textContent = `fps=${pad(fps, fpsDigits)}`;
    modeText.textContent = `loop=${LOOPS[loop].padEnd(7, "\u2007")}  labels=${showLabels ? "on\u2007" : "off"}  body=${selected < 0 ? "none" : frames[0].bodies[selected].label}`;
    const position = positionOf(fps);
    speedSlider.value = String(position);
    speedSlider.style.setProperty("--p", String(position / 100));
    // Rewriting a button's content during a click makes the browser drop the click, so write only changes.
    const icon = playing ? ICONS.pause : ICONS.play;
    if (playButton.innerHTML !== icon) playButton.innerHTML = icon;
    dirty = true;
  }

  function draw() {
    const frame = frames[index];
    write(bodyLines, frame.bodies.flatMap(({ segments }) => segments.flat()));
    write(jointLines, frame.joints.flatMap(({ i, j }) => [i, j]));
    write(origins, frame.bodies.map(({ origin }) => origin));
    highlight.visible = selected >= 0;
    highlightEnds.visible = selected >= 0;
    if (selected >= 0) {
      const ends = frame.bodies[selected].segments.flat();
      highlight.geometry.setPositions(ends.flat());
      write(highlightEnds, ends);
      highlightEnds.geometry.setDrawRange(0, ends.length);
    }
    renderer.setViewport(0, 0, stage.clientWidth, stage.clientHeight);
    renderer.clear();
    renderer.render(scene, camera);
    
    triadCamera.position.copy(camera.position).sub(controls.target).setLength(5);
    triadCamera.lookAt(0, 0, 0);
    renderer.clearDepth();
    renderer.setViewport(4, 4, TRIAD_SIZE, TRIAD_SIZE);
    renderer.render(triadScene, triadCamera);
    for (const { tip, node } of AXES) {
      const { x, y } = tip.clone().project(triadCamera);
      node.style.transform = `translate(${4 + ((x + 1) / 2) * TRIAD_SIZE}px, ${stage.clientHeight - 4 - ((y + 1) / 2) * TRIAD_SIZE}px)`;
    }
    labelLayer.hidden = !showLabels;
    if (!showLabels) return;
    const { clientWidth: width, clientHeight: height } = stage;
    for (const { at, node } of labels) {
      const screen = new THREE.Vector3(...at(frame)).project(camera);
      node.style.transform = `translate(${((screen.x + 1) / 2) * width}px, ${((1 - screen.y) / 2) * height}px)`;
    }
  }

  /** Draws at once if anything changed; user input calls it so it never waits for the next frame. */
  function flush() {
    if (!dirty || !root.offsetParent) return;
    dirty = false;
    draw();
  }

  /** Same rules as the terminal viewer. */
  function tick() {
    if (direction > 0 && index < last) index++;
    else if (direction > 0) {
      if (LOOPS[loop] === "restart") index = 0;
      else if (LOOPS[loop] === "bounce" && last > 0) [direction, index] = [-1, index - 1];
      else playing = false;
    } else if (index > 0) index--;
    else {
      direction = 1;
      if (last > 0) index = 1;
    }
    sync();
  }

  function togglePlay() {
    if (last === 0) return;
    if (!playing && LOOPS[loop] === "once" && index === last) [index, direction] = [0, 1];
    playing = !playing;
    sync();
  }

  function step(by: -1 | 1) {
    playing = false;
    if (by < 0) index = index > 0 ? index - 1 : last;
    else [index, direction] = [Math.min(index + 1, last), 1];
    sync();
  }

  function speed(by: number) {
    fps = fpsAt(Math.min(Math.max(positionOf(fps) + by * 100 / 6, 0), 100));
    sync();
  }

  function cycleLoop() {
    [loop, direction] = [(loop + 1) % LOOPS.length, 1];
    sync();
  }

  function toggleLabels() {
    showLabels = !showLabels;
    sync();
  }

  const KEYS: Record<string, () => void> = {
    " ": togglePlay,
    ArrowLeft: () => step(-1),
    ArrowRight: () => step(1),
    ArrowUp: () => speed(1),
    ArrowDown: () => speed(-1),
    "+": () => speed(1),
    "=": () => speed(1),
    "-": () => speed(-1),
    l: cycleLoop,
    v: toggleLabels,
    f: () => fit(false),
    F: () => fit(true),
    ...Object.fromEntries(VIEWS.map(({ key }, view) => [key, () => look(view)])),
  };

  root.addEventListener("keydown", (event) => {
    // The slider keeps its arrow keys and a focused button its Space.
    const own = event.target instanceof HTMLInputElement || event.target instanceof HTMLButtonElement;
    if (own || event.ctrlKey || event.metaKey || event.altKey) return;
    const action = KEYS[event.key];
    if (!action) return;
    event.preventDefault();
    action();
    flush();
  });
  // Keys work after a click anywhere in the viewer, buttons included.
  root.addEventListener("click", (event) => {
    root.focus();
    flush();
  });
  /** Selects the body whose segment or origin is within `PICK_PX` of the click, or none. */
  function pick(x: number, y: number) {
    const { left, top, width, height } = renderer.domElement.getBoundingClientRect();
    const screen = (p: Vec3) => {
      const v = new THREE.Vector3(...p).project(camera);
      return [((v.x + 1) / 2) * width, ((1 - v.y) / 2) * height];
    };
    let [best, nearest] = [-1, PICK_PX];
    frames[index].bodies.forEach(({ origin, segments }, body) => {
      for (const [a, b] of segments.length ? segments : [[origin, origin] as [Vec3, Vec3]]) {
        const [ax, ay] = screen(a);
        const [bx, by] = screen(b);
        const [dx, dy] = [bx - ax, by - ay];
        const along = dx || dy ? Math.min(Math.max(((x - left - ax) * dx + (y - top - ay) * dy) / (dx * dx + dy * dy), 0), 1) : 0;
        const distance = Math.hypot(x - left - ax - along * dx, y - top - ay - along * dy);
        if (distance < nearest) [best, nearest] = [body, distance];
      }
    });
    selected = best === selected ? -1 : best;
    paintBodies();
    onSelect?.(selected < 0 ? null : selected);
    sync();
  }
  let down: [number, number] | null = null;
  renderer.domElement.addEventListener("pointerdown", ({ clientX, clientY }) => (down = [clientX, clientY]));
  renderer.domElement.addEventListener("pointerup", ({ clientX, clientY }) => {
    if (down && Math.hypot(clientX - down[0], clientY - down[1]) < 4) {
      pick(clientX, clientY);
      flush();
    }
    down = null;
  });
  speedSlider.addEventListener("input", () => {
    fps = fpsAt(Number(speedSlider.value));
    sync();
    flush();
  });
  timeline.addEventListener("input", () => {
    playing = false;
    index = Number(timeline.value);
    sync();
    flush();
  });

  const observer = new ResizeObserver(resize);
  observer.observe(stage);

  let before = performance.now();
  let owed = 0;
  let frameId = 0;
  const run = (now: number) => {
    frameId = requestAnimationFrame(run);
    const elapsed = now - before;
    before = now;
    // Nothing is drawn or played while the pane is hidden.
    if (!root.offsetParent) return;
    if (playing) {
      // Steps due since the last frame; a slow renderer skips steps to stay on the clock.
      const interval = 1000 / fps;
      owed = Math.min(owed + elapsed, 10 * interval);
      for (; owed >= interval && playing; owed -= interval) tick();
    }
    flush();
  };

  look(1);
  fit(true);
  sync();
  frameId = requestAnimationFrame(run);

  return {
    destroy() {
      cancelAnimationFrame(frameId);
      observer.disconnect();
      dark.removeEventListener("change", applyTheme);
      controls.dispose();
      renderer.dispose();
      root.remove();
    },
  };
}
