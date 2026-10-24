import json, random, subprocess, time, urllib.request, sys, base64, os
random.seed(int(sys.argv[1]) if len(sys.argv)>1 else 1)
N=int(sys.argv[2]) if len(sys.argv)>2 else 200
LC=os.environ.get("LAYOUTCOUNT", os.path.join(os.path.dirname(__file__), "../../mrt-layout/target/release/layoutcount"))
def track():
    r=random.random()
    if r<.25: return "%dpx"%random.choice([10,20,30,50,80])
    if r<.4: return "%dfr"%random.choice([1,1,2,3])
    if r<.55: return "auto"
    if r<.65: return "minmax(%dpx, %dfr)"%(random.choice([10,30]),random.choice([1,2]))
    if r<.72: return "minmax(%dpx, auto)"%random.choice([10,40])
    if r<.78: return "%d%%"%random.choice([10,25,50])
    if r<.82: return "min-content"
    if r<.86: return "max-content"
    if r<.9: return "fit-content(%dpx)"%random.choice([20,60])
    return "minmax(auto, %dpx)"%random.choice([30,70])
def tracks(n): return " ".join(track() for _ in range(n))
def gen():
    cs=[]
    ncol=random.randint(1,4); nrow=random.randint(0,3)
    if random.random()<.15: cs.append("grid-template-columns:repeat(%s, %s)"%(random.choice(["auto-fill","auto-fit","3"]),random.choice(["minmax(60px,1fr)","50px","80px"])))
    else: cs.append("grid-template-columns:"+tracks(ncol))
    if nrow: cs.append("grid-template-rows:"+tracks(nrow))
    if random.random()<.4: cs.append("grid-auto-rows:"+random.choice(["20px","auto","minmax(10px,auto)","30px 10px"]))
    if random.random()<.2: cs.append("grid-auto-columns:"+random.choice(["20px","auto","40px"]))
    if random.random()<.3: cs.append("gap:%dpx %dpx"%(random.choice([0,5,10]),random.choice([0,5,10])))
    if random.random()<.25: cs.append("grid-auto-flow:"+random.choice(["column","row dense","column dense"]))
    if random.random()<.3: cs.append("justify-content:"+random.choice(["start","center","end","space-between","space-around","space-evenly","stretch"]))
    if random.random()<.3: cs.append("align-content:"+random.choice(["start","center","end","space-between","space-around","space-evenly","stretch"]))
    if random.random()<.3: cs.append("justify-items:"+random.choice(["start","center","end","stretch"]))
    if random.random()<.3: cs.append("align-items:"+random.choice(["start","center","end","stretch"]))
    w=random.choice(["width:400px","width:300px","width:333px",""])
    if w: cs.append(w)
    if random.random()<.4: cs.append("height:%dpx"%random.choice([100,150,200]))
    if random.random()<.2: cs.append("padding:%dpx;border:%dpx solid"%(random.choice([0,5,9]),random.choice([0,2])))
    disp=random.choice(["grid","grid","grid","inline-grid"])
    cs.append("display:"+disp)
    items=[]
    for i in range(random.randint(1,7)):
        st=[]
        if random.random()<.5: st.append("width:%dpx"%random.choice([10,25,40,70,120]))
        if random.random()<.5: st.append("height:%dpx"%random.choice([5,10,25,40]))
        if random.random()<.25: st.append("grid-column:%s"%random.choice(["1","2","3","span 2","1 / span 2","2 / 4","-2","1 / -1","auto / span 3"]))
        if random.random()<.25: st.append("grid-row:%s"%random.choice(["1","2","span 2","1 / 3","2 / span 2","-1","auto / span 2"]))
        if random.random()<.2: st.append("margin:%dpx %dpx"%(random.choice([0,3,7]),random.choice([0,4,"auto"] and [0,4,10])))
        if random.random()<.1: st.append("margin-left:auto")
        if random.random()<.15: st.append("justify-self:"+random.choice(["start","end","center","stretch"]))
        if random.random()<.15: st.append("align-self:"+random.choice(["start","end","center","stretch"]))
        if random.random()<.1: st.append("padding:4px;border:1px solid")
        if random.random()<.1: st.append("min-height:%dpx"%random.choice([20,45]))
        if random.random()<.08: st.append("max-width:%dpx"%random.choice([15,50]))
        if random.random()<.08: st.append("order:%d"%random.choice([-1,1,2]))
        items.append("<div style='%s'>%s</div>"%(";".join(st),random.choice(["","","aaaa bbbb cc","x","aaaaaaaaaaaaaaaaaaaa","<div style='display:grid;grid-template-columns:1fr 20px'><div>a b c d e f</div><div>q</div></div>","<div style='display:flex'><div style='width:10px'>m</div><div>nn oo</div></div>"])))
    return "<div style='%s'>%s</div>"%(";".join(cs),"".join(items))
snips=[gen() for _ in range(N)]
def req(method, path, body=None):
    r = urllib.request.Request("http://127.0.0.1:4456"+path, data=json.dumps(body).encode() if body is not None else None, method=method, headers={"Content-Type":"application/json"})
    return json.load(urllib.request.urlopen(r, timeout=60))
p = subprocess.Popen(["geckodriver","--port","4456"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
time.sleep(2)
bad=0
try:
    s = req("POST","/session",{"capabilities":{"alwaysMatch":{"browserName":"firefox","moz:firefoxOptions":{"args":["-headless","--width=1489","--height=844"],"prefs":{"javascript.enabled":True,"ui.useOverlayScrollbars":1}}}}})["value"]["sessionId"]
    for k,sn in enumerate(snips):
        html="<!DOCTYPE html><html><head><style>body{margin:0;font:10px monospace}</style></head><body>"+sn+"</body></html>"
        url="data:text/html;base64,"+base64.b64encode(html.encode()).decode()
        req("POST","/session/%s/url"%s,{"url":url})
        js="var c=document.body.firstElementChild;var r0=c.getBoundingClientRect();return [[r0.left,r0.top,r0.width,r0.height]].concat(Array.from(c.children).map(function(e){var r=e.getBoundingClientRect();return [r.left,r.top,r.width,r.height]}));"
        ff=req("POST","/session/%s/execute/sync"%s,{"script":js,"args":[]})["value"]
        open("/tmp/claude-1000/wd/t.html","w").write(html)
        out=subprocess.run([LC,"/tmp/claude-1000/wd/t.html","--boxes"],capture_output=True,text=True).stdout
        mine=[]; cont=None
        for line in out.splitlines():
            if line.startswith("E "):
                f=line.split()
                if int(f[1])>=4 or (f[2]=="3" and f[3]=="div"):
                    pass
                if f[3]=="div" and len(f)>=8 and (f[2]=="3" or (cont is not None and int(f[2])==cont)):
                    if f[2]=="3": cont=int(f[1])
                    mine.append([float(x) for x in f[4:8]])
        # element order: container first then items; items with display:none are absent in both
        ok = len(mine)==len(ff) and all(all(abs(a-b)<=0.51 for a,b in zip(x,y)) for x,y in zip(mine,ff))
        if not ok:
            bad+=1
            print("MISMATCH",k); print(sn); print(" ff  ",[[round(v,2) for v in r] for r in ff]); print(" mine",mine)
    req("DELETE","/session/"+s)
finally:
    p.terminate()
print("bad",bad,"of",N,"last compared",len(mine),len(ff))
