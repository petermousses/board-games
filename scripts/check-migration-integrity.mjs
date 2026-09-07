#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { lstatSync, readdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const DEFAULT_BASELINE_REF = "origin/develop";
const MIGRATIONS_DIRECTORY = "migrations";
const MIGRATION_FILE_PATTERN = /^(\d{4,})_([A-Za-z0-9][A-Za-z0-9_-]*)\.sql$/;

function printUsage() {
  console.log(`usage: ${process.argv[1]} [--baseline <git-ref>]

Checks that SQLx migrations are sequential, append-only, and unchanged from a
baseline ref. The baseline defaults to ${DEFAULT_BASELINE_REF}; override it with
--baseline or MIGRATION_BASELINE_REF.`);
}

function fail(message) {
  console.error(`migration integrity check failed: ${message}`);
  process.exit(1);
}

function runGit(args, options = {}) {
  const result = spawnSync("git", args, {
    cwd: options.cwd,
    encoding: null,
    stdio: ["ignore", "pipe", "pipe"],
  });

  if (result.error) {
    fail(`could not run git ${args[0]}: ${result.error.message}`);
  }

  return result;
}

function gitOutput(args, message, options = {}) {
  const result = runGit(args, options);

  if (result.status !== 0) {
    const details = result.stderr.toString("utf8").trim();
    fail(`${message}${details ? `: ${details}` : ""}`);
  }

  return result.stdout;
}

function parseArguments() {
  let baselineRef = process.env.MIGRATION_BASELINE_REF || DEFAULT_BASELINE_REF;
  const args = process.argv.slice(2);

  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];

    if (argument === "--help" || argument === "-h") {
      printUsage();
      process.exit(0);
    }

    if (argument === "--baseline") {
      const value = args[index + 1];

      if (!value || value.startsWith("-")) {
        fail("--baseline requires a git ref");
      }

      baselineRef = value;
      index += 1;
      continue;
    }

    fail(`unknown argument ${JSON.stringify(argument)}`);
  }

  return baselineRef;
}

function gitRoot() {
  return gitOutput(
    ["rev-parse", "--show-toplevel"],
    "must run inside a git worktree",
  )
    .toString("utf8")
    .trim();
}

function resolveBaseline(ref, root) {
  if (!ref || ref.startsWith("-")) {
    fail("baseline ref must be a non-option git ref");
  }

  const result = runGit(["rev-parse", "--verify", "--quiet", `${ref}^{commit}`], {
    cwd: root,
  });

  if (result.status !== 0) {
    fail(
      `baseline ref ${JSON.stringify(ref)} cannot be resolved to a commit; fetch it or pass --baseline <ref>`,
    );
  }

  return result.stdout.toString("utf8").trim();
}

function formatVersion(version) {
  return version.toString().padStart(4, "0");
}

function parseMigrationPath(path, source) {
  const fileName = path.slice(path.lastIndexOf("/") + 1);
  const match = MIGRATION_FILE_PATTERN.exec(fileName);

  if (!match) {
    fail(`${source} migration ${JSON.stringify(path)} must match NNNN_description.sql`);
  }

  const version = BigInt(match[1]);

  if (version < 1n) {
    fail(`${source} migration ${JSON.stringify(path)} must start at version 0001 or later`);
  }

  const expected = formatVersion(version);

  if (match[1] !== expected) {
    fail(
      `${source} migration ${JSON.stringify(path)} has non-canonical version ${match[1]}; use ${expected}`,
    );
  }

  return { path, version };
}

function validateMigrationSet(entries, source) {
  const byVersion = new Map();
  const byPath = new Map();

  for (const entry of entries) {
    if (byPath.has(entry.path)) {
      fail(`${source} contains duplicate migration path ${JSON.stringify(entry.path)}`);
    }

    if (byVersion.has(entry.version)) {
      fail(
        `${source} contains duplicate migration version ${formatVersion(entry.version)} in ${JSON.stringify(byVersion.get(entry.version).path)} and ${JSON.stringify(entry.path)}`,
      );
    }

    byPath.set(entry.path, entry);
    byVersion.set(entry.version, entry);
  }

  const sorted = [...byVersion.values()].sort((left, right) => {
    if (left.version < right.version) return -1;
    if (left.version > right.version) return 1;
    return 0;
  });

  let expected = 1n;

  for (const entry of sorted) {
    if (entry.version !== expected) {
      fail(
        `${source} has a migration version gap: expected ${formatVersion(expected)} but found ${formatVersion(entry.version)} in ${JSON.stringify(entry.path)}`,
      );
    }

    expected += 1n;
  }

  return {
    byPath,
    maxVersion: sorted.length === 0 ? 0n : sorted.at(-1).version,
  };
}

