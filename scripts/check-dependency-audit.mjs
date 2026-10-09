import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const policy = JSON.parse(
  fs.readFileSync(new URL("./dependency-audit-policy.json", import.meta.url), "utf8"),
);
const severities = ["info", "low", "moderate", "high", "critical"];
const severityRank = new Map(severities.map((severity, index) => [severity, index]));

export function parseAuditJson(text, label = "audit report") {
  try {
    return JSON.parse(text);
  } catch {
    throw new Error(`${label} is not valid JSON`);
  }
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function requireRecord(value, label) {
  if (!isRecord(value)) {
    throw new Error(`${label} must be a JSON object`);
  }
  return value;
}

function requireString(value, label) {
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error(`${label} must be a non-empty string`);
  }
  return value;
}

function normalizeRepoPath(value) {
  return value.replaceAll("\\", "/").replace(/^\.\//, "").replace(/\/+/g, "/");
}

function isValidDate(value) {
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(value)) {
    return false;
  }
  const parsed = new Date(`${value}T00:00:00.000Z`);
  return !Number.isNaN(parsed.getTime()) && parsed.toISOString().slice(0, 10) === value;
}

function validatePolicy(auditPolicy) {
  requireRecord(auditPolicy, "Dependency audit policy");
  const scopes = requireRecord(auditPolicy.scopes, "Policy scopes");
  if (!Array.isArray(auditPolicy.exceptions)) {
    throw new Error("Policy exceptions must be an array");
  }

  const exceptionKeys = new Set();
  for (const [scope, ecosystem] of Object.entries(scopes)) {
    requireString(scope, "Policy scope");
    if (!["cargo", "npm"].includes(ecosystem)) {
      throw new Error(`Unknown ecosystem "${ecosystem}" for ${scope}`);
    }
  }

  for (const exception of auditPolicy.exceptions) {
    requireRecord(exception, "Policy exception");
    for (const key of ["ecosystem", "scope", "id", "package", "version", "owner", "reason"]) {
      requireString(exception[key], `Policy exception ${key}`);
    }
    if (!isValidDate(exception.expiresOn)) {
      throw new Error(`Policy exception ${exception.id} has an invalid expiresOn date`);
    }
    if (scopes[exception.scope] !== exception.ecosystem) {
      throw new Error(`Policy exception ${exception.id} is outside its declared scope`);
    }

    const key = [
      exception.ecosystem,
      exception.scope,
      exception.id,
      exception.package,
      exception.version,
    ].join("|");
    if (exceptionKeys.has(key)) {
      throw new Error(`Duplicate policy exception for ${exception.id} ${exception.package}@${exception.version}`);
    }
    exceptionKeys.add(key);
  }
}

function scopedExceptions(auditPolicy, ecosystem, scope, today) {
  validatePolicy(auditPolicy);
  if (auditPolicy.scopes[scope] !== ecosystem) {
    throw new Error(`Undeclared ${ecosystem} audit scope: ${scope}`);
  }
  if (!isValidDate(today)) {
    throw new Error(`Invalid audit date: ${today}`);
  }

  const matching = auditPolicy.exceptions.filter(
    (exception) => exception.ecosystem === ecosystem && exception.scope === scope,
  );
  for (const exception of matching) {
    if (today > exception.expiresOn) {
      throw new Error(`Exception ${exception.id} for ${exception.package}@${exception.version} expired on ${exception.expiresOn}`);
    }
  }
  return matching;
}

function validateExitCode(exitCode, findingCount, tool) {
  if (!Number.isInteger(exitCode) || exitCode < 0 || exitCode > 1) {
    throw new Error(`${tool} exited with unexpected status ${exitCode}`);
  }
  if (exitCode === 1 && findingCount === 0) {
    throw new Error(`${tool} failed without machine-readable findings`);
  }
}

function exceptionFor(finding, exceptions, used) {
  const matches = exceptions.filter(
    (exception) =>
      exception.id === finding.id &&
      exception.package === finding.package &&
      exception.version === finding.version,
  );
  if (matches.length > 1) {
    throw new Error(`Multiple exceptions match ${finding.id} ${finding.package}@${finding.version}`);
  }
  if (matches.length === 0) {
    return null;
  }
  used.add(matches[0]);
  return matches[0];
}

