// Tauri `beforeBundleCommand` (macOS only): make SOURCE.app independent of
// Homebrew. Copies every non-system dylib the app binary needs (FFmpeg,
// Tesseract, Leptonica, libarchive and everything they pull in), plus the
// `ffmpeg` command-line tool the app runs for device listing and audio
// capture, into src-tauri/target/bundled-macos. Rewrites their install
// names and references to @rpath, signs them, and points the compiled
// binary at @executable_path/../Frameworks. tauri.conf.json's
// `bundle.macOS.files` copies the staged files into SOURCE.app
// (Contents/Frameworks and Contents/MacOS/ffmpeg), and the bundler then
// signs the app itself.
//
// Without this, a `brew upgrade` or `brew cleanup` that removes a library
// the app was linked against (libx265.215.dylib on 2026-10-18) makes the
// app crash at launch.
//
// Usage: node scripts/bundle-macos-dylibs.mjs [path/to/binary]
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { collectClosure, isSystemPath, readMachO } from "./macos-dylibs/macho.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const TAURI_DIR = path.join(ROOT, "src-tauri");
const STAGE_DIR = path.join(TAURI_DIR, "target", "bundled-macos");
const FRAMEWORKS_DIR = path.join(STAGE_DIR, "Frameworks");
const STAGED_CLI = path.join(STAGE_DIR, "ffmpeg");
const FRAMEWORKS_RPATH = "@executable_path/../Frameworks";
const BINARY_NAME = "SOURCE";

function run(cmd, args) {
  execFileSync(cmd, args, { stdio: ["ignore", "ignore", "pipe"] });
}

/** The binary `tauri build` is about to bundle. */
function findBinary() {
  if (process.argv[2]) return path.resolve(process.argv[2]);
  const targetDir = process.env.CARGO_TARGET_DIR
    ? path.resolve(TAURI_DIR, process.env.CARGO_TARGET_DIR)
    : path.join(TAURI_DIR, "target");
  const profile = process.env.TAURI_ENV_DEBUG === "true" ? "debug" : "release";
  const triple = process.env.TAURI_ENV_TARGET_TRIPLE;
  const options = [
    triple && path.join(targetDir, triple, profile, BINARY_NAME),
    path.join(targetDir, profile, BINARY_NAME),
  ].filter(Boolean);
  const found = options.find((p) => fs.existsSync(p));
  if (!found) throw new Error(`App binary not found; looked in:\n  ${options.join("\n  ")}`);
  return found;
}

/** The `ffmpeg` CLI from the same install as the libavcodec the app linked. */
function findFfmpegCli(binary, origins) {
  const dep = readMachO(binary).deps.find((d) => /\/libavcodec\.[^/]*dylib$/.test(d.name));
  if (!dep) throw new Error("The app binary does not link libavcodec");
  const linked = origins[dep.name] ?? dep.name;
  const cli = path.join(path.dirname(fs.realpathSync(linked)), "..", "bin", "ffmpeg");
  if (!fs.existsSync(cli)) throw new Error(`ffmpeg CLI not found at ${cli}`);
  return fs.realpathSync(cli);
}

/** Same identity the Tauri bundler uses for the app. */
function signingIdentity() {
  if (process.env.APPLE_SIGNING_IDENTITY) return process.env.APPLE_SIGNING_IDENTITY;
  const conf = JSON.parse(fs.readFileSync(path.join(TAURI_DIR, "tauri.conf.json"), "utf8"));
  return conf.bundle?.macOS?.signingIdentity ?? "-";
}

/**
 * Where each rewritten reference in the binary originally pointed. Kept
 * next to the binary so a rerun on an unchanged (already rewritten) binary
 * can still find the libraries; a relink restores absolute paths anyway.
 */
function originsFile(binary) {
  return path.join(path.dirname(binary), `.${path.basename(binary)}.dylib-origins.json`);
}

function readOrigins(binary) {
  try {
    return JSON.parse(fs.readFileSync(originsFile(binary), "utf8"));
  } catch {
    return {};
  }
}

/** install_name_tool arguments that point `image`'s references at @rpath. */
function changeArgs(refs, libs) {
  const args = [];
  for (const ref of refs) {
    const target = `@rpath/${libs.get(ref.realPath).name}`;
    if (ref.name !== target) args.push("-change", ref.name, target);
  }
  return args;
}

