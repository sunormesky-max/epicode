import assert from "node:assert/strict";
import test from "node:test";

import { evaluateAuditReport, parseAuditJson } from "./check-dependency-audit.mjs";

const npmScope = "frontend/package-lock.json";
const cargoScope = "backend/Cargo.lock";

function policy(exceptions = []) {
  return {
    scopes: {
      [npmScope]: "npm",
      [cargoScope]: "cargo",
    },
    exceptions: [
      {
        ecosystem: "npm",
        scope: npmScope,
        id: "GHSA-vfj7-8cjw-p6xm",
        package: "braces",
        version: "3.0.3",
        owner: "security@epicode.cn",
        expiresOn: "2026-11-09",
        reason: "Unpatched dev-only dependency.",
      },
      {
        ecosystem: "cargo",
        scope: cargoScope,
        id: "RUSTSEC-2023-0071",
        package: "rsa",
        version: "0.9.10",
        owner: "security@epicode.cn",
        expiresOn: "2026-11-09",
        reason: "No patched release; private decrypt is not used by HTTP/MCP handlers.",
      },
      ...exceptions,
    ],
  };
}

const npmReport = {
  metadata: {
    vulnerabilities: { info: 0, low: 0, moderate: 0, high: 2, critical: 0, total: 2 },
  },
  vulnerabilities: {
    braces: {
      name: "braces",
      severity: "high",
      via: [
        {
          name: "braces",
          url: "https://github.com/advisories/GHSA-vfj7-8cjw-p6xm",
          severity: "high",
          range: "<=3.0.3",
        },
      ],
      nodes: ["node_modules/braces"],
    },
    tailwindcss: {
      name: "tailwindcss",
      severity: "high",
      via: ["braces"],
      nodes: ["node_modules/tailwindcss"],
    },
  },
};

const npmLockfile = {
  packages: {
    "node_modules/braces": { version: "3.0.3" },
    "node_modules/tailwindcss": { version: "3.4.19" },
  },
};

function cargoReport(version = "0.9.10") {
  return {
    database: { "advisory-count": 1 },
    lockfile: { "dependency-count": 1 },
    settings: { ignore: [] },
    vulnerabilities: {
      found: true,
      count: 1,
      list: [
        {
          advisory: { id: "RUSTSEC-2023-0071", package: "rsa" },
          package: { name: "rsa", version },
        },
      ],
    },
    warnings: {},
  };
}

test("allows only the exact npm advisory/package/version exception through aggregate via records", () => {
  const messages = evaluateAuditReport({
    ecosystem: "npm",
    report: npmReport,
    lockfile: npmLockfile,
    scope: npmScope,
    auditExitCode: 1,
    auditPolicy: policy(),
    today: "2026-10-09",
  });

  assert.match(messages.join("\n"), /GHSA-vfj7-8cjw-p6xm braces@3\.0\.3/);
});

test("fails closed on an unrelated high npm advisory", () => {
  const report = {
    metadata: {
      vulnerabilities: { info: 0, low: 0, moderate: 0, high: 1, critical: 0, total: 1 },
    },
    vulnerabilities: {
      "new-package": {
        name: "new-package",
        severity: "high",
        via: [
          {
            name: "new-package",
            url: "https://github.com/advisories/GHSA-aaaa-bbbb-cccc",
            severity: "high",
            range: "<2.0.0",
          },
        ],
        nodes: ["node_modules/new-package"],
      },
    },
  };

  assert.throws(
    () =>
      evaluateAuditReport({
        ecosystem: "npm",
        report,
        lockfile: { packages: { "node_modules/new-package": { version: "1.0.0" } } },
        scope: npmScope,
        auditExitCode: 1,
        auditPolicy: policy(),
        today: "2026-10-09",
      }),
    /Unexpected high\/critical npm advisory GHSA-aaaa-bbbb-cccc/,
  );
});

test("allows the exact current Rust exception but rejects a different locked version", () => {
  const report = cargoReport();

  const messages = evaluateAuditReport({
    ecosystem: "cargo",
    report,
    scope: cargoScope,
    auditExitCode: 1,
    auditPolicy: policy(),
    today: "2026-10-09",
  });
  assert.match(messages.join("\n"), /RUSTSEC-2023-0071 rsa@0\.9\.10/);

  report.vulnerabilities.list[0].package.version = "0.9.11";
  assert.throws(
    () =>
      evaluateAuditReport({
        ecosystem: "cargo",
        report,
        scope: cargoScope,
        auditExitCode: 1,
        auditPolicy: policy(),
        today: "2026-10-09",
      }),
    /Unexpected cargo finding/,
  );
});

test("rejects expired exceptions", () => {
  const report = cargoReport();
  const expiredPolicy = policy().exceptions.map((exception) => ({
    ...exception,
    expiresOn: "2026-10-08",
  }));

  assert.throws(
    () =>
      evaluateAuditReport({
        ecosystem: "cargo",
        report,
        scope: cargoScope,
        auditExitCode: 1,
        auditPolicy: { ...policy(), exceptions: expiredPolicy },
        today: "2026-10-09",
      }),
    /expired on 2026-10-08/,
  );
});

test("rejects malformed audit JSON", () => {
  assert.throws(() => parseAuditJson('{"metadata":', "fixture"), /fixture is not valid JSON/);
});
