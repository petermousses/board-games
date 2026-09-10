export const LOW_PERFORMANCE_STORAGE_KEY = "tabletop.low-performance.v1";

const CLASSIC_PROFILE = Object.freeze({
  name: "classic",
  antialias: false,
  pixelRatioCap: 1,
  shadows: false,
  shadowMapType: "pcf",
  shadowMapSize: 512,
  roundedGeometry: false,
  background: "#173b32",
});

const ENHANCED_PROFILE = Object.freeze({
  name: "enhanced",
  antialias: true,
  pixelRatioCap: 2,
  shadows: true,
  shadowMapType: "pcf-soft",
  shadowMapSize: 1536,
  roundedGeometry: true,
  background: "#0f211b",
});

export function renderProfile(lowPerformance = false) {
  return lowPerformance ? CLASSIC_PROFILE : ENHANCED_PROFILE;
}

export function parseLowPerformanceMode(value) {
  return value === "true";
}

export function readLowPerformanceMode(storage) {
  try {
    return parseLowPerformanceMode(storage.getItem(LOW_PERFORMANCE_STORAGE_KEY));
  } catch {
    return false;
  }
}

export function writeLowPerformanceMode(storage, enabled) {
  try {
    storage.setItem(LOW_PERFORMANCE_STORAGE_KEY, String(Boolean(enabled)));
    return true;
  } catch {
    return false;
  }
}

export function cameraDefaults() {
  return { enablePan: true, screenSpacePanning: true, minZoom: 0.7, maxZoom: 2.6 };
}
