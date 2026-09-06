import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";

// Rendering stays here; rule decisions and private state stay on the server.
export class Table {
  constructor(container, onPick, onError) {
    this.container = container;
    this.onPick = onPick;
    this.onError = onError;
    this.top = false;
    this.disposed = false;
    this.scene = new THREE.Scene();
    this.scene.background = new THREE.Color("#173b32");
    this.camera = new THREE.OrthographicCamera(-6, 6, 6, -6, 0.1, 100);
    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.renderer.shadowMap.enabled = true;
    this.renderer.shadowMap.type = THREE.PCFShadowMap;
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    this.renderer.toneMapping = THREE.ACESFilmicToneMapping;
    this.renderer.toneMappingExposure = 1.35;
    const canvas = this.renderer.domElement;
    canvas.setAttribute("aria-label", "interactive Three.js game board; keyboard controls are below the table");
    canvas.setAttribute("role", "img");
    canvas.dataset.renderer = "threejs";
    container.append(canvas);
    this.controls = new OrbitControls(this.camera, canvas);
    this.controls.enablePan = false;
    this.controls.enableRotate = false;
    this.controls.enableZoom = false;
    this.controls.minPolarAngle = 0.05;
    this.controls.maxPolarAngle = Math.PI / 2.6;
    this.controls.addEventListener("change", () => this.render());
    this.scene.add(new THREE.HemisphereLight(0xfff2d2, 0x345e54, 3));
    const sun = new THREE.DirectionalLight(0xfff3dc, 4);
    sun.position.set(-6, 16, 8);
    sun.castShadow = true;
    sun.shadow.mapSize.set(1024, 1024);
    Object.assign(sun.shadow.camera, { left: -14, right: 14, top: 14, bottom: -14, near: 0.5, far: 40 });
    sun.shadow.bias = -0.001;
    this.scene.add(sun);
    this.raycaster = new THREE.Raycaster();
    this.pointer = new THREE.Vector2();
    this.down = (event) => { this.pointerDown = { x: event.clientX, y: event.clientY, button: event.button }; };
    this.up = (event) => {
      if (!this.pointerDown || this.pointerDown.button !== 0 || Math.hypot(event.clientX - this.pointerDown.x, event.clientY - this.pointerDown.y) > 6) return;
      this.pointerDown = null;
      const pick = this.pick(event);
      if (pick) this.onPick(pick);
    };
    this.move = (event) => { canvas.style.cursor = this.pick(event) ? "pointer" : "default"; };
    this.lost = (event) => { event.preventDefault(); if (!this.disposed) this.onError("the graphics context was interrupted. reload to restore the 3D table; the move controls still work."); };
    canvas.addEventListener("pointerdown", this.down);
    canvas.addEventListener("pointerup", this.up);
    canvas.addEventListener("pointermove", this.move);
    canvas.addEventListener("webglcontextlost", this.lost);
    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(container);
  }

  update(model) {
    this.model = model;
    if (this.board) { this.scene.remove(this.board); disposeGroup(this.board); }
    this.board = new THREE.Group();
    this.scene.add(this.board);
    const base = this.mesh(new THREE.BoxGeometry(model.width + 0.25, 0.34, model.depth + 0.25), "#79583b");
    base.position.y = -0.25;
    this.board.add(base);
    const felt = this.mesh(new THREE.BoxGeometry(model.width, 0.08, model.depth), "#315647");
    felt.position.y = -0.05;
    this.board.add(felt);
    for (const link of model.links) {
      const dx = link.to.x - link.from.x, dz = link.to.z - link.from.z;
      const path = this.mesh(new THREE.BoxGeometry(0.32, 0.04, Math.hypot(dx, dz)), "#c6b994");
      path.position.set((link.from.x + link.to.x) / 2, 0.02, (link.from.z + link.to.z) / 2);
      path.rotation.y = Math.atan2(dx, dz);
      this.board.add(path);
    }
    for (const tile of model.tiles) {
      const square = this.mesh(new THREE.BoxGeometry(tile.width, 0.07, tile.depth), tile.color);
      square.position.set(tile.x, 0.025, tile.z);
      square.userData.pick = tile.pick;
      this.board.add(square);
    }
    for (const item of model.pieces) this.addPiece(item);
    for (const label of model.labels) this.addLabel(label);
    this.resize();
  }

  mesh(geometry, color, options = {}) {
    const mesh = new THREE.Mesh(geometry, new THREE.MeshStandardMaterial({ color, roughness: 0.65, ...options }));
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    return mesh;
  }

