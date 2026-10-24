"""A WebSocket server for the `websocket` bridge case (compare_bridge.py starts it): RFC 6455 over the
standard library, enough for the case.

  /echo      the handshake (the first offered subprotocol accepted, no extension); each text or binary
             message sent back; the text "bye" answered with a close frame 4001 "server bye"; a client's
             close frame answered with the same code; the text "cut" ends the TCP connection with no close
             frame, as a network that drops it (the bridge-recovery case: what a page whose bridge tab was
             dropped gets in Firefox)
  /echo?...  the query: deflate=1 takes permessage-deflate when offered (answered with pmce, extra
             parameters, appended), the echo deflated with the server's own window; delay=MS waits that
             long before the 101; tag=T records the connection in the WebSocket log
  /wslog?tag=PREFIX  HTTP: the WebSocket requests seen with a tag starting with PREFIX (JSON, with
             CORS): the tag, when the request came and when its 101 went (ms since the server
             started), the Sec-WebSocket-Extensions offered, and for each data frame the client sent
             whether it was compressed (RSV1) and its length on the wire
  /echo-cookie  HTTP: the request's Cookie header as the body, with CORS for the requesting origin and
             credentials (the bridge-recovery case: the profile's cookies reach a new bridge tab's requests)
  /set-cookie?LINES  HTTP: one Set-Cookie header per line of the URL-encoded query, with CORS for the
             requesting origin and credentials (the cookie case: HttpOnly and other cookies from a response)
  /h?...     HTTP, for the fetch case: an answer made from the query (status, loc: Location, acao: the
             Access-Control-Allow-Origin, "echo" for the request's Origin, acac, aceh, acam, acah, maxage
             for a preflight, pacao and pstatus for the preflight only, corp, setcookie: Set-Cookie lines,
             type, body, enc: gzip or deflate); without `body`, the request as the server saw it (JSON)
             rc=1: Cache-Control and Pragma reported too (the fetch-policy case's cache modes)
  /log       HTTP: the /h requests seen so far (method, the query's `id`, what they sent), JSON;
             /log?clear empties it
  /doc/FILE  HTTP: bench/bridge-cases/FILE with the response headers listed in FILE.headers (JSON
             [[name, value], ...]), for a case whose document sets CSP, Referrer-Policy or COEP
  TLS        a ClientHello is answered with a plain HTTP 400 (a wss: URL here fails its TLS at once)
  any other  HTTP 404, as the replay refuses a WebSocket (replay_addon.py); recorded in the
             WebSocket log when the query has a tag

usage: python3 ws_echo.py PORT
"""
import base64, gzip, hashlib, json, os, socketserver, struct, sys, threading, time, zlib
from urllib.parse import parse_qs

# the WebSocket requests with a tag (/wslog)
WSLOG = []
WSLOG_LOCK = threading.Lock()
T0 = time.monotonic()


def now_ms():
    return round((time.monotonic() - T0) * 1000)

# the /h requests seen (the fetch case reads it through /log)
SEEN = []
# the request headers /h reports
REPORTED = ["origin", "referer", "cookie", "sec-fetch-site", "sec-fetch-mode", "sec-fetch-dest", "content-type",
            "x-custom", "x-http-method-override", "accept", "access-control-request-method", "access-control-request-headers"]

# reported only with rc=1: Firefox's own requests elsewhere in the cases may carry them
REPORTED_CACHE = ["cache-control", "pragma"]
CASES = os.path.join(os.path.dirname(os.path.abspath(__file__)), "bridge-cases")
TYPES = {".html": "text/html; charset=utf-8", ".js": "text/javascript", ".css": "text/css"}

GUID = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


def frame(opcode, payload, rsv1=False):
    n = len(payload)
    head = bytes([0x80 | (0x40 if rsv1 else 0) | opcode])
    if n < 126:
        head += bytes([n])
    elif n < 65536:
        head += bytes([126]) + struct.pack("!H", n)
    else:
        head += bytes([127]) + struct.pack("!Q", n)
    return head + payload


