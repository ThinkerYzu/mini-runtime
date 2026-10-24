"""Writes glue/webgl_idl.js from Gecko's WebGL WebIDL (dom/webidl/WebGLRenderingContext.webidl and
WebGL2RenderingContext.webidl in the Firefox tree): the tables glue/webgl.js builds the runtime's WebGL
interfaces from (DESIGN "Phase 4's WebGL", task 23bb).

For each interface (the two contexts, the WebGL object interfaces and the extensions): its constants
with their values, its attributes, and its operations, each with
  - the least number of arguments WebIDL requires (over the overloads), for the "At least N arguments
    required" TypeError a binding throws;
  - what it returns: "undefined" (the call is queued), or the IDL type (the call needs an answer);
  - its argument types when it has one overload (for the conversions a binding does in the caller);
    null when it is overloaded (the bridge's real context picks the overload).
Members behind a pref or a [Func] are kept with it (`pref`), so webgl.js can leave out the ones
release Firefox does not expose.

usage: python3 tools/gen_webgl_idl.py [FIREFOX_TREE]   (or set FIREFOX_SRC)
"""
import json, os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
TREE = sys.argv[1] if len(sys.argv) > 1 else os.environ["FIREFOX_SRC"]
FILES = ["WebGLRenderingContext.webidl", "WebGL2RenderingContext.webidl"]


def strip_comments(text):
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    return re.sub(r"//[^\n]*", "", text)


def split_top(text, sep):
    """`text` split at `sep` outside brackets, parentheses and angle brackets."""
    out, depth, cur = [], 0, ""
    for ch in text:
        if ch in "([<{":
            depth += 1
        elif ch in ")]>}":
            depth -= 1
        if ch == sep and depth == 0:
            out.append(cur)
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur)
    return out


def ext_attrs(text):
    """(the extended attributes in front of `text` as a dict, the rest)."""
    text = text.strip()
    attrs = {}
    while text.startswith("["):
        depth = 0
        for i, ch in enumerate(text):
            depth += ch == "["
            depth -= ch == "]"
            if depth == 0:
                break
        for a in split_top(text[1:i], ","):
            k, _, v = a.strip().partition("=")
            attrs[k.strip()] = v.strip().strip('"') or True
        text = text[i + 1:].strip()
    return attrs, text


def parse_arg(text):
    _, text = ext_attrs(text)
    text = text.split("=")[0].strip()
    optional = text.startswith("optional ")
    if optional:
        text = text[len("optional "):]
    typ, name = text.rsplit(None, 1)
    return {"type": " ".join(typ.split()), "name": name, "optional": optional}


def parse(files):
    interfaces, includes = {}, []
    for f in files:
        text = strip_comments(open(os.path.join(TREE, "dom", "webidl", f)).read())
        for m in re.finditer(r"(\[[^\]]*\]\s*)?(partial\s+)?interface\s+(mixin\s+)?(\w+)\s*(:\s*\w+\s*)?\{(.*?)\};", text, re.S):
            attrs, _ = ext_attrs(m.group(1) or "")
            name = m.group(4)
            iface = interfaces.setdefault(name, {"consts": {}, "attrs": {}, "ops": {}, "pref": attrs.get("Pref"),
                                                 "global": "LegacyNoInterfaceObject" not in attrs and not m.group(3)})
            for member in split_top(m.group(6), ";"):
                mattrs, member = ext_attrs(member)
                member = " ".join(member.split())
                if not member:
                    continue
                pref = mattrs.get("Pref") or mattrs.get("Func")
                c = re.match(r"const \w+ (\w+) = (\S+)$", member)
                if c:
                    iface["consts"][c.group(1)] = int(c.group(2), 0)
                    continue
                a = re.match(r"(readonly )?attribute (.+) (\w+)$", member)
                if a:
                    iface["attrs"][a.group(3)] = {"type": a.group(2), "readonly": bool(a.group(1)), "pref": pref}
                    continue
                o = re.match(r"(.+?) (\w+)\s*\((.*)\)$", member)
                assert o, member
                args = [parse_arg(x) for x in split_top(o.group(3), ",")]
                iface["ops"].setdefault(o.group(2), []).append({"ret": o.group(1), "args": args, "pref": pref})
        for m in re.finditer(r"(\w+) includes (\w+);", text):
            includes.append((m.group(1), m.group(2)))
    for target, mixin in includes:
        src = interfaces[mixin]
        dst = interfaces[target]
        dst["consts"].update(src["consts"])
        dst["attrs"].update(src["attrs"])
        for k, v in src["ops"].items():
            dst["ops"].setdefault(k, []).extend(v)
    return {k: v for k, v in interfaces.items() if not k.endswith("Base")}


def table(iface):
    ops = {}
    for name, overloads in iface["ops"].items():
        least = min(sum(1 for a in o["args"] if not a["optional"]) for o in overloads)
        rets = {o["ret"] for o in overloads}
        assert len(rets) == 1, (name, rets)
        types = [a["type"] for a in overloads[0]["args"]] if len(overloads) == 1 else None
        pref = overloads[0]["pref"]
        ops[name] = [least, rets.pop(), types] + ([pref] if pref else [])
    out = {"consts": iface["consts"], "attrs": {k: [v["type"], v["readonly"]] + ([v["pref"]] if v["pref"] else []) for k, v in iface["attrs"].items()},
           "ops": ops}
    if iface["pref"]:
        out["pref"] = iface["pref"]
    return out


interfaces = parse(FILES)
data = {name: table(iface) for name, iface in interfaces.items()}
# the order the interfaces were declared in, the globals first (WebGLBuffer, ..., the contexts)
globals_ = [n for n, i in interfaces.items() if i["global"]]
extensions = [n for n, i in interfaces.items() if not i["global"]]
lines = [
    "// Generated by tools/gen_webgl_idl.py from Gecko's dom/webidl/WebGLRenderingContext.webidl and",
    "// WebGL2RenderingContext.webidl. Do not edit.",
    "// Each interface: consts {NAME: value}; attrs {name: [type, readonly, pref?]}; ops {name: [the least",
    "// argument count, the return type, the argument types (null when overloaded), pref?]}.",
    "const WEBGL_GLOBALS = " + json.dumps(globals_) + ";",
    "const WEBGL_EXTENSIONS = " + json.dumps(extensions) + ";",
    "const WEBGL_IDL = {",
]
for name in globals_ + extensions:
    lines.append("  " + json.dumps(name) + ": " + json.dumps(data[name], separators=(",", ":")) + ",")
lines.append("};")
open(os.path.join(HERE, "..", "glue", "webgl_idl.js"), "w").write("\n".join(lines) + "\n")
print(len(globals_), "interfaces,", len(extensions), "extensions,",
      sum(len(d["ops"]) for d in data.values()), "operations,", sum(len(d["consts"]) for d in data.values()), "constants")