  addPiece(item) {
    const group = new THREE.Group();
    group.position.set(item.x, item.y || 0.08, item.z);
    group.userData.pick = item.pick;
    if (item.scale) group.scale.setScalar(item.scale);
    this.board.add(group);
    const add = (geometry, y, color = item.color) => {
      const mesh = this.mesh(geometry, color);
      mesh.position.y = y;
      group.add(mesh);
      return mesh;
    };
    if (item.type === "card") {
      const card = add(new THREE.BoxGeometry(item.width, 0.045, item.depth), 0, item.selected ? "#ecc35d" : item.back ? "#2c526a" : "#f5efdf");
      const texture = textTexture(item.text, { color: item.back ? "#d4dfd2" : item.color, background: item.back ? "#2c526a" : item.selected ? "#ecc35d" : "#f5efdf", card: true, back: item.back });
      const face = new THREE.Mesh(new THREE.PlaneGeometry(item.width * 0.94, item.depth * 0.96), new THREE.MeshBasicMaterial({ map: texture }));
      face.rotation.x = -Math.PI / 2;
      face.position.y = 0.026;
      card.add(face);
      return;
    }
    if (item.type === "checker") {
      add(new THREE.CylinderGeometry(0.36, 0.39, 0.16, 40), 0.08);
      add(new THREE.CylinderGeometry(0.29, 0.29, 0.025, 40), 0.175);
      if (item.king) add(new THREE.CylinderGeometry(0.23, 0.27, 0.1, 6), 0.24, "#e6b957");
      return;
    }
    if (item.type === "cross") {
      for (const rotation of [-Math.PI / 4, Math.PI / 4]) {
        const arm = add(new THREE.BoxGeometry(0.16, 0.13, 0.76), 0.09);
        arm.rotation.y = rotation;
      }
      return;
    }
    if (item.type === "ring") {
      const ring = add(new THREE.TorusGeometry(0.27, 0.075, 12, 32), 0.11);
      ring.rotation.x = Math.PI / 2;
      return;
    }
    if (item.type === "ship") { add(new THREE.BoxGeometry(0.7, 0.25, 0.7), 0.12); return; }
    if (item.type === "pin") { add(new THREE.CylinderGeometry(0.12, 0.15, 0.22, 16), 0.11); add(new THREE.SphereGeometry(0.22, 16, 12), 0.28); return; }
    const role = item.type === "pawn" ? "p" : item.role;
    const points = [[0.01, 0], [0.36, 0], [0.37, 0.08], [0.29, 0.14], [0.28, 0.19], [0.18, 0.25], [0.12, 0.48], [0.18, 0.52], [0.18, 0.59], [0.01, 0.59]];
    const height = role === "p" ? 0.78 : role === "k" || role === "q" ? 1.18 : 1;
    const body = add(new THREE.LatheGeometry(points.map(([radius, y]) => new THREE.Vector2(radius, y * height)), 32), 0);
    body.material.roughness = 0.38;
    if (role === "p") add(new THREE.SphereGeometry(0.21, 24, 16), 0.6);
    if (role === "b") { add(new THREE.SphereGeometry(0.2, 24, 16), 0.75); add(new THREE.ConeGeometry(0.11, 0.25, 20), 0.95); }
    if (role === "r") {
      add(new THREE.CylinderGeometry(0.28, 0.21, 0.23, 24), 0.69);
      for (let i = 0; i < 6; i += 1) { const merlon = add(new THREE.BoxGeometry(0.12, 0.14, 0.13), 0.85); merlon.position.x = Math.sin(i * Math.PI / 3) * 0.2; merlon.position.z = Math.cos(i * Math.PI / 3) * 0.2; }
    }
    if (role === "n") {
      const neck = add(new THREE.BoxGeometry(0.3, 0.48, 0.22), 0.72); neck.rotation.x = -0.32;
      const head = add(new THREE.BoxGeometry(0.29, 0.22, 0.43), 0.94); head.position.z = -0.1;
      for (const x of [-0.1, 0.1]) { const ear = add(new THREE.ConeGeometry(0.07, 0.19, 4), 1.11); ear.position.x = x; }
    }
    if (role === "q" || role === "k") {
      add(new THREE.CylinderGeometry(0.25, 0.12, 0.24, role === "q" ? 8 : 24), 0.85);
      add(new THREE.SphereGeometry(0.12, 20, 12), 1.04);
      if (role === "k") { add(new THREE.BoxGeometry(0.075, 0.28, 0.075), 1.2); add(new THREE.BoxGeometry(0.25, 0.075, 0.075), 1.23); }
    }
  }

