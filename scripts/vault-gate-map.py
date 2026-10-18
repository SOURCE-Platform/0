#!/usr/bin/env python3
"""Map the vault's §16/§19 test IDs to the tests that carry them.

Final review SPEC-B1 / VER-B3: a gate must assert every required ID by
test name, so a missing test fails the gate instead of passing it
vacuously. This script reads the test sources (it runs nothing) and prints
one line per (ID, runner, test):

    ID <TAB> runner <TAB> test-name

runner is `cargo:<package>:<--lib|--test NAME>`, or `node:<file>`.
An ID is taken from the doc comment above a test, from the test's name
(`br_01_…`, `da02_…` → BR-01, DA-02) and from its body (assert messages).

Usage: python3 scripts/vault-gate-map.py [--ids]   (--ids: IDs only)
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ID = re.compile(r"\b(PA|PS|AU|MT|IO|SG|CX|PV|PW|FFI|RE|BR|DA|IM|NM|UI|CS|SC|RG|CP|BK|SY|TR|ST|RC|FR|RF|HC|RU|KD|RL|EV|CR|PR|XV|OP|SH|IC)-(\d{2}M?)\b")
NAME_ID = re.compile(r"(?:^|_)(pa|ps|au|mt|sg|cx|pw|re|br|da|im|nm|rg|cp|bk|sy|tr|st|rc|fr|rf|hc|ru|kd|rl|ev|cr|pr|sc|op)_?(\d{2})(?=_|$)")
CRATES = ["vault-helper", "vault-engine", "vault-proto", "vault-provider-core", "vault-coordinator", "vault-ffi", "vault-tests", "vault-nm-host"]


def package_of(crate_dir: Path) -> str:
    text = (crate_dir / "Cargo.toml").read_text()
    return re.search(r'^name\s*=\s*"([^"]+)"', text, re.M).group(1)


RANGE = re.compile(r"\b([A-Z]{2,3})-(\d{2})\s*(?:…|\.\.\.?|–)\s*(?:\1-)?(\d{2})\b")
ALT = re.compile(r"\b([A-Z]{2,3})-(\d{2})((?:/\d{2})+)\b")


def ids_in(text: str) -> set:
    """IDs, with ranges (`PA-01…05`) and alternatives (`PW-07/09`) expanded."""
    out = {f"{a}-{b}" for a, b in ID.findall(text)}
    for p, a, b in RANGE.findall(text):
        out |= {f"{p}-{i:02d}" for i in range(int(a), int(b) + 1)}
    for p, a, rest in ALT.findall(text):
        out |= {f"{p}-{a}"} | {f"{p}-{x}" for x in rest.strip("/").split("/")}
    out = {i for i in out if ID.fullmatch(i)}
    if "XV-PEER" in text:
        out.add("XV-PEER")
    return out


def rust_tests(path: Path):
    """Yield (fn_name, ids) for every #[test] in a Rust file."""
    lines = path.read_text().splitlines()
    # A file's `//!` header names what all of its tests cover together.
    header = ids_in("\n".join(l for l in lines if l.lstrip().startswith("//!")))
    comment, pending, i = [], False, 0
    while i < len(lines):
        line = lines[i].strip()
        if line.startswith("//"):
            comment.append(line)
        elif line.startswith("#[test]"):
            pending = True
        elif line.startswith("#["):
            pass  # #[ignore], #[should_panic], …
        elif pending and (m := re.match(r"(?:pub\s+)?(?:async\s+)?fn\s+(\w+)", line)):
            name, depth, body = m.group(1), 0, []
            j = i
            while j < len(lines):  # the body, by brace depth
                body.append(lines[j])
                depth += lines[j].count("{") - lines[j].count("}")
                if depth <= 0 and "{" in "".join(body):
                    break
                j += 1
            ids = header | ids_in("\n".join(comment)) | ids_in("\n".join(body))
            ids |= {f"{a.upper()}-{b}" for a, b in NAME_ID.findall(name)}
            yield name, ids
            comment, pending, i = [], False, j
        else:
            if line:
                comment = []
        i += 1


def node_tests(path: Path):
    for m in re.finditer(r'\btest\(\s*"([^"]+)"', path.read_text()):
        yield m.group(1), ids_in(m.group(1))


def rows():
    src = ROOT / "src-tauri"
    for crate in CRATES:
        d = src / crate
        if not (d / "Cargo.toml").exists():
            continue
        pkg = package_of(d)
        for f in sorted((d / "tests").glob("*.rs")) if (d / "tests").exists() else []:
            for name, ids in rust_tests(f):
                for i in ids:
                    yield i, f"cargo:{pkg}:--test {f.stem}", name
        for f in sorted((d / "src").rglob("*.rs")):
            for name, ids in rust_tests(f):
                for i in ids:
                    yield i, f"cargo:{pkg}:--lib", name
    for f in sorted((ROOT / "browser-extension/test").glob("*.mjs")):
        for name, ids in node_tests(f):
            for i in ids:
                yield i, f"node:{f.relative_to(ROOT)}", name


def main():
    out = sorted(set(rows()))
    if "--ids" in sys.argv:
        print("\n".join(sorted({r[0] for r in out})))
    else:
        for r in out:
            print("\t".join(r))


if __name__ == "__main__":
    main()
