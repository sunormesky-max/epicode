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
        id: "GHSA-test-waiver-0001",
        package: "sample-braces",
        version: "1.2.3",
        owner: "security@epicode.cn",
        expiresOn: "2026-11-09",
        reason: "Fixture proves exact exception matching.",
      },
      {
        ecosystem: "cargo",
        scope: cargoScope,
        id: "RUSTSEC-2099-0001",
        package: "sample-rsa",
        version: "1.2.3",
        owner: "security@epicode.cn",
        expiresOn: "2026-11-09",
        reason: "Fixture proves exact exception matching.",
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
    "sample-braces": {
      name: "sample-braces",
      severity: "high",
      via: [
        {
          name: "sample-braces",
          url: "https://github.com/advisories/GHSA-test-waiver-0001",
          severity: "high",
          range: "<2.0.0",
        },
      ],
      nodes: ["node_modules/sample-braces"],
    },
    tailwindcss: {
      name: "tailwindcss",
      severity: "high",
      via: ["sample-braces"],
      nodes: ["node_modules/tailwindcss"],
    },
  },
};

const npmLockfile = {
  packages: {
    "node_modules/sample-braces": { version: "1.2.3" },
    "node_modules/tailwindcss": { version: "3.4.19" },
  },
};

function cargoReport(version = "1.2.3") {
  return {
    database: { "advisory-count": 1 },
    lockfile: { "dependency-count": 1 },
    settings: { ignore: [] },
    vulnerabilities: {
      found: true,
      count: 1,
      list: [
        {
          advisory: { id: "RUSTSEC-2099-0001", package: "sample-rsa" },
          package: { name: "sample-rsa", version },
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

  assert.match(messages.join("\n"), /GHSA-test-waiver-0001 sample-braces@1\.2\.3/);
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

test("allows an exact Rust exception but rejects a different locked version", () => {
  const report = cargoReport();

  const messages = evaluateAuditReport({
    ecosystem: "cargo",
    report,
    scope: cargoScope,
    auditExitCode: 1,
    auditPolicy: policy(),
    today: "2026-10-09",
  });
  assert.match(messages.join("\n"), /RUSTSEC-2099-0001 sample-rsa@1\.2\.3/);

  report.vulnerabilities.list[0].package.version = "1.2.4";
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