function ensureNoStaleExceptions(exceptions, used) {
  const stale = exceptions.filter((exception) => !used.has(exception));
  if (stale.length > 0) {
    throw new Error(
      `Unused exception(s) must be removed: ${stale
        .map((exception) => `${exception.id} ${exception.package}@${exception.version}`)
        .join(", ")}`,
    );
  }
}

function summarizeExceptions(exceptions) {
  return exceptions.map(
    (exception) =>
      `allowed ${exception.id} ${exception.package}@${exception.version} (owner ${exception.owner}, expires ${exception.expiresOn})`,
  );
}

function advisoryId(advisory) {
  if (typeof advisory.id === "string" && advisory.id.trim() !== "") {
    return advisory.id;
  }
  if (typeof advisory.url === "string") {
    const match = advisory.url.match(/(?:GHSA-[A-Za-z0-9-]+|CVE-\d{4}-\d+)/i);
    if (match) {
      return match[0];
    }
  }
  return null;
}

function resolveNpmAdvisories(packageName, records, stack = []) {
  if (stack.includes(packageName)) {
    throw new Error(`Cycle in npm audit via graph: ${[...stack, packageName].join(" -> ")}`);
  }
  const record = records[packageName];
  if (!record) {
    throw new Error(`npm audit via graph references missing vulnerability record "${packageName}"`);
  }

  const nextStack = [...stack, packageName];
  const resolved = [];
  for (const via of record.via) {
    if (typeof via === "string") {
      resolved.push(...resolveNpmAdvisories(via, records, nextStack));
      continue;
    }
    if (!isRecord(via)) {
      throw new Error(`Invalid npm audit via entry for "${packageName}"`);
    }
    resolved.push({ advisory: via, packageName, record });
  }
  return resolved;
}

function evaluateNpm(report, lockfile, scope, exitCode, auditPolicy, today) {
  requireRecord(report, "npm audit report");
  if (report.error) {
    throw new Error(`npm audit returned an error: ${JSON.stringify(report.error)}`);
  }
  const metadataCounts = requireRecord(
    report.metadata?.vulnerabilities,
    "npm audit metadata.vulnerabilities",
  );
  const records = requireRecord(report.vulnerabilities, "npm audit vulnerabilities");
  const packages = requireRecord(lockfile?.packages, "npm package-lock packages");

  const observedCounts = Object.fromEntries(severities.map((severity) => [severity, 0]));
  for (const [name, record] of Object.entries(records)) {
    requireRecord(record, `npm vulnerability "${name}"`);
    if (!severityRank.has(record.severity)) {
      throw new Error(`Unknown npm severity "${record.severity}" for "${name}"`);
    }
    if (!Array.isArray(record.nodes) || record.nodes.length === 0) {
      throw new Error(`npm vulnerability "${name}" has no affected lockfile nodes`);
    }
    if (!Array.isArray(record.via) || record.via.length === 0) {
      throw new Error(`npm vulnerability "${name}" has no advisory details`);
    }
    observedCounts[record.severity] += 1;
  }

  for (const severity of severities) {
    if (!Number.isInteger(metadataCounts[severity]) || metadataCounts[severity] !== observedCounts[severity]) {
      throw new Error(`npm ${severity} summary does not match vulnerability records`);
    }
  }
  if (metadataCounts.total !== Object.keys(records).length) {
    throw new Error("npm total summary does not match vulnerability records");
  }

  validateExitCode(exitCode, metadataCounts.total, "npm audit");
  const exceptions = scopedExceptions(auditPolicy, "npm", scope, today);
  const usedExceptions = new Set();
  const blocking = new Map();

  for (const [name, record] of Object.entries(records)) {
    if (severityRank.get(record.severity) < severityRank.get("high")) {
      continue;
    }
    const reachable = resolveNpmAdvisories(name, records);
    const actionable = reachable.filter(
      ({ advisory }) =>
        typeof advisory.severity === "string" &&
        severityRank.has(advisory.severity) &&
        severityRank.get(advisory.severity) >= severityRank.get("high"),
    );
    if (actionable.length === 0) {
      throw new Error(`High/critical npm finding "${name}" has no matching high/critical advisory`);
    }

    for (const { advisory, packageName, record: advisoryRecord } of actionable) {
      const id = advisoryId(advisory);
      const packageNameFromAdvisory = requireString(
        advisory.name ?? packageName,
        `npm advisory package for "${name}"`,
      );
      if (packageNameFromAdvisory !== packageName) {
        throw new Error(
          `npm advisory package "${packageNameFromAdvisory}" does not match its record "${packageName}"`,
        );
      }
      if (!id) {
        throw new Error(`High/critical npm advisory for "${packageNameFromAdvisory}" has no stable ID`);
      }
      for (const node of advisoryRecord.nodes) {
        const lockedPackage = packages[node];
        if (!isRecord(lockedPackage) || typeof lockedPackage.version !== "string") {
          throw new Error(`npm advisory ${id} references missing lockfile node "${node}"`);
        }
        const finding = {
          id,
          package: packageNameFromAdvisory,
          version: lockedPackage.version,
        };
        const key = `${finding.id}|${finding.package}|${finding.version}`;
        if (!blocking.has(key)) {
          blocking.set(key, finding);
        }
      }
    }
  }

  for (const finding of blocking.values()) {
    if (!exceptionFor(finding, exceptions, usedExceptions)) {
      throw new Error(
        `Unexpected high/critical npm advisory ${finding.id} ${finding.package}@${finding.version} in ${scope}`,
      );
    }
  }
  ensureNoStaleExceptions(exceptions, usedExceptions);

  const lowerSeverityCount =
    observedCounts.info + observedCounts.low + observedCounts.moderate;
  const messages = [
    `npm audit policy passed for ${scope}: ${blocking.size} high/critical advisory package version(s) covered by exact exceptions.`,
  ];
  if (lowerSeverityCount > 0) {
    messages.push(`${lowerSeverityCount} low/moderate/info npm finding(s) are reported but do not block.`);
  }
  messages.push(...summarizeExceptions(exceptions));
  return messages;
}

