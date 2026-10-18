// Mach-O inspection for bundling the app's non-system dylibs.
// Reads load commands with `otool -l` and walks the dependency closure the
// way dyld would resolve it (absolute, @rpath, @loader_path,
// @executable_path), so every library the app needs can be copied into
// Contents/Frameworks.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const LOAD_COMMANDS = new Set([
  "LC_LOAD_DYLIB",
  "LC_LOAD_WEAK_DYLIB",
  "LC_REEXPORT_DYLIB",
  "LC_LAZY_LOAD_DYLIB",
  "LC_LOAD_UPWARD_DYLIB",
]);

/** Paths macOS provides (on disk or in the dyld shared cache). */
export function isSystemPath(p) {
  return p.startsWith("/usr/lib/") || p.startsWith("/System/");
}

/** Parse the dylib id, dependencies and rpaths of one Mach-O file. */
export function readMachO(file) {
  const text = execFileSync("otool", ["-l", file], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  const result = { id: null, deps: [], rpaths: [] };
  let cmd = null;
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    const cmdMatch = /^cmd (\S+)$/.exec(line);
    if (cmdMatch) {
      cmd = cmdMatch[1];
      continue;
    }
    const nameMatch = /^name (.+) \(offset \d+\)$/.exec(line);
    if (nameMatch && cmd === "LC_ID_DYLIB") result.id = nameMatch[1];
    if (nameMatch && LOAD_COMMANDS.has(cmd)) {
      result.deps.push({ name: nameMatch[1], weak: cmd === "LC_LOAD_WEAK_DYLIB" });
    }
    const pathMatch = /^path (.+) \(offset \d+\)$/.exec(line);
    if (pathMatch && cmd === "LC_RPATH") result.rpaths.push(pathMatch[1]);
  }
  return result;
}

/** Expand @loader_path / @executable_path in an rpath or dependency name. */
function expand(name, loaderDir, executableDir) {
  if (name.startsWith("@loader_path")) {
    return path.join(loaderDir, name.slice("@loader_path".length));
  }
  if (name.startsWith("@executable_path")) {
    return path.join(executableDir, name.slice("@executable_path".length));
  }
  return name;
}

/**
 * Candidate files for a dependency name. `chain` lists the loading image
 * first, then the images that loaded it, ending at the main executable —
 * the order dyld searches LC_RPATH entries in.
 */
function candidates(name, chain, executableDir) {
  const loaderDir = path.dirname(chain[0].loadPath);
  if (!name.startsWith("@rpath/")) return [expand(name, loaderDir, executableDir)];
  const rest = name.slice("@rpath/".length);
  const out = [];
  for (const image of chain) {
    for (const rpath of image.rpaths) {
      out.push(path.join(expand(rpath, path.dirname(image.loadPath), executableDir), rest));
    }
  }
  return out;
}

/**
 * Walk every non-system dylib the given executables need, transitively,
 * into one shared set. Each root is `{ path, origins }`; `origins` maps
 * names already rewritten in that executable (e.g.
 * `@rpath/libavcodec.62.dylib`) back to the path they were linked against,
 * so a second run over an already-rewritten binary still finds them.
 *
 * Returns `libs`: realpath → { realPath, name, refs: Set<dependency name> },
 * and `refsByImage`: image path → [{ name, realPath }] for every dependency
 * that resolved to a bundled library.
 */
export function collectClosure(roots) {
  const libs = new Map();
  const refsByImage = new Map();
  const unresolved = [];

  const visit = (image, chain, executableDir, origins) => {
    const info = readMachO(image.realPath);
    const node = { loadPath: image.loadPath, rpaths: info.rpaths };
    const nextChain = [node, ...chain];
    const refs = [];
    refsByImage.set(image.realPath, refs);
    for (const dep of info.deps) {
      if (isSystemPath(dep.name)) continue;
      const lookup = image.isExecutable && origins[dep.name] ? origins[dep.name] : dep.name;
      const options = candidates(lookup, nextChain, executableDir);
      const found = options.find((c) => fs.existsSync(c));
      if (!found) {
        // A system library living only in the dyld shared cache (e.g. the
        // Swift runtime via the /usr/lib/swift rpath) has no file on disk.
        if (options.some(isSystemPath) || dep.weak) continue;
        unresolved.push(`${dep.name} (needed by ${image.realPath})`);
        continue;
      }
      if (isSystemPath(found)) continue;
      const realPath = fs.realpathSync(found);
      refs.push({ name: dep.name, realPath });
      if (libs.has(realPath)) {
        libs.get(realPath).refs.add(dep.name);
        continue;
      }
      const id = readMachO(realPath).id;
      const lib = {
        realPath,
        name: path.basename(id ?? lookup),
        refs: new Set([dep.name]),
      };
      libs.set(realPath, lib);
      visit({ realPath, loadPath: found, isExecutable: false }, nextChain, executableDir, origins);
    }
  };

  for (const { path: executable, origins = {} } of roots) {
    const image = { realPath: executable, loadPath: executable, isExecutable: true };
    visit(image, [], path.dirname(executable), origins);
  }
  if (unresolved.length > 0) {
    throw new Error(`Cannot find these libraries:\n  ${unresolved.join("\n  ")}`);
  }
  const byName = new Map();
  for (const lib of libs.values()) {
    const other = byName.get(lib.name);
    if (other) {
      throw new Error(`Two different libraries share the name ${lib.name}:\n  ${other.realPath}\n  ${lib.realPath}`);
    }
    byName.set(lib.name, lib);
  }
  return { libs, refsByImage };
}
