import { readFile } from "node:fs/promises";
import path from "node:path";

const metricsPath = process.argv[2] ?? "benchmarks/latest.json";
const resolvedMetricsPath = path.resolve(process.cwd(), metricsPath);

const metrics = JSON.parse(await readFile(resolvedMetricsPath, "utf8"));

const thresholds = {
  startup_p95_ms: 1000,
  memory_steady_mb: 50,
  search_p95_ms: 300,
};

const failures = [];

for (const [metricName, threshold] of Object.entries(thresholds)) {
  const metricValue = Number(metrics[metricName]);

  if (Number.isNaN(metricValue)) {
    failures.push(`${metricName} is missing or invalid.`);
    continue;
  }

  if (metricValue > threshold) {
    failures.push(`${metricName}=${metricValue} exceeds threshold ${threshold}.`);
  }
}

console.log("Quality gate thresholds:", thresholds);
console.log("Measured metrics:", {
  startup_p95_ms: metrics.startup_p95_ms,
  memory_steady_mb: metrics.memory_steady_mb,
  search_p95_ms: metrics.search_p95_ms,
  profile: metrics.profile,
  hardware_profile: metrics.hardware_profile,
});

if (failures.length > 0) {
  console.error("Quality gates FAILED:");
  for (const failure of failures) {
    console.error(`- ${failure}`);
  }
  process.exit(1);
}

console.log("Quality gates passed.");
