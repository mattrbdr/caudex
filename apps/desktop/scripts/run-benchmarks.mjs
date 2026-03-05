import { mkdir, readFile, writeFile } from "node:fs/promises";
import { execFile } from "node:child_process";
import path from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

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

async function runSearchBenchmark() {
  const { stdout } = await execFileAsync(
    "cargo",
    [
      "run",
      "--quiet",
      "--manifest-path",
      "src-tauri/Cargo.toml",
      "--bin",
      "search_benchmark",
      "--",
      "--corpus",
      "2000",
      "--queries",
      "250",
    ],
    {
      cwd: process.cwd(),
      maxBuffer: 10 * 1024 * 1024,
    },
  );

  const lines = stdout
    .split(/\r?\n/g)
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
  if (lines.length === 0) {
    throw new Error("No benchmark payload emitted by search_benchmark binary.");
  }

  const payload = JSON.parse(lines[lines.length - 1]);
  const searchP95 = Number(payload.search_p95_ms);
  if (!Number.isFinite(searchP95)) {
    throw new Error("search_benchmark returned an invalid search_p95_ms.");
  }

  return Number(searchP95.toFixed(3));
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
const searchP95Ms = await runSearchBenchmark();

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
