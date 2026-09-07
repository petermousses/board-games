import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const nginxConfig = new URL("../nginx.conf", import.meta.url);

// Six-player Clue is the largest supported multiplayer table. Each browser polls once per 3 seconds.
const MAX_MULTIPLAYER_PLAYERS = 6;
const POLL_INTERVAL_SECONDS = 3;
const POLLS_PER_PLAYER_PER_MINUTE = 60 / POLL_INTERVAL_SECONDS;
const SESSION_READS_PER_SOURCE_PER_MINUTE = MAX_MULTIPLAYER_PLAYERS * POLLS_PER_PLAYER_PER_MINUTE;

// Keep action traffic at its existing conservative budget. Session reads get bounded headroom for six
// participants sharing a NAT and for a synchronized poll batch.
const GENERIC_API_RATE_PER_MINUTE = 30;
const GENERIC_API_BURST = 20;
const SESSION_READ_SOURCE_RATE_PER_MINUTE = 180;
const SESSION_READ_SOURCE_BURST = 30;
const SESSION_READ_IDENTITY_RATE_PER_MINUTE = 60;
const SESSION_READ_IDENTITY_BURST = 12;

function configuredZones(config) {
  return new Map(
    [...config.matchAll(/^\s*limit_req_zone\s+(\$\S+)\s+zone=(\w+):\S+\s+rate=(\d+)r\/m;\s*$/gm)]
      .map(([, key, name, rate]) => [name, { key, rate: Number(rate) }]),
  );
}

function locationBlock(config, predicate) {
  const lines = config.split("\n");
  const start = lines.findIndex((line) => predicate(line.trim()));
  assert.notEqual(start, -1, "expected nginx location is missing");
  assert.ok(lines[start].trimEnd().endsWith("{"), "location must open a block");

  let depth = 1;
  const block = [lines[start]];
  for (let index = start + 1; index < lines.length; index += 1) {
    const line = lines[index];
    block.push(line);
    depth += [...line].filter((character) => character === "{").length;
    depth -= [...line].filter((character) => character === "}").length;
    if (depth === 0) return block.join("\n");
  }
  assert.fail("unterminated nginx location block");
}

test("session polling has an isolated, bounded dual rate limit", async () => {
  const config = await readFile(nginxConfig, "utf8");
  const zones = configuredZones(config);

  // This captures the production failure mode before the specialized read policy exists.
  assert.ok(
    SESSION_READS_PER_SOURCE_PER_MINUTE > GENERIC_API_RATE_PER_MINUTE,
    `${MAX_MULTIPLAYER_PLAYERS} players polling every ${POLL_INTERVAL_SECONDS}s exceed the generic API budget`,
  );
  assert.ok(
    SESSION_READS_PER_SOURCE_PER_MINUTE < SESSION_READ_SOURCE_RATE_PER_MINUTE,
    "the shared-source read budget must exceed six-player steady polling",
  );
  assert.ok(
    POLLS_PER_PLAYER_PER_MINUTE < SESSION_READ_IDENTITY_RATE_PER_MINUTE,
    "each authenticated participant needs a finite budget above its polling cadence",
  );
  assert.ok(
    SESSION_READ_SOURCE_BURST >= MAX_MULTIPLAYER_PLAYERS,
    "the shared-source burst must accept one synchronized six-player poll batch",
  );

  assert.deepEqual(zones.get("game_requests"), {
    key: "$binary_remote_addr",
    rate: GENERIC_API_RATE_PER_MINUTE,
  });
  assert.deepEqual(zones.get("session_read_source"), {
    key: "$binary_remote_addr",
    rate: SESSION_READ_SOURCE_RATE_PER_MINUTE,
  });
  assert.deepEqual(zones.get("session_read_identity"), {
    key: "$http_authorization",
    rate: SESSION_READ_IDENTITY_RATE_PER_MINUTE,
  });
  assert.match(config, /^\s*limit_req_status 429;\s*$/m);

  const sessionLocation = locationBlock(
    config,
    (line) => line.startsWith('location ~ "^/api/v1/sessions/'),
  );
  const sessionPattern = /^\s*location\s+~\s+"(.+)"\s+\{\s*$/.exec(sessionLocation.split("\n", 1)[0]);
  assert.ok(sessionPattern, "session polling must use a regex location");
  const sessionPath = new RegExp(sessionPattern[1]);
  const sessionId = "00000000-0000-4000-8000-000000000001";
  assert.ok(sessionPath.test(`/api/v1/sessions/${sessionId}`), "the session detail path must use the read policy");
  for (const stateChangingSuffix of ["/join", "/start", "/actions"]) {
    assert.equal(
      sessionPath.test(`/api/v1/sessions/${sessionId}${stateChangingSuffix}`),
      false,
      `${stateChangingSuffix} must remain outside the read policy`,
    );
  }
  assert.match(
    sessionLocation,
    new RegExp(`^\\s*limit_req zone=session_read_source burst=${SESSION_READ_SOURCE_BURST} nodelay;\\s*$`, "m"),
  );
  assert.match(
    sessionLocation,
    new RegExp(`^\\s*limit_req zone=session_read_identity burst=${SESSION_READ_IDENTITY_BURST} nodelay;\\s*$`, "m"),
  );
  assert.doesNotMatch(sessionLocation, /game_requests/);

  const genericApiLocation = locationBlock(config, (line) => line === "location /api/ {");
  assert.match(
    genericApiLocation,
    new RegExp(`^\\s*limit_req zone=game_requests burst=${GENERIC_API_BURST} nodelay;\\s*$`, "m"),
  );
  assert.doesNotMatch(genericApiLocation, /session_read_(?:source|identity)/);
});