  addLabel(item) {
    const texture = textTexture(item.text, { color: "#eadfc4" });
    const mesh = new THREE.Mesh(new THREE.PlaneGeometry(item.width, item.depth), new THREE.MeshBasicMaterial({ map: texture, transparent: true, depthWrite: false }));
    mesh.rotation.x = -Math.PI / 2;
    mesh.position.set(item.x, 0.075, item.z);
    this.board.add(mesh);
  }

  resize() {
    if (this.disposed || !this.model) return;
    const width = Math.max(1, this.container.clientWidth), height = Math.max(1, this.container.clientHeight);
    this.renderer.setSize(width, height, false);
    const aspect = width / height;
    const extent = Math.max(this.model.depth * (this.top ? 1 : 0.85), this.model.width / aspect) * 0.6 + 0.7;
    Object.assign(this.camera, { left: -extent * aspect, right: extent * aspect, top: extent, bottom: -extent });
    this.camera.position.set(0, 24, this.top ? 0.001 : 17);
    this.camera.lookAt(0, 0, 0);
    this.camera.updateProjectionMatrix();
    this.controls.target.set(0, 0, 0);
    this.controls.update();
    this.render();
  }

  setTop(top) { this.top = top; this.resize(); }
  setOrbit(enabled) { this.controls.enableRotate = enabled; this.controls.enableZoom = enabled; }
  render() { if (!this.disposed) this.renderer.render(this.scene, this.camera); }
  pick(event) {
    if (!this.board || this.disposed) return null;
    const rect = this.renderer.domElement.getBoundingClientRect();
    this.pointer.set((event.clientX - rect.left) / rect.width * 2 - 1, -(event.clientY - rect.top) / rect.height * 2 + 1);
    this.raycaster.setFromCamera(this.pointer, this.camera);
    for (const hit of this.raycaster.intersectObject(this.board, true)) {
      let object = hit.object;
      while (object && object !== this.board) {
        if (object.userData.pick) return object.userData.pick;
        object = object.parent;
      }
    }
    return null;
  }

  dispose() {
    this.disposed = true;
    this.resizeObserver.disconnect();
    this.controls.dispose();
    const canvas = this.renderer.domElement;
    canvas.removeEventListener("pointerdown", this.down);
    canvas.removeEventListener("pointerup", this.up);
    canvas.removeEventListener("pointermove", this.move);
    canvas.removeEventListener("webglcontextlost", this.lost);
    disposeGroup(this.scene);
    this.renderer.dispose();
    this.renderer.forceContextLoss();
    canvas.remove();
  }
}

function textTexture(text, { color = "#eadfc4", background = null, card = false, back = false } = {}) {
  const canvas = document.createElement("canvas");
  canvas.width = card ? 256 : 512;
  canvas.height = card ? 384 : 96;
  const context = canvas.getContext("2d");
  if (background) { context.fillStyle = background; context.fillRect(0, 0, canvas.width, canvas.height); }
  context.fillStyle = color;
  context.textBaseline = "middle";
  if (card && !back) {
    context.font = "bold 49px Georgia, serif";
    context.fillText(text, 12, 37);
    context.font = "100px Georgia, serif";
    context.textAlign = "center";
    context.fillText(text.slice(-1), 128, 207);
  } else {
    if (back) {
      context.strokeStyle = "#8aa8a5";
      context.lineWidth = 2;
      for (let i = -384; i < 256; i += 24) { context.beginPath(); context.moveTo(i, 0); context.lineTo(i + 384, 384); context.stroke(); }
    }
    context.textAlign = "center";
    context.font = back ? "100px Georgia, serif" : "42px system-ui, sans-serif";
    context.fillText(text, canvas.width / 2, canvas.height / 2, canvas.width - 10);
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  return texture;
}

function disposeGroup(group) {
  const disposed = new Set();
  group.traverse((object) => {
    if (object.geometry && !disposed.has(object.geometry)) { object.geometry.dispose(); disposed.add(object.geometry); }
    for (const material of object.material ? (Array.isArray(object.material) ? object.material : [object.material]) : []) {
      if (disposed.has(material)) continue;
      for (const value of Object.values(material)) if (value?.isTexture && !disposed.has(value)) { value.dispose(); disposed.add(value); }
      material.dispose(); disposed.add(material);
    }
    object.shadow?.map?.dispose();
  });
}
