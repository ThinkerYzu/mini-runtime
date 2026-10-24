import json,sys,subprocess,os
S=os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(S))
from recorded_tests import require_recordings
require_recordings(record="OPAGES=/path/to/bench/oracle/opages OOUT=/path/to/bench/oracle/oout ONLY=PAGE FIREFOX_BIN=... ./mach python /path/to/bench/oracle/oracle.py")
if len(sys.argv) < 2:
    sys.exit("usage: MRT_RECORDED_TESTS=1 python3 bench/oracle/cmp.py PAGE")
name=sys.argv[1]
require_recordings(f"{S}/oout/{name}.json",
                   record="OPAGES=/path/to/bench/oracle/opages OOUT=/path/to/bench/oracle/oout ONLY=PAGE FIREFOX_BIN=... ./mach python /path/to/bench/oracle/oracle.py")
BIN=os.path.expanduser("~/.cache/mrt-tgt-S/release/layoutcount")
ff=json.load(open(f"{S}/oout/{name}.json"))
r=subprocess.run([BIN,f"{S}/opages/{name}.html","--boxes"],capture_output=True,text=True)
lines=r.stdout.splitlines()
print(lines[0][:200])
mb={}
for ln in lines[1:]:
    if ln.startswith("E "):
        f=ln.split(); 
        if len(f)>4: mb[int(f[1])]=tuple(float(v) for v in f[4:8])
for b in ff["boxes"]:
    i=b[0]; t=ff["tags"][i]
    if t in ("div","body","html","option"): continue
    m=mb.get(i)
    f=tuple(b[1:5])
    ok = m is not None and all(abs(a-c)<0.6 for a,c in zip(f,m))
    print("%-4d %-9s %-8s FF[%.1f %.1f %.1f %.1f] mini %s %s"%(i,t,b[6],*f,("[%.1f %.1f %.1f %.1f]"%m) if m else None,"" if ok else "  <<<"))
