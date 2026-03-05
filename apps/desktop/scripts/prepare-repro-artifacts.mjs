import { access, copyFile, mkdir } from "node:fs/promises";
import path from "node:path";

const outputDirArg = process.argv[2] ?? "artifacts/reproducibility";
const outputDir = path.resolve(process.cwd(), outputDirArg);

async function ensureFile(filePath) {
  try {
    await access(filePath);
    return true;
  } catch {
    return false;
  }
}

async function copyRequired(sourceRelativePath, targetName) {
  const sourcePath = path.resolve(process.cwd(), sourceRelativePath);
  if (!(await ensureFile(sourcePath))) {
    throw new Error(`Required artifact is missing: ${sourceRelativePath}`);
  }

  const targetPath = path.join(outputDir, targetName);
  await copyFile(sourcePath, targetPath);
  return targetPath;
}

await mkdir(outputDir, { recursive: true });

const copied = [];
copied.push(await copyRequired("package.json", "package.json"));
copied.push(await copyRequired("bun.lock", "bun.lock"));
copied.push(await copyRequired("src-tauri/Cargo.lock", "Cargo.lock"));
copied.push(await copyRequired("build/index.html", "frontend-index.html"));

const backendCandidates = [
  "src-tauri/target/release/appsdesktop",
  "src-tauri/target/release/appsdesktop.exe",
];

let backendSource = null;
for (const candidate of backendCandidates) {
  const resolved = path.resolve(process.cwd(), candidate);
  if (await ensureFile(resolved)) {
    backendSource = resolved;
    break;
  }
}

if (!backendSource) {
  throw new Error("No backend release binary found (appsdesktop or appsdesktop.exe).");
}

const backendTarget = path.join(outputDir, "backend-binary");
await copyFile(backendSource, backendTarget);
copied.push(backendTarget);

console.log("Prepared reproducibility artifacts:");
for (const artifact of copied) {
  console.log(`- ${artifact}`);
}