function evaluateCargo(report, scope, exitCode, auditPolicy, today) {
  requireRecord(report, "cargo audit report");
  const database = requireRecord(report.database, "cargo audit database");
  if (!Number.isInteger(database["advisory-count"]) || database["advisory-count"] < 1) {
    throw new Error("cargo audit database has no advisory records");
  }
  const lockfile = requireRecord(report.lockfile, "cargo audit lockfile");
  if (!Number.isInteger(lockfile["dependency-count"]) || lockfile["dependency-count"] < 1) {
    throw new Error("cargo audit lockfile has no dependencies");
  }
  const vulnerabilities = requireRecord(report.vulnerabilities, "cargo audit vulnerabilities");
  if (!Array.isArray(vulnerabilities.list)) {
    throw new Error("cargo audit vulnerabilities.list must be an array");
  }
  if (vulnerabilities.found !== (vulnerabilities.list.length > 0)) {
    throw new Error("cargo audit found flag does not match vulnerability records");
  }
  if (typeof vulnerabilities.count === "number" && vulnerabilities.count !== vulnerabilities.list.length) {
    throw new Error("cargo audit vulnerability count does not match its list");
  }
  const settings = requireRecord(report.settings, "cargo audit settings");
  if (!Array.isArray(settings.ignore) || settings.ignore.length !== 0) {
    throw new Error("cargo audit must run without advisory ignore entries");
  }

  const warnings = report.warnings === undefined ? {} : requireRecord(report.warnings, "cargo audit warnings");
  const findings = vulnerabilities.list.map((item) => ({ item, kind: "vulnerability" }));
  const informationalWarnings = [];
  for (const [kind, items] of Object.entries(warnings)) {
    if (!Array.isArray(items)) {
      throw new Error(`cargo audit warning group "${kind}" must be an array`);
    }
    if (kind === "unmaintained" || kind === "notice") {
      informationalWarnings.push(...items.map((item) => ({ item, kind })));
    } else {
      findings.push(...items.map((item) => ({ item, kind })));
    }
  }

  validateExitCode(exitCode, findings.length, "cargo audit");
  const exceptions = scopedExceptions(auditPolicy, "cargo", scope, today);
  const usedExceptions = new Set();
  const blocking = [];
  for (const { item, kind } of findings) {
    requireRecord(item, `cargo ${kind} finding`);
    const advisory = requireRecord(item.advisory, `cargo ${kind} advisory`);
    const packageInfo = requireRecord(item.package, `cargo ${kind} package`);
    const finding = {
      id: requireString(advisory.id, `cargo ${kind} advisory ID`),
      package: requireString(packageInfo.name, `cargo ${kind} package name`),
      version: requireString(packageInfo.version, `cargo ${kind} package version`),
    };
    if (!exceptionFor(finding, exceptions, usedExceptions)) {
      blocking.push({ ...finding, kind });
    }
  }
  if (blocking.length > 0) {
    throw new Error(
      `Unexpected cargo finding(s) in ${scope}: ${blocking
        .map((finding) => `${finding.kind} ${finding.id} ${finding.package}@${finding.version}`)
        .join(", ")}`,
    );
  }
  ensureNoStaleExceptions(exceptions, usedExceptions);

  const messages = [
    `cargo audit policy passed for ${scope}: ${findings.length} blocking finding(s) covered by exact exceptions.`,
  ];
  if (informationalWarnings.length > 0) {
    messages.push(
      `${informationalWarnings.length} unmaintained/notice warning(s) remain non-blocking: ${informationalWarnings
        .map(({ item }) => `${item.advisory?.id ?? "unknown"} ${item.package?.name ?? "unknown"}@${item.package?.version ?? "unknown"}`)
        .join(", ")}.`,
    );
  }
  messages.push(...summarizeExceptions(exceptions));
  return messages;
}

