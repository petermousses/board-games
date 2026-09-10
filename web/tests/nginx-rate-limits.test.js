import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const nginxConfig = new URL("../nginx.conf", import.meta.url);
const ingressManifest = new URL("../../deploy/k8s/ingress.yaml", import.meta.url);
const middlewareManifest = new URL("../../deploy/k8s/middleware.yaml", import.meta.url);
const kustomizationManifest = new URL("../../deploy/k8s/kustomization.yaml", import.meta.url);
const networkPolicyManifest = new URL("../../deploy/k8s/networkpolicy.yaml", import.meta.url);

function manifestByName(manifest, kind, name) {
  const resource = manifest
    .split(/^---\s*$/m)
    .find((block) => new RegExp(`^kind: ${kind}$`, "m").test(block) && new RegExp(`^  name: ${name}$`, "m").test(block));
  assert.ok(resource, `${kind} ${name} is missing`);
  return resource;
}

function serviceBackend(ingress, path, service) {
  const escapedPath = path.replaceAll("/", "\\/");
  return new RegExp(
    `^\\s+- path: ${escapedPath}\\s*$\\n` +
      "\\s+pathType: Prefix\\s*\\n" +
      "\\s+backend:\\s*\\n" +
      "\\s+service:\\s*\\n" +
      `\\s+name: ${service}\\s*$\\n` +
      "\\s+port:\\s*\\n" +
      "\\s+name: http\\s*$",
    "m",
  ).test(ingress);
}

test("nginx serves frontend assets only and leaves API routing to ingress", async () => {
  const config = await readFile(nginxConfig, "utf8");

  assert.doesNotMatch(config, /^\s*(?:proxy(?:_[a-z_]+)?|limit_(?:req|conn|rate)(?:_[a-z_]+)?)\b/m);
  assert.doesNotMatch(config, /^\s*location\b[^\n{]*\/api(?:\/|\s|\{|$)/m);
  assert.deepEqual(
    [...config.matchAll(/^\s*location\s+([^\n{]+)\s*\{/gm)].map(([, location]) => location.trim()),
    ["= /healthz", "/"],
  );
  assert.match(config, /^\s*root \/usr\/share\/nginx\/html;\s*$/m);
  assert.match(config, /^\s*try_files \$uri \$uri\/ \/index\.html;\s*$/m);
});

test("ingress preserves the public host and TLS while routing API and frontend separately", async () => {
  const ingress = await readFile(ingressManifest, "utf8");
  const apiIngress = manifestByName(ingress, "Ingress", "board-games-api");
  const frontendIngress = manifestByName(ingress, "Ingress", "board-games");

  for (const resource of [apiIngress, frontendIngress]) {
    assert.match(resource, /^\s*ingressClassName: traefik\s*$/m);
    assert.match(resource, /^\s+- games\.omv\.mousses\.xyz\s*$/m);
    assert.match(resource, /^\s+secretName: board-games-tls\s*$/m);
  }
  assert.ok(serviceBackend(apiIngress, "/api", "board-games-api"), "the /api prefix must route to the API");
  assert.doesNotMatch(apiIngress, /^\s+- path: \/\s*$/m, "the API ingress must not own the frontend catch-all");
  assert.ok(serviceBackend(frontendIngress, "/", "board-games-web"), "the / prefix must route to the frontend");
  assert.doesNotMatch(frontendIngress, /^\s+- path: \/api\s*$/m, "the frontend ingress must not own the API route");
});

test("Traefik rate limiting is attached to the API ingress only", async () => {
  const ingress = await readFile(ingressManifest, "utf8");
  const middleware = await readFile(middlewareManifest, "utf8");
  const kustomization = await readFile(kustomizationManifest, "utf8");
  const apiIngress = manifestByName(ingress, "Ingress", "board-games-api");
  const frontendIngress = manifestByName(ingress, "Ingress", "board-games");
  const rateLimit = manifestByName(middleware, "Middleware", "board-games-api-rate-limit");

  assert.match(rateLimit, /^apiVersion: traefik\.io\/v1alpha1\s*$/m);
  assert.match(rateLimit, /^\s+average: 30\s*$/m);
  assert.match(rateLimit, /^\s+period: 1m\s*$/m);
  assert.match(rateLimit, /^\s+burst: 20\s*$/m);
  assert.match(kustomization, /^\s+- middleware\.yaml\s*$/m);
  assert.match(
    apiIngress,
    /^\s+traefik\.ingress\.kubernetes\.io\/router\.middlewares: board-games-board-games-api-rate-limit@kubernetescrd\s*$/m,
  );
  assert.doesNotMatch(frontendIngress, /traefik\.ingress\.kubernetes\.io\/router\.middlewares/);
  assert.equal(
    [...ingress.matchAll(/traefik\.ingress\.kubernetes\.io\/router\.middlewares/g)].length,
    1,
    "exactly one ingress route may attach the API middleware",
  );
});

test("network policy permits Traefik to reach the API service port", async () => {
  const networkPolicy = await readFile(networkPolicyManifest, "utf8");
  const traefikToApi = manifestByName(networkPolicy, "NetworkPolicy", "allow-traefik-to-api");

  assert.match(traefikToApi, /podSelector:\s*\n\s+matchLabels:\s*\n\s+app\.kubernetes\.io\/name: board-games-api/);
  assert.match(traefikToApi, /policyTypes: \[Ingress\]/);
  assert.match(
    traefikToApi,
    /namespaceSelector:\s*\n\s+matchLabels:\s*\n\s+kubernetes\.io\/metadata\.name: kube-system\s*\n\s+podSelector:\s*\n\s+matchLabels:\s*\n\s+app\.kubernetes\.io\/name: traefik/,
  );
  assert.match(traefikToApi, /ports:\s*\n\s+- protocol: TCP\s*\n\s+port: 8080/);
});
