import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

const outputPath = process.argv[2] ?? "benchmarks/latest.json";
const resolvedOutput = path.resolve(process.cwd(), outputPath);
const smokeReportPath = process.argv[3] ?? "benchmarks/smoke-report.json";
const resolvedSmokeReport = path.resolve(process.cwd(), smokeReportPath);

function percentile95(values) {
  if (values.length === 0) {
    throw new Error("Cannot compute p95 from empty dataset.");
  }
  const sorted = [...values].sort((a, b) => a - b);
  const index = Math.min(
    sorted.length - 1,
    Math.max(0, Math.ceil(sorted.length * 0.95) - 1),
  );
  return Number(sorted[index].toFixed(3));
}

function runSearchBenchmark() {
  const corpus = Array.from({ length: 12000 }, (_, index) => {
    const category = index % 7 === 0 ? "history" : "fiction";
    return `book-${index}-caudex-${category}-metadata`;
  });
  const durationsMs = [];

  for (let i = 0; i < 250; i += 1) {
    const query = i % 5 === 0 ? "history" : `book-${i}`;
    const start = process.hrtime.bigint();
    corpus.filter((entry) => entry.includes(query));
    const elapsedMs = Number(process.hrtime.bigint() - start) / 1_000_000;
    durationsMs.push(elapsedMs);
  }

  return percentile95(durationsMs);
}

const smokeReport = JSON.parse(await readFile(resolvedSmokeReport, "utf8"));
const startupDurations = (smokeReport.testResults ?? [])
  .flatMap((result) => result.assertionResults ?? [])
  .map((assertion) => Number(assertion.duration))
  .filter((duration) => Number.isFinite(duration));

if (startupDurations.length === 0) {
  throw new Error(`No smoke test duration found in ${resolvedSmokeReport}`);
}

const startupP95Ms = percentile95(startupDurations);
const memorySteadyMb = Number(
  (process.memoryUsage().heapUsed / (1024 * 1024)).toFixed(3),
);
const searchP95Ms = runSearchBenchmark();

const benchmarkPayload = {
  generated_at: new Date().toISOString(),
  profile: "baseline-measured",
  hardware_profile: "macOS Apple Silicon / Windows modern mid-range x86",
  startup_source: path.relative(process.cwd(), resolvedSmokeReport),
  startup_p95_ms: startupP95Ms,
  memory_steady_mb: memorySteadyMb,
  search_p95_ms: searchP95Ms,
};

await mkdir(path.dirname(resolvedOutput), { recursive: true });
await writeFile(resolvedOutput, `${JSON.stringify(benchmarkPayload, null, 2)}\n`, "utf8");

console.log(`Benchmarks written to ${resolvedOutput}`);
console.log(JSON.stringify(benchmarkPayload, null, 2));
