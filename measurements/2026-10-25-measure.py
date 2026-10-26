#!/usr/bin/env python3
"""Mede CPU/RSS/GPU de um processo (ou só o sistema) numa janela fixa.

uso: measure.py LABEL LOGFILE [--] [cmd ...]
Sem cmd: baseline (só sistema).
"""
import os, sys, time, json, subprocess, glob

WARMUP = 10
WINDOW = 60
TCK = os.sysconf("SC_CLK_TCK")
CARD = "/sys/class/drm/card1"
RC6 = CARD + "/gt/gt0/rc6_residency_ms"


def proc_ticks(pid):
    with open(f"/proc/{pid}/stat") as f:
        data = f.read()
    rest = data[data.rindex(")") + 2:].split()
    # rest[0] é o campo 3; utime = campo 14, stime = campo 15
    return int(rest[11]), int(rest[12])


def sys_cpu():
    with open("/proc/stat") as f:
        parts = f.readline().split()
    vals = list(map(int, parts[1:]))
    idle = vals[3] + vals[4]  # idle + iowait
    total = sum(vals[:8])  # user..steal (guest já incluso em user)
    return total, idle


def rss_kb(pid):
    with open(f"/proc/{pid}/status") as f:
        for line in f:
            if line.startswith("VmRSS:"):
                return int(line.split()[1])


def drm_engines(pid):
    """Soma drm-engine-* (ns) por cliente DRM único do processo."""
    clients = {}
    for fd in glob.glob(f"/proc/{pid}/fdinfo/*"):
        try:
            with open(fd) as f:
                txt = f.read()
        except OSError:
            continue
        if "drm-client-id" not in txt:
            continue
        kv = {}
        for line in txt.splitlines():
            if ":" in line:
                k, v = line.split(":", 1)
                kv[k.strip()] = v.strip()
        cid = kv["drm-client-id"]
        eng = {k[len("drm-engine-"):]: int(v.split()[0]) for k, v in kv.items()
               if k.startswith("drm-engine-") and not k.startswith("drm-engine-capacity")}
        clients[cid] = eng
    tot = {}
    for eng in clients.values():
        for k, v in eng.items():
            tot[k] = tot.get(k, 0) + v
    return tot, len(clients)


def read(path):
    with open(path) as f:
        return int(f.read().strip())


def pids_named(name):
    out = []
    for d in glob.glob("/proc/[0-9]*"):
        try:
            with open(d + "/comm") as f:
                if f.read().strip() == name:
                    out.append(int(d[6:]))
        except OSError:
            pass
    return out


def main():
    label, logfile = sys.argv[1], sys.argv[2]
    cmd = [a for a in sys.argv[3:] if a != "--"]
    proc = None
    log = open(logfile, "w")
    if cmd:
        proc = subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT)
    time.sleep(WARMUP)
    if proc and proc.poll() is not None:
        print(json.dumps({"label": label, "error": f"saiu cedo, código {proc.returncode}"}))
        return

    extra = {n: pids_named(n) for n in ("Xorg", "cinnamon")}
    extra0 = {n: [sum(proc_ticks(p)) for p in ps] for n, ps in extra.items()}

    t0 = time.monotonic()
    s0 = sys_cpu()
    rc0 = read(RC6)
    if proc:
        p0 = proc_ticks(proc.pid)
        d0, nclients = drm_engines(proc.pid)
    act, cur = [], []
    for i in range(WINDOW):
        act.append(read(CARD + "/gt_act_freq_mhz"))
        cur.append(read(CARD + "/gt_cur_freq_mhz"))
        target = t0 + i + 1
        time.sleep(max(0, target - time.monotonic()))
    t1 = time.monotonic()
    s1 = sys_cpu()
    rc1 = read(RC6)
    res = {"label": label, "cmd": cmd, "window_s": round(t1 - t0, 3), "clk_tck": TCK}
    if proc:
        if proc.poll() is not None:
            res["warning"] = f"processo saiu durante a janela, código {proc.returncode}"
        p1 = proc_ticks(proc.pid)
        d1, _ = drm_engines(proc.pid)
        res["rss_kb_end"] = rss_kb(proc.pid)
        res["proc_ticks"] = {"utime": p1[0] - p0[0], "stime": p1[1] - p0[1]}
        dt = (p1[0] + p1[1]) - (p0[0] + p0[1])
        res["proc_cpu_pct_one_core"] = 100.0 * dt / (TCK * (t1 - t0))
        res["drm_clients"] = nclients
        res["drm_engine_busy_pct"] = {k: 100.0 * (d1[k] - d0.get(k, 0)) / ((t1 - t0) * 1e9)
                                      for k in d1}
    st = s1[0] - s0[0]
    si = s1[1] - s0[1]
    res["sys_ticks"] = {"total": st, "idle_iowait": si}
    res["sys_cpu_pct_all_cpus"] = 100.0 * (st - si) / st
    res["gpu_rc6_pct"] = 100.0 * (rc1 - rc0) / ((t1 - t0) * 1000)
    res["gt_act_mhz"] = {"avg": sum(act) / len(act), "max": max(act), "min": min(act)}
    res["gt_cur_mhz"] = {"avg": sum(cur) / len(cur), "max": max(cur), "min": min(cur)}
    extra1 = {}
    for n, ps in extra.items():
        vals = []
        for p, a in zip(ps, extra0[n]):
            try:
                vals.append(100.0 * (sum(proc_ticks(p)) - a) / (TCK * (t1 - t0)))
            except OSError:
                pass
        extra1[n] = vals
    res["other_proc_cpu_pct_one_core"] = extra1
    print(json.dumps(res))
    sys.stdout.flush()
    if proc:
        try:
            proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            proc.terminate()
            proc.wait()
        res2 = {"label": label, "exit_code": proc.returncode}
        print(json.dumps(res2))


main()
