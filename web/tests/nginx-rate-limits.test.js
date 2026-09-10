import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const nginxConfig = new URL("../nginx.conf", import.meta.url);
const ingressManifest = new URL("../../deploy/k8s/ingress.yaml", import.meta.url);
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

  assert.doesNotMatch(config, /^\s*(?:proxy_|fastcgi_|uwsgi_|scgi_|limit_req)/m);
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

  assert.match(ingress, /^\s*ingressClassName: traefik\s*$/m);
  assert.match(ingress, /^\s+- games\.omv\.mousses\.xyz\s*$/m);
  assert.match(ingress, /^\s+secretName: board-games-tls\s*$/m);
  assert.ok(serviceBackend(ingress, "/api", "board-games-api"), "the /api prefix must route to the API");
  assert.ok(serviceBackend(ingress, "/", "board-games-web"), "the / prefix must route to the frontend");
  assert.ok(ingress.indexOf("- path: /api") < ingress.indexOf("- path: /\n"), "the API route must precede the catch-all");
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
