#!/usr/bin/env python3
"""Make a per-level TAS for a category: search plans with the solver, play the best in the real game, check it
against the simulator, and record a video.

usage: tas/pipeline.py CATEGORY OUT PHASE [levels...]
  CATEGORY  min-time | uef | max-score | showcase        (the four runs in tas/runs/, see tas/README.md)
  OUT       output directory (created; every phase is resumable and skips finished work)
  PHASE     screen | refine | scout | play | video | all   (scout: max-score only, not part of all)
  levels    1-1 .. 11-5 (default: all 55)

phases:
  screen   cheap search over many (master, seed) pairs per level         → OUT/screen, OUT/logs
  refine   stronger searches on the TOP best (seed, master) pairs        → OUT/best/S_L.{txt,plan.txt,seed}
  scout    max-score: rank SCOUT_SEEDS (default 1-100000) by their Fever lane with solver `scout`, then the
           Fever search on the best SCOUT_TOP (2) seeds, three ways; a better plan replaces OUT/best
                                                                         → OUT/scout, OUT/scoutruns
  play     play OUT/best in the real game (tas/play.sh), check it        → OUT/play/S_L, OUT/results.tsv
  video    replay at real speed with the cursor shown, record, stitch    → OUT/video/S_L, OUT/tas.mp4

env: CHARS (masters, e.g. 7 or 0,3,6), SEEDS (e.g. 1-16), TOP (refine candidates), SETS (refine sets, e.g.
light), JOBS (parallel searches' levels; plays always run one at a time). Solver searches use all cores
but two (RAYON_NUM_THREADS to change).
"""
import os, re, shutil, subprocess, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SOLVER = f"{ROOT}/solver/target/release/peggle-solver"
GC = f"{ROOT}/sim/target/release/game_check"
NAMES = ["Bjorn", "Jimmy", "Kat", "Splork", "Claude", "Renfield", "Tula", "Warren", "Cinderbottom", "Hu"]
ALL = [f"{s}-{l}" for s in range(1, 12) for l in range(1, 6)]
FEVER = "--probe-untrip 0.7 --fever-frac 0.7 --strata --max-shots 60"
IL_SETS = [("light", "--beam 10 --coarse 360 --delays 0,2 --refine-top 4 --refine-rounds 3"),
           ("wide", "--beam 16 --coarse 480 --delays 0,1,3 --refine-top 4 --refine-rounds 3")]
CATEGORIES = {
    # fastest clear (speedrun.com IL timing: app frames until Level Complete); fast forward held throughout
    "min-time": dict(objective="speed", extra="", chars="0,3,4,6,8", seeds="1-8", top=2, rank="frames", mode="time",
                     screen="--beam 6 --coarse 240 --delays 0 --refine-top 3 --refine-rounds 2", sets=IL_SETS),
    # Ultra Extreme Fever: clear every peg, fastest
    "uef": dict(objective="uef", extra="--max-shots 20", chars="0,1,3,4,5,6,8", seeds="1-4", top=2, rank="frames", mode="time",
                screen="--beam 6 --coarse 240 --delays 0 --refine-top 3 --refine-rounds 2", sets=IL_SETS),
    # highest score: save a lane of pegs for a tripled 100x Extreme Fever finish (Warren)
    "max-score": dict(objective="score", extra=FEVER, chars="7", seeds="1-16", top=4, rank="total", mode="score",
                      screen="--beam 20 --probe-pool 100 --probe 160 --coarse 360 --delays 0,2 --refine-top 4 --refine-rounds 3",
                      sets=[("light", "--beam 30 --probe-pool 150 --probe 240 --coarse 480 --delays 0,1,3 --refine-top 4 --refine-rounds 3"),
                            ("wide", "--beam 60 --probe-pool 240 --probe 240 --coarse 480 --delays 0,1,3 --refine-top 4 --refine-rounds 3")]),
    # fewest shots, then highest score
    "showcase": dict(objective="shots", extra="--max-shots 15", chars="0,1,2,3,4,5,6,7,8,9", seeds="1-2", top=3, rank="shots", mode="fun",
                     screen="--beam 16 --coarse 480 --delays 0 --refine-top 4 --refine-rounds 3",
                     sets=[("light", "--beam 32 --coarse 720 --delays 0,2 --refine-top 6 --refine-rounds 4"),
                           ("wide", "--beam 48 --coarse 1080 --delays 0,1,3 --refine-top 6 --refine-rounds 4")]),
}


def sl(lv):
    s, l = lv.split("-"); return f"{int(s) - 1}_{int(l) - 1}"


def value(plan_text, rank):
    """ranking value of a plan (higher is better)"""
    t = re.search(r"# total (\d+) after (\d+) shots", plan_text)
    if rank == "frames":
        m = re.search(r"fast forward held: (\d+) frames", plan_text)
        return -int(m.group(1)) if m else None
    if not t: return None
    return int(t.group(1)) if rank == "total" else (-int(t.group(2)), int(t.group(1)))


