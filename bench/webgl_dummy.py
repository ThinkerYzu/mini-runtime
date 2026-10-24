"""Detached WebGL sink for an HTTP viewer without an API process (the protocol tests).
No shaders execute; pixel outputs are zero. With `webgl` routed to mrt-api the viewer
takes its real contexts' answers instead (GAPS B169).

Query answers are provisional, not GPU capabilities. The HTTP viewer keeps the
commands separately so an attached browser can reconstruct the real GL state.
"""
import base64
import struct
import zlib

MAX_OUTPUT = 8 * 1024 * 1024
ELEMENT_BYTES = {"Int8Array": 1, "Uint8Array": 1, "Uint8ClampedArray": 1,
                 "Int16Array": 2, "Uint16Array": 2, "Int32Array": 4, "Uint32Array": 4,
                 "Float32Array": 4, "Float64Array": 8, "BigInt64Array": 8, "BigUint64Array": 8, "DataView": 1}


class DummyWebGL:
    def __init__(self):
        self.contexts = {}
        self.objects = set()

    def execute(self, req):
        result, outs, lost = None, [], set()
        for cid, ext, method, args, made in req["cmds"]:
            result, outs = {"$u": 1}, []
            if method == "$create":
                attrs = dict(alpha=True, antialias=True, depth=True, stencil=False,
                             premultipliedAlpha=True, preserveDrawingBuffer=False,
                             failIfMajorPerformanceCaveat=False, powerPreference="default")
                attrs.update(args[1])
                self.contexts[cid] = {"size": list(args[2:4]), "attrs": attrs, "type": args[0]}
                result = {"attrs": attrs, "exts": ["WEBGL_lose_context"]}
                continue
            c = self.contexts.get(cid)
            if c is None:
                lost.add(cid)
                result = None
                continue
            if made:
                self.objects.add(made)
            if method == "$drop":
                self.contexts.pop(cid, None)
            elif method == "$size":
                c["size"] = list(args[:2])
            elif method == "$dbsize":
                result = c["size"]
            elif method == "$dataURL":
                result = self.black_png(*c["size"])
            elif method == "getUniformLocation":
                result = {"$o": made, "$t": "WebGLUniformLocation"}
            elif method == "getShaderPrecisionFormat":
                result = {"$pf": [127, 127, 23]}
            elif method in ("getShaderInfoLog", "getProgramInfoLog", "getShaderSource"):
                result = ""
            elif method == "getShaderParameter":
                result = True if args[1] in (0x8B80, 0x8B81) else 0
            elif method == "getProgramParameter":
                result = True if args[1] in (0x8B80, 0x8B82, 0x8B83) else 0
            elif method in ("getActiveAttrib", "getActiveUniform", "getFramebufferAttachmentParameter"):
                result = None
            elif method == "getAttachedShaders":
                result = []
            elif method == "checkFramebufferStatus":
                result = 0x8CD5
            elif method.startswith("is"):
                result = bool(args and isinstance(args[0], dict) and args[0].get("$o") in self.objects)
            elif method == "getParameter":
                result = {0x1F00: "Mini runtime", 0x1F01: "Detached black WebGL sink",
                          0x1F02: "WebGL 2.0" if c["type"] == "webgl2" else "WebGL 1.0",
                          0x8B8C: "WebGL GLSL ES 3.00" if c["type"] == "webgl2" else "WebGL GLSL ES 1.0",
                          0x0D33: 2048, 0x851C: 2048, 0x84E8: 2048, 0x8869: 8,
                          0x8872: 8, 0x8B4D: 8, 0x8B4C: 0, 0x8DFB: 128, 0x8DFD: 16,
                          0x8DFC: 8, 0x0BA2: [0, 0, *c["size"]], 0x0D3A: [2048, 2048]}.get(args[0], 0)
            elif method.startswith("get") or method == "clientWaitSync":
                result = 0
            for arg in args:
                if isinstance(arg, dict) and "$out" in arg:
                    size = arg["n"] * ELEMENT_BYTES.get(arg["$out"], 1)
                    if not 0 <= size <= MAX_OUTPUT:
                        raise ValueError("Detached WebGL output exceeds 8 MiB")
                    outs.append(base64.b64encode(bytes(size)).decode())
        return {"result": result, "outs": outs if req.get("sync") else [],
                "throws": [], "lost": list(lost), "lastThrew": None}

    @staticmethod
    def black_png(width, height):
        # Transparent black, matching zero-filled readPixels. Large encodes are
        # bounded independently of the caller's canvas attributes.
        if width < 1 or height < 1 or width * height > MAX_OUTPUT // 4:
            return "data:,"
        def chunk(kind, data):
            return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
        png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        png += chunk(b"IDAT", zlib.compress(bytes(height * (1 + width * 4)))) + chunk(b"IEND", b"")
        return "data:image/png;base64," + base64.b64encode(png).decode()