export function evaluateAuditReport({
  ecosystem,
  report,
  lockfile,
  scope,
  auditExitCode,
  auditPolicy = policy,
  today = new Date().toISOString().slice(0, 10),
}) {
  if (ecosystem === "npm") {
    return evaluateNpm(report, lockfile, scope, auditExitCode, auditPolicy, today);
  }
  if (ecosystem === "cargo") {
    return evaluateCargo(report, scope, auditExitCode, auditPolicy, today);
  }
  throw new Error(`Unsupported dependency audit ecosystem "${ecosystem}"`);
}

function parseArguments(args) {
  const [ecosystem, reportPath, scope, exitCodeText, ...options] = args;
  if (!ecosystem || !reportPath || !scope || exitCodeText === undefined) {
    throw new Error(
      "Usage: check-dependency-audit.mjs <npm|cargo> <report.json> <manifest-scope> <audit-exit-code> [--lockfile package-lock.json]",
    );
  }
  const auditExitCode = Number(exitCodeText);
  let lockfilePath;
  for (let index = 0; index < options.length; index += 1) {
    if (options[index] !== "--lockfile" || !options[index + 1] || lockfilePath) {
      throw new Error(`Invalid dependency audit option "${options[index]}"`);
    }
    lockfilePath = options[index + 1];
    index += 1;
  }
  if (ecosystem === "npm" && !lockfilePath) {
    throw new Error("npm audit checks require --lockfile");
  }
  if (ecosystem === "npm" && normalizeRepoPath(lockfilePath) !== normalizeRepoPath(scope)) {
    throw new Error("npm lockfile path must match its declared audit scope");
  }
  if (ecosystem !== "npm" && lockfilePath) {
    throw new Error("--lockfile is only valid for npm audit checks");
  }
  return { ecosystem, reportPath, scope, auditExitCode, lockfilePath };
}

function main() {
  try {
    const args = parseArguments(process.argv.slice(2));
    const report = parseAuditJson(fs.readFileSync(args.reportPath, "utf8"), args.reportPath);
    const lockfile = args.lockfilePath
      ? parseAuditJson(fs.readFileSync(args.lockfilePath, "utf8"), args.lockfilePath)
      : undefined;
    const messages = evaluateAuditReport({
      ecosystem: args.ecosystem,
      report,
      lockfile,
      scope: args.scope,
      auditExitCode: args.auditExitCode,
    });
    for (const message of messages) {
      console.log(message);
    }
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error(`Dependency audit policy failed: ${message}`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