def plan_value(path, rank):
    return value(open(path).read(), rank) if os.path.exists(path) else None


def solve(c, lv, chars, seeds, args, out, log):
    with open(log, "w") as lf:
        subprocess.run([SOLVER, lv, "--objective", c["objective"], "--chars", chars, "--seeds", seeds, *args.split(), *c["extra"].split(),
                        "--out", out], stdout=lf, stderr=subprocess.STDOUT)


def screen(c, o, lv):
    log = f"{o}/logs/{lv}.screen"
    if os.path.exists(log) and "# " in open(log).read(): return
    solve(c, lv, os.environ.get("CHARS", c["chars"]), os.environ.get("SEEDS", c["seeds"]), c["screen"], f"{o}/screen", log)
    print(lv, "screen", plan_value(f"{o}/screen/{sl(lv)}.plan.txt", c["rank"]), flush=True)


def refine(c, o, lv):
    k = sl(lv); log = f"{o}/logs/{lv}.screen"
    if not os.path.exists(log): return
    # per (seed, master) results of the screen: "seed S char Some(C): N shots, F frames ..., total T"
    cand = {}
    for m in re.finditer(r"^seed (\d+) char Some\((\d+)\): (\d+) shots, (\d+) frames.*total (\d+)", open(log).read(), re.M):
        shots, frames, tot = int(m.group(3)), int(m.group(4)), int(m.group(5))
        v = {"frames": -frames, "total": tot, "shots": (-shots, tot)}[c["rank"]]
        key = (int(m.group(1)), int(m.group(2))); cand[key] = max(cand.get(key, v), v)
    pick = sorted(cand, key=lambda x: cand[x], reverse=True)[:int(os.environ.get("TOP", c["top"]))]
    sets = [s for s in c["sets"] if not os.environ.get("SETS") or s[0] in os.environ["SETS"].split(",")]
    res = [(plan_value(f"{o}/screen/{k}.plan.txt", c["rank"]), f"{o}/screen")]
    for seed, ch in pick:
        for name, args in sets:
            d = f"{o}/refine/{lv}_s{seed}_c{ch}_{name}"
            if not os.path.exists(f"{d}/{k}.plan.txt"): solve(c, lv, str(ch), str(seed), args, d, f"{o}/logs/{lv}_s{seed}_c{ch}_{name}.log")
            res.append((plan_value(f"{d}/{k}.plan.txt", c["rank"]), d))
    res = sorted([r for r in res if r[0] is not None], reverse=True)
    if not res: print(lv, "no plan found"); return
    os.makedirs(f"{o}/best", exist_ok=True)
    for ext in ("txt", "plan.txt", "seed"): shutil.copy(f"{res[0][1]}/{k}.{ext}", f"{o}/best/{k}.{ext}")
    print(lv, "best", res[0][0], "from", os.path.relpath(res[0][1], o), flush=True)


# scout phase: the Fever search per scouted seed, with the catch-streak value off / 0.5 / plus the extra options
SCOUT_RUNS = [("w0", "--streak-value 0"), ("w0.5", "--streak-value 0.5"),
              ("wR", "--streak-value 0.5 --review --probe-delays 0,2")]


def scout(c, o, lv):
    """seed scouting (the max-score results came from this): the seed decides which pegs are orange and green, and
    the best finishes need the last orange at the start of a long lane with the other oranges off it"""
    k = sl(lv); os.makedirs(f"{o}/scout", exist_ok=True)
    out = f"{o}/scout/{lv}.txt"
    if not (os.path.exists(out) and os.path.getsize(out) > 0):
        a, b = os.environ.get("SCOUT_SEEDS", "1-100000").split("-")
        with open(f"{out}.tmp", "w") as f, open(f"{o}/logs/{lv}.scout", "w") as lf:
            r = subprocess.run([f"{ROOT}/solver/target/release/scout", lv, c["chars"].split(",")[0], a, b, "4"], stdout=f, stderr=lf,
                               env=dict(os.environ, SCOUT_LANES=os.environ.get("SCOUT_LANES", "600")))
        if r.returncode: print(lv, "scout failed, see", f"{o}/logs/{lv}.scout"); return
        os.replace(f"{out}.tmp", out)
    seeds = [l.split()[1].rstrip(":") for l in open(out).read().splitlines()[:int(os.environ.get("SCOUT_TOP", "2"))]]
    sets = dict(c["sets"]); args = sets.get("light", c["sets"][0][1])
    res = [(plan_value(f"{o}/best/{k}.plan.txt", c["rank"]), None)]
    for seed in seeds:
        for name, extra in SCOUT_RUNS:
            d = f"{o}/scoutruns/{lv}_s{seed}_{name}"
            if not os.path.exists(f"{d}/{k}.plan.txt"):
                solve(c, lv, c["chars"].split(",")[0], seed, f"{args} {extra}", d, f"{o}/logs/{lv}_s{seed}_{name}.log")
            res.append((plan_value(f"{d}/{k}.plan.txt", c["rank"]), d))
    res = sorted([r for r in res if r[0] is not None], key=lambda r: r[0], reverse=True)
    if res and res[0][1]:
        os.makedirs(f"{o}/best", exist_ok=True)
        for ext in ("txt", "plan.txt", "seed"): shutil.copy(f"{res[0][1]}/{k}.{ext}", f"{o}/best/{k}.{ext}")
        print(lv, "scout best", res[0][0], "from", os.path.relpath(res[0][1], o), flush=True)
    elif res:
        print(lv, "scout: no better plan than OUT/best", res[0][0], flush=True)