function readWorkingMigrations(root) {
  const directory = resolve(root, MIGRATIONS_DIRECTORY);
  const rootPrefix = `${root}/`;

  if (!directory.startsWith(rootPrefix)) {
    fail(`migration directory escapes git worktree: ${directory}`);
  }

  let directoryStat;

  try {
    directoryStat = lstatSync(directory);
  } catch (error) {
    fail(`could not read ${MIGRATIONS_DIRECTORY}: ${error.message}`);
  }

  if (!directoryStat.isDirectory() || directoryStat.isSymbolicLink()) {
    fail(`${MIGRATIONS_DIRECTORY} must be a real directory inside the git worktree`);
  }

  let directoryEntries;

  try {
    directoryEntries = readdirSync(directory, { withFileTypes: true });
  } catch (error) {
    fail(`could not read ${MIGRATIONS_DIRECTORY}: ${error.message}`);
  }

  const migrations = directoryEntries.map((entry) => {
    if (!entry.isFile()) {
      fail(`working tree migration entry ${JSON.stringify(join(MIGRATIONS_DIRECTORY, entry.name))} must be a regular file`);
    }

    const path = `${MIGRATIONS_DIRECTORY}/${entry.name}`;
    const parsed = parseMigrationPath(path, "working tree");

    return {
      ...parsed,
      contents: readFileSync(join(directory, entry.name)),
    };
  });

  return validateMigrationSet(migrations, "working tree");
}

function readBaselineMigrations(root, baselineCommit) {
  const output = gitOutput(
    ["ls-tree", "-r", "-z", "--full-tree", baselineCommit, "--", MIGRATIONS_DIRECTORY],
    `could not enumerate migrations at baseline ${baselineCommit}`,
    { cwd: root },
  );
  const migrations = [];

  for (const record of output.toString("utf8").split("\0")) {
    if (!record) continue;

    const separator = record.indexOf("\t");

    if (separator === -1) {
      fail(`could not parse baseline migration entry ${JSON.stringify(record)}`);
    }

    const metadata = record.slice(0, separator).split(" ");
    const path = record.slice(separator + 1);
    const name = path.slice(`${MIGRATIONS_DIRECTORY}/`.length);

    if (metadata[1] !== "blob" || !/^100[0-7]{3}$/.test(metadata[0])) {
      fail(`baseline migration entry ${JSON.stringify(path)} must be a file`);
    }

    if (!path.startsWith(`${MIGRATIONS_DIRECTORY}/`) || name.includes("/")) {
      fail(`baseline migration ${JSON.stringify(path)} must be a direct child of ${MIGRATIONS_DIRECTORY}`);
    }

    migrations.push(parseMigrationPath(path, "baseline"));
  }

  return validateMigrationSet(migrations, "baseline");
}

function baselineContents(root, baselineCommit, path) {
  return gitOutput(
    ["show", `${baselineCommit}:${path}`],
    `could not read baseline migration ${JSON.stringify(path)}`,
    { cwd: root },
  );
}

function verifyAppendOnly(root, baselineCommit, baseline, working) {
  for (const [path, baselineMigration] of baseline.byPath) {
    const workingMigration = working.byPath.get(path);

    if (!workingMigration) {
      fail(`baseline migration ${JSON.stringify(path)} was deleted`);
    }

    const expectedContents = baselineContents(root, baselineCommit, path);

    if (!expectedContents.equals(workingMigration.contents)) {
      fail(`baseline migration ${JSON.stringify(path)} was edited`);
    }

    if (workingMigration.version !== baselineMigration.version) {
      fail(`baseline migration ${JSON.stringify(path)} changed version`);
    }
  }

  for (const [path, migration] of working.byPath) {
    if (!baseline.byPath.has(path) && migration.version <= baseline.maxVersion) {
      fail(
        `new migration ${JSON.stringify(path)} must use a version greater than baseline maximum ${formatVersion(baseline.maxVersion)}`,
      );
    }
  }
}

function main() {
  const baselineRef = parseArguments();
  const root = gitRoot();
  const baselineCommit = resolveBaseline(baselineRef, root);
  const baseline = readBaselineMigrations(root, baselineCommit);
  const working = readWorkingMigrations(root);

  verifyAppendOnly(root, baselineCommit, baseline, working);

  console.log(
    `migration integrity check passed: ${working.byPath.size} migration(s), baseline ${baselineRef} (${baselineCommit.slice(0, 12)})`,
  );
}

main();
