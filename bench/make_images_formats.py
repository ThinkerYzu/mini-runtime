"""Writes the binary fixtures of bench/bridge-cases/images-formats.html (GAPS B165): AVIF from ImageMagick
(libheif), PNGs written here so their sizes and damage are exact.

  python3 bench/make_images_formats.py
"""
import gzip, os, struct, subprocess, zlib

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "bridge-cases")


def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


def png(width, height, idat=None):
    """A grey PNG (one byte a pixel); idat replaces the compressed pixels."""
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0)
    if idat is None:
        packer = zlib.compressobj(9)
        row = b"\0" * (width + 1)
        idat = b"".join(packer.compress(row) for _ in range(height)) + packer.flush()
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + (chunk(b"IDAT", idat) if idat else b"") + chunk(b"IEND", b"")


def rotate(avif):
    """The AVIF with an irot of a quarter turn on its one item: the box added to ipco, its index to
    ipma's list, the sizes of the boxes around them and iloc's offset into mdat moved to match."""
    data = bytearray(avif)
    grow = lambda at, n: data.__setitem__(slice(at, at + 4), struct.pack(">I", struct.unpack(">I", data[at:at + 4])[0] + n))
    meta, iprp, ipco, ipma, iloc = (data.index(k) - 4 for k in (b"meta", b"iprp", b"ipco", b"ipma", b"iloc"))
    # ipma (version 0, flags 0): entry count, item id, association count, then one byte an index
    count_at = ipma + 12 + 4 + 2
    count = data[count_at]
    data[count_at + 1 + count:count_at + 1 + count] = bytes([0x80 | (count + 1)])
    data[count_at] = count + 1
    grow(ipma, 1)
    ipco_end = ipco + struct.unpack(">I", data[ipco:ipco + 4])[0]
    data[ipco_end:ipco_end] = struct.pack(">I", 9) + b"irot" + bytes([1])
    grow(ipco, 9)
    grow(iprp, 10)
    grow(meta, 10)
    # iloc version 0 (ImageMagick's): sizes, item count, item id, data reference, base offset
    grow(iloc + 12 + 2 + 2 + 2 + 2, 10)
    return bytes(data)


def write(name, data):
    with open(os.path.join(OUT, "images-formats-" + name), "wb") as f:
        f.write(data)


def main():
    avif = os.path.join(OUT, "images-formats.avif")
    subprocess.run(["magick", "-size", "7x3", "xc:#3080c0", "-quality", "60", avif], check=True)
    data = open(avif, "rb").read()
    mdat = data.index(b"mdat") + 4
    write("cut.avif", data[:mdat + 4])
    # the AV1 payload's bytes after its sequence header, inverted: the container still parses
    write("bad.avif", data[:mdat + 12] + bytes(b ^ 0xFF for b in data[mdat + 12:]))
    write("irot.avif", rotate(data))
    write("wide.png", png(20000, 2))
    write("axis.png", png(70000, 1))
    write("budget.png", png(12000, 12000))
    write("header.png", png(7, 3, idat=b""))
    write("idat.png", png(7, 3, idat=b"\x78\x9c" + b"\xff" * 12))
    full = png(7, 3)
    write("cut.png", full[:len(full) - 20])
    with open(os.path.join(OUT, "images-api.jpg"), "rb") as f:
        jpeg = f.read()
    sos = jpeg.index(b"\xff\xda")
    write("header.jpg", jpeg[:sos])
    write("scan.jpg", jpeg[:sos + 20])
    with open(os.path.join(OUT, "images-api.gif"), "rb") as f:
        gif = f.read()
    write("header.gif", gif[:13])
    # SVGZ: gzip bytes served as image/svg+xml, with and without Content-Encoding
    svg = b'<svg xmlns="http://www.w3.org/2000/svg" width="17" height="9"/>'
    write("svgz.svgz", gzip.compress(svg, mtime=0))


main()