def reusable(d, script, seed):
    """a finished run in d is reused only when it played this exact script and seed (after a better plan is found
    for the level, the old plan's run must not be reported for it)"""
    try:
        h, sc = open(f"{d}/harness.txt").read(), open(f"{d}/script.txt").read()
        info = open(f"{d}/peggle_oracle_info.txt").read()
    except OSError:
        return False
    return "state 5" in h and sc == open(script).read() and f"tas: rng seed {seed}\n" in info + "\n"


def play(o, lv, video=False):
    k = sl(lv); script = f"{o}/best/{k}.txt"
    if not os.path.exists(script): return None
    d = f"{o}/{'video' if video else 'play'}/{k}"
    if video:  # the cursor glides to each shot's aim point on screen (visual only)
        os.makedirs(d, exist_ok=True)
        subprocess.run([sys.executable, f"{ROOT}/tas/cursor_script.py", script, f"{o}/best/{k}.plan.txt", f"{d}.cursor.txt"], check=True)
        script = f"{d}.cursor.txt"
    seed = open(f"{o}/best/{k}.seed").read().strip()
    if not reusable(d, script, seed):
        subprocess.run(["sh", f"{ROOT}/tas/play.sh", script, seed, d] + (["--video"] if video else []))
    h = open(f"{d}/harness.txt").read() if os.path.exists(f"{d}/harness.txt") else ""
    tot = re.findall(r"st5 frm \d+ .* total (\d+)", h)
    frm = re.findall(r"frm (\d+) tick \d+ state 5", h)
    gc = subprocess.run([GC, d], capture_output=True, text=True, env=dict(os.environ, GC_REAL_FIRE="1")).stdout.strip().splitlines()
    nshots = len(re.findall(r"^shotx", open(f"{o}/best/{k}.txt").read(), re.M))
    exact = len(gc) >= 2 and "shots ok" in gc[0] and not any(("peg diffs" in l or " sim " in l) and "UNSAFE" not in l for l in gc[1:-1])
    pt = open(f"{o}/best/{k}.plan.txt").read()
    ch = re.search(r"char Some\((\d+)\)", pt)
    return dict(level=lv, master=NAMES[int(ch.group(1))] if ch else "-", seed=open(f"{o}/best/{k}.seed").read().strip(), shots=nshots,
                predicted=(re.search(r"# total (\d+)", pt) or [None, None])[1], real=int(tot[-1]) if tot else None,
                frames=int(frm[0]) if frm else None, won=bool(gc) and f"real won at shot {nshots})" in gc[0], exact=exact)


def main():
    if len(sys.argv) < 4 or sys.argv[1] not in CATEGORIES: sys.exit(__doc__)
    c, o, phase = CATEGORIES[sys.argv[1]], sys.argv[2], sys.argv[3]
    levels = sys.argv[4:] or ALL
    for sub in ("logs", "screen", "refine", "best"): os.makedirs(f"{o}/{sub}", exist_ok=True)
    if phase in ("screen", "all"):
        for lv in levels: screen(c, o, lv)
    if phase in ("refine", "all"):
        for lv in levels: refine(c, o, lv)
    if phase == "scout":
        if c["objective"] != "score": sys.exit("scout: max-score only")
        for lv in levels: scout(c, o, lv)
    for vid in ([False] if phase == "play" else [True] if phase == "video" else [False, True] if phase == "all" else []):
        rs = [r for r in (play(o, lv, vid) for lv in levels) if r]
        cols = ["level", "master", "seed", "shots", "predicted", "real", "frames", "won", "exact"]
        path = f"{o}/results{'_video' if vid else ''}.tsv"
        rows = {}  # keep other levels' rows when only some levels were played
        if os.path.exists(path):
            for line in open(path).read().splitlines()[1:]:
                rows[line.split("\t")[0]] = line
        for r in rs: rows[r["level"]] = "\t".join(str(r[x]) for x in cols)
        with open(path, "w") as f:
            f.write("\t".join(cols) + "\n" + "".join(rows[lv] + "\n" for lv in ALL if lv in rows))
        print(f"played {len(rs)} levels{' (video)' if vid else ''}: {sum(r['won'] for r in rs)} won, {sum(r['exact'] for r in rs)} exact", flush=True)
        if vid:
            subprocess.run([sys.executable, f"{ROOT}/tas/video.py", o, *levels], env=dict(os.environ, MODE=c["mode"]))


if __name__ == "__main__":
    main()
