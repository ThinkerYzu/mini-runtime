"""Unique memory (USS) of a process, and of a Firefox and its child processes, read from /proc while a
page is in use (task 0e92: the end-to-end run on live X samples both sides after each step).

uss_mb(pid)         a process's Private_Clean + Private_Dirty, MB (None once it has ended)
cpu_s(pid)          a process's user and system CPU time so far, seconds
firefox_mb(pid)     a Firefox's processes by kind: {"total", "main", "tab_max", "tabs", "other"}, MB;
                    `tab_max` is the largest content process, the site's own when one site is open;
                    with the CPU seconds of all of them (`cpu_s`) and of the largest (`tab_max_cpu_s`)
"""
import os


def uss_mb(pid):
    try:
        kb = 0
        for line in open(f"/proc/{pid}/smaps_rollup"):
            if line.startswith(("Private_Clean:", "Private_Dirty:")):
                kb += int(line.split()[1])
        return round(kb / 1024, 1)
    except OSError:
        return None


def cpu_s(pid):
    try:
        fields = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
        return round((int(fields[11]) + int(fields[12])) / os.sysconf("SC_CLK_TCK"), 2)
    except (OSError, IndexError, ValueError):
        return None


def children(pid):
    """Every process descended from `pid`."""
    kids = {}
    for p in os.listdir("/proc"):
        if not p.isdigit():
            continue
        try:
            ppid = int(open(f"/proc/{p}/stat").read().rsplit(")", 1)[1].split()[1])
        except (OSError, IndexError, ValueError):
            continue
        kids.setdefault(ppid, []).append(int(p))
    out, todo = [], [pid]
    while todo:
        for k in kids.get(todo.pop(), []):
            out.append(k)
            todo.append(k)
    return out


def firefox_mb(pid):
    tabs, other, cpu = [], 0.0, cpu_s(pid) or 0.0
    for k in children(pid):
        try:
            # a forked content process rewrites its command line as one string: split on spaces too
            cmd = open(f"/proc/{k}/cmdline").read().replace("\0", " ").split()
        except OSError:
            continue
        uss = uss_mb(k) or 0.0
        cpu += cpu_s(k) or 0.0
        # a content process's command line ends with its type: tab, rdd, socket, forkserver, ...
        kind = cmd[-1] if cmd else ""
        if "-contentproc" in cmd and kind == "tab":
            tabs.append((uss, cpu_s(k) or 0.0))
        else:
            other += uss
    main = uss_mb(pid) or 0.0
    biggest = max(tabs, default=(0.0, 0.0))
    return {"total": round(main + other + sum(t[0] for t in tabs), 1), "main": main, "tab_max": biggest[0],
            "tabs": len(tabs), "other": round(other, 1), "cpu_s": round(cpu, 2), "tab_max_cpu_s": biggest[1]}
