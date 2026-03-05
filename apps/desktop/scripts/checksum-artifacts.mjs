import { mkdir, readFile, writeFile } from "node:fs/promises";
import crypto from "node:crypto";
import path from "node:path";

function hashBuffer(buffer) {
  return crypto.createHash("sha256").update(buffer).digest("hex");
}

async function generate(outFile, files) {
  if (files.length === 0) {
    throw new Error("No files provided for checksum generation.");
  }

  const lines = [];
  const resolvedOut = path.resolve(process.cwd(), outFile);
  const manifestDir = path.dirname(resolvedOut);
  await mkdir(manifestDir, { recursive: true });

  for (const file of files) {
    const resolved = path.resolve(process.cwd(), file);
    const contents = await readFile(resolved);
    const hash = hashBuffer(contents);
    const relativePath = path
      .relative(manifestDir, resolved)
      .split(path.sep)
      .join("/");
    lines.push(`${hash}  ${relativePath}`);
  }

  await writeFile(resolvedOut, `${lines.join("\n")}\n`, "utf8");
  console.log(`Checksums generated at ${resolvedOut}`);
}

async function verify(manifestPath) {
  const resolvedManifest = path.resolve(process.cwd(), manifestPath);
  const manifestDir = path.dirname(resolvedManifest);
  const manifest = await readFile(resolvedManifest, "utf8");

  const lines = manifest
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);

  for (const line of lines) {
    const [expectedHash, fileName] = line.split(/\s{2,}/);
    if (!expectedHash || !fileName) {
      throw new Error(`Invalid checksum manifest entry: ${line}`);
    }

    const filePath = path.resolve(manifestDir, fileName);
    const contents = await readFile(filePath);
    const actualHash = hashBuffer(contents);

    if (actualHash !== expectedHash) {
      throw new Error(`Checksum mismatch for ${fileName}`);
    }
  }

  console.log(`Checksum verification passed for ${lines.length} file(s).`);
}

const [mode, ...rest] = process.argv.slice(2);

if (mode === "generate") {
  const [outFile, ...files] = rest;
  if (!outFile) {
    throw new Error("Usage: checksum-artifacts.mjs generate <output-file> <file> [...files]");
  }
  await generate(outFile, files);
} else if (mode === "verify") {
  const [manifestPath] = rest;
  if (!manifestPath) {
    throw new Error("Usage: checksum-artifacts.mjs verify <manifest-file>");
  }
  await verify(manifestPath);
} else {
  throw new Error("Usage: checksum-artifacts.mjs <generate|verify> ...");
}
