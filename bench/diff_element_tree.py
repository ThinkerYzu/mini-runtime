"""Where two element trees part: Firefox's (firefox_probe.py's output, its last line) against the
runtime's (the `console: TREE` line of mini_realsites.py's log), both from probe_element_tree.js.

Each level's children are aligned by signature (difflib); an element on one side only is printed
with its subtree to DEPTH levels and its element count. Equal subtrees are skipped. A node moved
elsewhere shows as a delete on one side and an insert on the other with the same subtree.

usage: python3 diff_element_tree.py FIREFOX_PROBE.txt MINI.log [DEPTH]  (default 4)
"""
import difflib, json, sys


def count(t):
    return 1 + sum(count(c) for c in kids(t))


def kids(t):
    return t[2] if len(t) > 2 else []


def show(t, depth, limit):
    if depth > limit:
        return
    print("    " * depth + t[0], repr(t[1]) if t[1] else "", f"({count(t)})")
    for c in kids(t):
        show(c, depth + 1, limit)


def walk(a, b, path, limit):
    if json.dumps(a) == json.dumps(b):
        return
    ka, kb = kids(a), kids(b)
    sm = difflib.SequenceMatcher(None, [c[0] for c in ka], [c[0] for c in kb], autojunk=False)
    for op, i1, i2, j1, j2 in sm.get_opcodes():
        if op == "equal":
            for x, y in zip(ka[i1:i2], kb[j1:j2]):
                walk(x, y, path + [x[0]], limit)
            continue
        print("== at", " > ".join(p[:50] for p in path[-6:]), op)
        for x in ka[i1:i2]:
            print("  Firefox only:")
            show(x, 2, limit)
        for y in kb[j1:j2]:
            print("  runtime only:")
            show(y, 2, limit)


def main():
    firefox = json.loads(open(sys.argv[1]).read().strip().splitlines()[-1].split(" ", 1)[1])
    tag = "console: TREE "
    mini = json.loads([l for l in open(sys.argv[2], errors="replace") if l.startswith(tag)][-1][len(tag):])
    limit = int(sys.argv[3]) if len(sys.argv) > 3 else 4
    print("elements: Firefox", count(firefox), "runtime", count(mini))
    walk(firefox, mini, [firefox[0]], limit)


if __name__ == "__main__":
    main()