function stageLibrary(lib, libs, refsByImage, identity) {
  const dest = path.join(FRAMEWORKS_DIR, lib.name);
  fs.copyFileSync(lib.realPath, dest);
  fs.chmodSync(dest, 0o755);
  const info = readMachO(dest);
  // Drop every rpath: a leftover /opt/homebrew rpath would let dyld load
  // Homebrew's copy instead of the bundled one.
  const args = ["-id", `@rpath/${lib.name}`, ...changeArgs(refsByImage.get(lib.realPath), libs)];
  for (const rpath of new Set(info.rpaths)) args.push("-delete_rpath", rpath);
  run("install_name_tool", [...args, dest]);
  try {
    run("install_name_tool", ["-add_rpath", "@loader_path", dest]);
  } catch {
    // No header room; the app binary's rpath still covers this library.
  }
  run("codesign", ["--force", "--sign", identity, "--timestamp=none", dest]);
}

/** Point an executable in Contents/MacOS at the bundled libraries. */
function rewriteExecutable(file, libs, refs, identity) {
  const info = readMachO(file);
  const args = changeArgs(refs, libs);
  for (const rpath of new Set(info.rpaths)) {
    if (!rpath.startsWith("@") && !isSystemPath(rpath)) args.push("-delete_rpath", rpath);
  }
  if (!info.rpaths.includes(FRAMEWORKS_RPATH)) args.push("-add_rpath", FRAMEWORKS_RPATH);
  if (args.length === 0) return;
  run("install_name_tool", [...args, file]);
  // install_name_tool voids the existing signature; arm64 refuses to run
  // unsigned code.
  run("codesign", ["--force", "--sign", identity, "--timestamp=none", file]);
}

function rewriteBinary(binary, libs, refs, origins) {
  const nextOrigins = {};
  for (const ref of refs) {
    const name = `@rpath/${libs.get(ref.realPath).name}`;
    nextOrigins[name] = ref.name.startsWith("@rpath/") ? origins[ref.name] ?? ref.name : ref.name;
  }
  fs.writeFileSync(originsFile(binary), `${JSON.stringify(nextOrigins, null, 2)}\n`);
  // Ad-hoc is enough here: the bundler re-signs the bundled copy.
  rewriteExecutable(binary, libs, refs, "-");
}

/** Fail the build if anything staged still points outside the app. */
function verify(binary) {
  const problems = [];
  const libFiles = fs.readdirSync(FRAMEWORKS_DIR).map((f) => path.join(FRAMEWORKS_DIR, f));
  const files = [binary, STAGED_CLI, ...libFiles];
  for (const file of files) {
    const info = readMachO(file);
    for (const dep of info.deps) {
      if (!isSystemPath(dep.name) && !dep.name.startsWith("@rpath/")) {
        problems.push(`${path.basename(file)} → ${dep.name}`);
      }
    }
    for (const rpath of info.rpaths) {
      if (!rpath.startsWith("@") && !isSystemPath(rpath)) {
        problems.push(`${path.basename(file)} rpath ${rpath}`);
      }
    }
  }
  if (problems.length > 0) {
    throw new Error(`Non-bundled references remain:\n  ${problems.join("\n  ")}`);
  }
}

function main() {
  const platform = process.env.TAURI_ENV_PLATFORM ?? process.platform;
  if (platform !== "darwin" || process.platform !== "darwin") return;
  const binary = findBinary();
  const identity = signingIdentity();
  const origins = readOrigins(binary);
  const cli = findFfmpegCli(binary, origins);

  fs.rmSync(STAGE_DIR, { recursive: true, force: true });
  fs.mkdirSync(FRAMEWORKS_DIR, { recursive: true });
  fs.copyFileSync(cli, STAGED_CLI);
  fs.chmodSync(STAGED_CLI, 0o755);
  const { libs, refsByImage } = collectClosure([
    { path: binary, origins },
    { path: STAGED_CLI },
  ]);
  for (const lib of libs.values()) stageLibrary(lib, libs, refsByImage, identity);
  rewriteExecutable(STAGED_CLI, libs, refsByImage.get(STAGED_CLI), identity);
  rewriteBinary(binary, libs, refsByImage.get(binary), origins);
  verify(binary);

  const bytes = [...libs.values()].reduce((n, lib) => n + fs.statSync(lib.realPath).size, 0);
  console.log(
    `Bundled ${libs.size} dylibs (${(bytes / 1048576).toFixed(1)} MB) and the ffmpeg CLI from ${cli}, signed with "${identity}"`,
  );
}

try {
  main();
} catch (error) {
  console.error(`bundle-macos-dylibs: ${error.message}`);
  process.exit(1);
}