class Handler(socketserver.StreamRequestHandler):
    def answer(self, status, headers, body):
        reason = {200: "OK", 204: "No Content", 301: "Moved Permanently", 302: "Found", 303: "See Other",
                  307: "Temporary Redirect", 404: "Not Found", 418: "I'm a teapot"}.get(status, "Whatever")
        head = f"HTTP/1.1 {status} {reason}\r\n" + "".join(f"{k}: {v}\r\n" for k, v in headers)
        self.wfile.write((head + f"Content-Length: {len(body)}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n").encode("latin-1") + body)

    def h(self, method, target, headers):
        """/h and /log (the fetch case)"""
        path, _, query = target.partition("?")
        q = {k: v[-1] for k, v in parse_qs(query, keep_blank_values=True).items()}
        body = self.rfile.read(int(headers.get("content-length", "0") or 0))
        origin = headers.get("origin")
        cors = lambda acao: [] if acao in (None, "none") else [("Access-Control-Allow-Origin", (origin or "null") if acao == "echo" else acao)]
        if path == "/log":
            out = json.dumps(SEEN).encode()
            if "clear" in q:
                SEEN.clear()
            self.answer(200, [("Content-Type", "application/json"), ("Access-Control-Allow-Origin", origin or "*")], out)
            return
        seen = {k: headers[k] for k in REPORTED + (REPORTED_CACHE if "rc" in q else []) if k in headers}
        SEEN.append({"method": method, "id": q.get("id", ""), "h": seen})
        if method == "OPTIONS":
            out = cors(q.get("pacao", q.get("acao")))
            for key, name in (("acac", "Access-Control-Allow-Credentials"), ("acam", "Access-Control-Allow-Methods"),
                              ("acah", "Access-Control-Allow-Headers"), ("maxage", "Access-Control-Max-Age")):
                if key in q:
                    out.append((name, q[key]))
            self.answer(int(q.get("pstatus", "200")), out, b"")
            return
        out = cors(q.get("acao"))
        if "acac" in q:
            out.append(("Access-Control-Allow-Credentials", q["acac"]))
        if "aceh" in q:
            out.append(("Access-Control-Expose-Headers", q["aceh"]))
        if "loc" in q:
            out.append(("Location", q["loc"]))
        if "corp" in q:
            out.append(("Cross-Origin-Resource-Policy", q["corp"]))
        for line in q.get("setcookie", "").split("\n"):
            if line:
                out.append(("Set-Cookie", line))
        out += [("X-A", "a"), ("X-B", "b"), ("Content-Type", q.get("type", "application/json"))]
        payload = (base64.b64decode(q["body64"]) if "body64" in q else q["body"].encode() if "body" in q
                   else json.dumps({"method": method, **seen, "body": body.decode("latin-1")}, sort_keys=True).encode())
        if q.get("enc") == "gzip":
            payload = gzip.compress(payload)
            out.append(("Content-Encoding", "gzip"))
        elif q.get("enc") == "deflate":
            payload = zlib.compress(payload)
            out.append(("Content-Encoding", "deflate"))
        self.answer(int(q.get("status", "200")), out, payload)

    def handle(self):
        # a TLS ClientHello (a wss: URL to this plain server): answered at once with what is not TLS
        if self.rfile.peek(1)[:1] == b"\x16":
            self.wfile.write(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            return
        request = self.rfile.readline().decode("latin-1").split()
        headers = {}
        while True:
            line = self.rfile.readline().decode("latin-1").strip()
            if not line:
                break
            k, _, v = line.partition(":")
            # (a repeated header: the last one; the cases send none twice)
            headers[k.strip().lower()] = v.strip()
        if len(request) >= 2 and request[1].split("?")[0] in ("/h", "/log"):
            self.h(request[0], request[1], headers)
            return
        if len(request) >= 2 and request[1].startswith("/doc/"):
            name = os.path.basename(request[1][len("/doc/"):].split("?")[0])
            path = os.path.join(CASES, name)
            if not os.path.isfile(path):
                self.answer(404, [], b"")
                return
            extra = json.load(open(path + ".headers")) if os.path.exists(path + ".headers") else []
            self.answer(200, [("Content-Type", TYPES.get(os.path.splitext(name)[1], "application/octet-stream"))] + [tuple(h) for h in extra],
                        open(path, "rb").read())
            return
        if len(request) >= 2 and request[1].split("?")[0] == "/echo-cookie":
            body = headers.get("cookie", "").encode()
            origin = headers.get("origin", "*")
            self.wfile.write((f"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {len(body)}\r\n"
                              f"Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Credentials: true\r\n"
                              "Cache-Control: no-store\r\nConnection: close\r\n\r\n").encode() + body)
            return
        if len(request) >= 2 and request[1].split("?")[0] == "/set-cookie":
            from urllib.parse import unquote
            lines = unquote(request[1].partition("?")[2]).split("\n")
            origin = headers.get("origin", "*")
            self.wfile.write(("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n"
                              + "".join(f"Set-Cookie: {line}\r\n" for line in lines if line)
                              + f"Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Credentials: true\r\n"
                              "Cache-Control: no-store\r\nConnection: close\r\n\r\nok").encode())
            return
        if len(request) >= 2 and request[1].split("?")[0] == "/wslog":
            q = {k: v[-1] for k, v in parse_qs(request[1].partition("?")[2]).items()}
            with WSLOG_LOCK:
                out = json.dumps([e for e in WSLOG if e["tag"].startswith(q.get("tag", ""))]).encode()
            self.answer(200, [("Content-Type", "application/json"), ("Access-Control-Allow-Origin", headers.get("origin", "*"))], out)
            return
        q = {k: v[-1] for k, v in parse_qs(request[1].partition("?")[2] if len(request) >= 2 else "").items()}
        entry = None
        if "tag" in q and "sec-websocket-key" in headers:
            entry = {"tag": q["tag"], "at": now_ms(), "offered": headers.get("sec-websocket-extensions", ""), "frames": []}
            with WSLOG_LOCK:
                WSLOG.append(entry)
        if len(request) < 2 or request[1].split("?")[0] != "/echo" or "sec-websocket-key" not in headers:
            self.wfile.write(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            return
        if "delay" in q:
            time.sleep(int(q["delay"]) / 1000)
        accept = base64.b64encode(hashlib.sha1(headers["sec-websocket-key"].encode() + GUID).digest()).decode()
        reply = f"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n"
        offered = [p.strip() for p in headers.get("sec-websocket-protocol", "").split(",") if p.strip()]
        if offered:
            reply += f"Sec-WebSocket-Protocol: {offered[0]}\r\n"
        deflate = q.get("deflate") == "1" and "permessage-deflate" in headers.get("sec-websocket-extensions", "")
        if deflate:
            reply += f"Sec-WebSocket-Extensions: permessage-deflate{q.get('pmce', '')}\r\n"
        self.wfile.write((reply + "\r\n").encode())
        if entry is not None:
            entry["done"] = now_ms()
        # the server's side of permessage-deflate: the client's window (its messages) and the server's own
        inflater = zlib.decompressobj(-15)
        deflater = zlib.compressobj(wbits=-15)
        while True:
            head = self.rfile.read(2)
            if len(head) < 2:
                return
            rsv1, opcode, n = bool(head[0] & 0x40), head[0] & 0x0F, head[1] & 0x7F
            if n == 126:
                n = struct.unpack("!H", self.rfile.read(2))[0]
            elif n == 127:
                n = struct.unpack("!Q", self.rfile.read(8))[0]
            mask = self.rfile.read(4) if head[1] & 0x80 else b"\0\0\0\0"
            payload = bytes(b ^ mask[i % 4] for i, b in enumerate(self.rfile.read(n)))
            if opcode in (0x1, 0x2):
                if entry is not None:
                    entry["frames"].append({"rsv1": rsv1, "len": n})
                if rsv1 and deflate:
                    if "client_no_context_takeover" in q.get("pmce", ""):
                        inflater = zlib.decompressobj(-15)
                    payload = inflater.decompress(payload + b"\0\0\xff\xff")
            if opcode == 0x8:
                self.wfile.write(frame(0x8, payload[:2]))
                return
            if opcode == 0x9:
                self.wfile.write(frame(0xA, payload))
            elif opcode == 0x1 and payload == b"cut":
                return
            elif opcode == 0x1 and payload == b"bye":
                self.wfile.write(frame(0x8, struct.pack("!H", 4001) + b"server bye"))
            elif opcode in (0x1, 0x2) and deflate and payload:
                if "server_no_context_takeover" in q.get("pmce", ""):
                    deflater = zlib.compressobj(wbits=-15)
                self.wfile.write(frame(opcode, (deflater.compress(payload) + deflater.flush(zlib.Z_SYNC_FLUSH))[:-4], rsv1=True))
            elif opcode in (0x1, 0x2):
                self.wfile.write(frame(opcode, payload))


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


if __name__ == "__main__":
    Server(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
