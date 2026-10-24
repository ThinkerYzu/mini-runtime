#!/usr/bin/env python3
"""List which translated functions changed in Firefox since the revision they were translated from.

usage: gecko-drift.py [--tree PATH] [--new-rev REV] [--show]

Reads the `// Gecko: <path> <qualified name>` markers in src/**/*.rs (see CONVENTIONS.md). The revision
of a marker is the file's first-line `// Gecko-Rev: <sha>` if there is one, else GECKO_REV. For each
marker it takes the C++ definition of the name from the file at that revision and from the new one
(the working tree of --tree, or --new-rev in it), and compares them with whitespace normalised.
Exit status 1 if anything changed or went missing.
"""
import argparse, difflib, os, re, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)


def git_show(tree, rev, path):
    r = subprocess.run(["git", "-C", tree, "show", f"{rev}:{path}"], capture_output=True, text=True)
    return r.stdout if r.returncode == 0 else None


def read_new(tree, new_rev, path):
    if new_rev:
        return git_show(tree, new_rev, path)
    p = os.path.join(tree, path)
    return open(p, encoding="utf-8", errors="replace").read() if os.path.exists(p) else None


def match_brace(text, i):
    """text[i] is '{'; return the index after its matching '}' (skips strings, chars and comments)."""
    depth, n = 0, len(text)
    while i < n:
        c = text[i]
        if text.startswith("//", i):
            i = text.find("\n", i)
            if i < 0:
                return n
        elif text.startswith("/*", i):
            i = text.find("*/", i) + 2
            continue
        elif c in "\"'":
            q = c
            i += 1
            while i < n and text[i] != q:
                i += 2 if text[i] == "\\" else 1
        elif c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return n


def match_paren(text, i):
    depth = 0
    while i < len(text):
        if text[i] == "(":
            depth += 1
        elif text[i] == ")":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return len(text)


def extract(text, qualified):
    """The C++ definition of a qualified name, or None: a class, struct or enum with its body, a
    function with its body, a `#define`, an initialised constant, `using` or `typedef`, and as a last
    resort the first line that mentions the name (for enumerators and the like)."""
    parts = qualified.split("::")
    name = parts[-1]
    # a class, struct or enum definition (`struct MOZ_FOO nsStyleDisplay {`, `enum class UsedFloat : uint8_t {`)
    m = re.search(r"\b(?:class|struct|enum(?:\s+class)?)\s+(?:[A-Z][A-Z_0-9]+\s+)*(?:\w+::)*" + re.escape(name) + r"\b[^;{(]*\{", text)
    if m:
        end = match_brace(text, m.end() - 1)
        if text.startswith("enum", m.start()):
            semi = text.find(";", end)
            end = semi + 1 if semi >= 0 else end
        return text[m.start():end]
    pat = re.compile(r"(?<![\w:~])((?:\w+::)*)" + re.escape(name) + r"\s*\(")
    want = "::".join(parts[:-1])
    for m in pat.finditer(text):
        qual = m.group(1).rstrip(":")
        # a written qualifier must be a suffix of the wanted one; none is fine for a definition
        # inside the class body in a header.
        if qual and not (want == qual or want.endswith("::" + qual)):
            continue
        j = match_paren(text, m.end() - 1)
        k = j
        while k < len(text) and text[k] not in "{;":
            k += 1
        if k < len(text) and text[k] == "{":
            return text[m.start():match_brace(text, k)]
    m = re.search(r"^[ \t]*#\s*define\s+" + re.escape(name) + r"\b(?:.*\\\n)*.*$", text, re.M)
    if m:
        return m.group(0)
    m = re.search(r"(?<![\w:])(?:using\s+|typedef\s+[^;=]*?)?" + re.escape(name) + r"\s*(?:=|;)[^;]*;", text)
    if m:
        start = text.rfind("\n", 0, m.start()) + 1
        return text[start:m.end()]
    m = re.search(r"^.*(?<![\w:])" + re.escape(name) + r"\b.*$", text, re.M)
    return m.group(0) if m else None


def normalise(s):
    return re.sub(r"\s+", " ", s).strip()


def markers():
    default_rev = open(os.path.join(ROOT, "GECKO_REV")).read().strip()
    for dirpath, _, files in os.walk(os.path.join(ROOT, "src")):
        for f in sorted(files):
            if not f.endswith(".rs"):
                continue
            p = os.path.join(dirpath, f)
            lines = open(p, encoding="utf-8").read().splitlines()
            rev = default_rev
            if lines and lines[0].startswith("// Gecko-Rev:"):
                rev = lines[0].split(":", 1)[1].strip()
            for n, l in enumerate(lines, 1):
                m = re.match(r"\s*// Gecko: (\S+) ([\w:~]+)\s*$", l)
                if m:
                    yield os.path.relpath(p, ROOT), n, rev, m.group(1), m.group(2)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tree", default=os.environ.get("FIREFOX_SRC"))
    ap.add_argument("--new-rev", default=None)
    ap.add_argument("--show", action="store_true", help="print a diff for each changed function")
    a = ap.parse_args()
    if not a.tree:
        ap.error("pass --tree or set FIREFOX_SRC to a Firefox source checkout")
    counts = {"same": 0, "changed": 0, "missing": 0, "unreadable": 0}
    for rs, line, rev, path, name in markers():
        old = git_show(a.tree, rev, path)
        new = read_new(a.tree, a.new_rev, path)
        if old is None or new is None:
            counts["unreadable"] += 1
            print(f"UNREADABLE {rs}:{line} {path} {name} (rev {rev[:12]})")
            continue
        o, n = extract(old, name), extract(new, name)
        if o is None:
            counts["unreadable"] += 1
            print(f"NOT-FOUND-IN-OLD {rs}:{line} {path} {name}")
        elif n is None:
            counts["missing"] += 1
            print(f"MISSING {rs}:{line} {path} {name}")
        elif normalise(o) != normalise(n):
            counts["changed"] += 1
            print(f"CHANGED {rs}:{line} {path} {name}")
            if a.show:
                sys.stdout.writelines(difflib.unified_diff(o.splitlines(True), n.splitlines(True), "old", "new"))
        else:
            counts["same"] += 1
    print(", ".join(f"{k} {v}" for k, v in counts.items()))
    return 1 if counts["changed"] or counts["missing"] else 0


if __name__ == "__main__":
    sys.exit(main())
