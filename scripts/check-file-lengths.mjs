import { promises as fs } from "node:fs";
import path from "node:path";

const root = process.cwd();
const maxLines = Number(process.env.MAX_FILE_LINES ?? 350);
const includeExtensions = new Set([".rs", ".ts", ".tsx", ".js", ".jsx", ".swift"]);
const includeRoots = [
  path.join(root, "src"),
  path.join(root, "src-tauri", "src"),
  path.join(root, "src-tauri", "vault-helper", "src"),
  path.join(root, "src-tauri", "vault-helper", "tests"),
];

async function walk(dir) {
  const entries = await fs.readdir(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await walk(fullPath)));
      continue;
    }
    if (!includeExtensions.has(path.extname(entry.name))) {
      continue;
    }
    files.push(fullPath);
  }
  return files;
}

async function countLines(filePath) {
  const text = await fs.readFile(filePath, "utf8");
  if (text.length === 0) {
    return 0;
  }
  return text.split(/\r?\n/).length;
}

const allFiles = (
  await Promise.all(
    includeRoots.map(async (dir) => {
      try {
        return await walk(dir);
      } catch {
        return [];
      }
    }),
  )
).flat();

const violations = [];
for (const filePath of allFiles) {
  const lines = await countLines(filePath);
  if (lines > maxLines) {
    violations.push({
      filePath,
      lines,
      relativePath: path.relative(root, filePath),
    });
  }
}

violations.sort((left, right) => right.lines - left.lines || left.relativePath.localeCompare(right.relativePath));

if (violations.length === 0) {
  console.log(`All repo-owned source files are at or under ${maxLines} lines.`);
  process.exit(0);
}

console.error(`Found ${violations.length} source files over ${maxLines} lines:\n`);
for (const violation of violations) {
  console.error(`${String(violation.lines).padStart(5, " ")}  ${violation.relativePath}`);
}

process.exit(1);
