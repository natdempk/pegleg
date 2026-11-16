#!/usr/bin/env python3
"""Assemble recorded real-game runs (OUT/video/S_L/run.mp4, from `tas/pipeline.py CATEGORY OUT video`)
into one 1920x1080 video: game on the left (800x600 → 1440x1080, Lanczos), a side panel with the level, the
RNG seed and a running timer (app frames / 100 from the first board frame, stopped when the Level Complete
box appears), and the game's sound (mixed from the recording's sound log, sound_mix.py). Needs Pillow and
numpy (pip install pillow numpy).

usage: MODE=time|score|fun tas/video.py OUT [levels...]   → OUT/tas.mp4 (NAME=... to rename)
Fonts: FONT / MONO env, else Helvetica/Menlo (macOS), Arial/Consolas (Windows), DejaVu (Linux).
"""
import json, os, re, shutil, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from PIL import Image, ImageDraw, ImageFont

FPS = 50
def _font(env, *cands):
    for c in ([os.environ[env]] if os.environ.get(env) else []) + list(cands):
        if os.path.exists(c): return c
    raise SystemExit(f"no font found: set {env}=/path/to/font.ttf")
FONT = _font("FONT", "/System/Library/Fonts/Helvetica.ttc", "C:/Windows/Fonts/arial.ttf", "/mnt/c/Windows/Fonts/arial.ttf",
             "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf")
MONO = _font("MONO", "/System/Library/Fonts/Menlo.ttc", "C:/Windows/Fonts/consola.ttf", "/mnt/c/Windows/Fonts/consola.ttf",
             "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf")
NAMES = ["Bjorn", "Jimmy", "Kat", "Splork", "Claude", "Renfield", "Tula", "Warren", "Cinderbottom", "Hu"]
POWERS = ["Super Guide", "Multiball", "Pyramid", "Space Blast", "Flippers", "Spooky Ball", "Flower Power", "Lucky Spin",
          "Fireball", "Zen Shot"]
# style ids (LogicMgr style switch FUN_0046b100 → string table)
STYLES = {0: "Long Shot", 1: "Flipper Maniac", 2: "Extreme Slide", 3: "Orange Attack", 4: "Cool Clear", 5: "Multiball Madness",
          6: "Hat Trick", 7: "Hot Stuff", 8: "Free Ball Frenzy", 9: "Free Ball Skills", 10: "Power-up Combo",
          11: "Kick the Bucket", 12: "Lucky Bounce", 13: "Mad Skillz", 14: "Crazy Mad Skillz", 15: "Super Long Shot",
          16: "Pyramid Pickup", 17: "Bucket Bonus", 19: "Mega Shot Bonus"}
MODE = os.environ.get("MODE", "time")  # time (min-time IL) | score (max score) | fun (showcase captions)
GC = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "sim/target/release/game_check")
_memo = {}


def shot_info(d):
    """per-shot results of the real run (game_check replays the real fire ticks)"""
    o = subprocess.run([GC, d], capture_output=True, text=True, env=dict(os.environ, GC_REAL_FIRE="1", GC_VERBOSE="1"))
    o = o.stdout + o.stderr
    out = []
    for m in re.finditer(r"pegs: (\d+), oranges: (\d+), score: (\d+), shot_score: \d+, styles: \[([^\]]*)\].*?free_balls: (\d+), fever_hole: (\d+).*?activations: \[([^\]]*)\]", o):
        out.append(dict(pegs=int(m.group(1)), oranges=int(m.group(2)), score=int(m.group(3)),
                        styles=[int(x) for x in m.group(4).split(",") if x.strip()], free=int(m.group(5)),
                        hole=int(m.group(6)), acts=len([x for x in m.group(7).split(",") if x.strip()])))
    return out


def sl(lv):
    s, l = lv.split("-")
    return f"{int(s) - 1}_{int(l) - 1}"


def panel(path, lines, timer, timer_col):
    key = (tuple(lines), timer, timer_col)
    if key in _memo:
        os.link(_memo[key], path); return
    _memo[key] = path
    im = Image.new("RGB", (480, 1080), (14, 16, 28))
    d = ImageDraw.Draw(im)
    f_big, f_mid, f_small = ImageFont.truetype(FONT, 51), ImageFont.truetype(FONT, 33), ImageFont.truetype(FONT, 27)
    f_t = ImageFont.truetype(MONO, 78)
    y = 60
    for txt, size, col in lines:
        f = {"big": f_big, "mid": f_mid, "small": f_small}[size]
        d.text((36, y), txt, font=f, fill=col)
        y += {"big": 72, "mid": 51, "small": 42}[size]
    if timer: d.text((36, 900), timer, font=f_t, fill=timer_col)
    im.save(path)


def clip(out, lv, rec, idx, total, run_total=0):
    k = sl(lv)
    d = f"{out}/video/{k}"
    mp4, h = f"{d}/run.mp4", open(f"{d}/harness.txt").read()
    ms = lambda pat: int(re.search(r"\[(\d+)\] " + pat, h).group(1))
    m5 = re.search(r"\[(\d+)\] frm (\d+) tick \d+ state 5", h)
    if m5 is None:  # the run did not reach Level Complete: leave it out
        print(lv, "no Level Complete in the recording, skipped", flush=True)
        return None, 0.0
    t1, t5, frames5 = ms(r"frm 1 tick"), int(m5.group(1)), int(m5.group(2))
    dur = float(subprocess.check_output(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", mp4]))
    v0 = ms(r"quit") - dur * 1000                       # wall ms of video t=0 (ffmpeg stops as the game exits)
    start, end = (t1 - v0) / 1000 - 0.3, (t5 - v0) / 1000 + 1.8
    plan = open(f"{out}/best/{k}.plan.txt").read()
    ch = int(re.search(r"char Some\((\d+)\)", plan).group(1))
    seed = open(f"{out}/best/{k}.seed").read().strip()
    shots = len(re.findall(r"^\d+: pixel", plan, re.M))
    r = rec.get(lv, {})
    ours = frames5 / 100 + 0.5
    pd = f"{d}/panel"; shutil.rmtree(pd, ignore_errors=True); os.makedirs(pd, exist_ok=True); _memo.clear()
    fire_ms = [int(x) for x in re.findall(r"\[(\d+)\] fire \d+ ", h)]
    info = shot_info(d) if MODE != "time" else []
    tots = re.findall(r"st5 frm \d+ .* total (\d+)", h)
    final = int(tots[-1]) if tots else 0
    n = int((end - start) * FPS) + 1
    for i in range(n):
        t = start + i / FPS
        wall = (t * 1000 + v0 - t1) / 1000                # seconds since the first board frame (real time)
        g = max(0.0, min(wall, frames5 / 100))
        done = wall >= frames5 / 100
        # simple panel: level, name, timer (no records / master / seed)
        lines = [(f"{lv}", "big", (255, 210, 90)), (r.get("name", "")[len(lv) + 1:], "mid", (230, 230, 240)),
                 ("", "small", (0, 0, 0)),
                 # the TAS fixes the game's random seed at level start: it decides the peg colours and the luck
                 (f"RNG seed {seed}", "mid", (190, 220, 255)), ("sets peg colours & luck", "small", (130, 140, 170)),
                 ("", "small", (0, 0, 0))]
        if MODE == "time":
            lines.append((f"level {idx}/{total}", "small", (110, 110, 130)))
            panel(f"{pd}/{i:05d}.png", lines, f"{g:6.2f}", (120, 255, 140) if done else (255, 255, 255))
            continue
        lines += [(f"Master: {NAMES[ch]}", "mid", (190, 220, 255)), ("", "small", (0, 0, 0))]
        k_now = sum(1 for f in fire_ms if f <= t * 1000 + v0)        # shots fired so far
        if MODE == "score":
            lines += [(f"Shot {k_now}/{shots}", "small", (150, 150, 170))]
            sc = sum(x["score"] for x in info[:max(0, k_now - 1)]) if not done else final
            lines += [("Score", "small", (150, 150, 170)), (f"{sc:,}", "big", (120, 255, 140) if done else (255, 255, 255))]
            if done: lines += [("", "small", (0, 0, 0)), (f"Run total: {run_total + final:,}", "mid", (255, 210, 90))]
        else:
            # one caption block per finished shot (the newest 6), then the level-cleared line
            fin = info[:max(0, k_now - 1)] if not done else info
            for j, x in list(enumerate(fin, 1))[-4:]:
                lines.append((f"Shot {j}: {x['pegs']} pegs, {x['score']:,}", "small", (230, 230, 240)))
                extra = [STYLES.get(st, "") for st in x["styles"]] + ([f"{POWERS[ch]}" + (f" x{x['acts']}" if x['acts'] > 1 else "")] if x["acts"] else [])
                if x["hole"]: extra.append(f"Fever bucket {x['hole']:,}")
                cur = ""
                for e in [e for e in extra if e]:  # wrap the bonus names to the panel width
                    if cur and len(cur) + len(e) + 2 > 28: lines.append(("   " + cur + ",", "small", (255, 200, 120))); cur = e
                    else: cur = f"{cur}, {e}" if cur else e
                if cur: lines.append(("   " + cur, "small", (255, 200, 120)))
            if done:
                lines += [("", "small", (0, 0, 0)), (f"Cleared in {len(info)} shot{'s' if len(info) > 1 else ''}!", "mid", (120, 255, 140)),
                          (f"{sum(x['pegs'] for x in info)} pegs, {final:,} pts", "small", (230, 230, 240))]
        lines.append((f"level {idx}/{total}", "small", (110, 110, 130)))
        panel(f"{pd}/{i:05d}.png", lines, "", (0, 0, 0))
    o = f"{d}/final.mp4"
    # soundtrack: mixed offline from the recording's sound log (sound_mix.py); silence if there is none
    if os.path.exists(f"{d}/sound.txt") and not os.path.exists(f"{d}/sound.wav"):
        import sound_mix
        sound_mix.mix(d)
    snd = ["-ss", f"{start:.3f}", "-t", f"{end - start:.3f}", "-i", f"{d}/sound.wav"] if os.path.exists(f"{d}/sound.wav") else \
          ["-f", "lavfi", "-t", f"{end - start:.3f}", "-i", "anullsrc=r=48000:cl=stereo"]
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-ss", f"{start:.3f}", "-t", f"{end - start:.3f}", "-i", mp4,
                    "-framerate", str(FPS), "-i", f"{pd}/%05d.png", *snd,
                    "-filter_complex", "[0:v]fps=50,scale=1440:1080:flags=lanczos+accurate_rnd+full_chroma_int[g];[1:v]format=yuv444p[p];"
                    "[g][p]hstack=inputs=2,format=yuv420p[v]", "-map", "[v]", "-map", "2:a",
                    "-c:v", "libx264", "-preset", "slow", "-crf", "10", "-tune", "animation", "-profile:v", "high",
                    "-c:a", "aac", "-b:a", "192k", "-ar", "48000", "-ac", "2", "-shortest",
                    "-r", str(FPS), "-movflags", "+faststart", o], check=True)
    return o, (final if MODE == "score" else ours)


def main():
    out = sys.argv[1]
    levels = sys.argv[2:] or [f"{s}-{l}" for s in range(1, 12) for l in range(1, 6)]
    names = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "levels.json")))
    rec = {lv: {"name": f"{lv} {n}"} for lv, n in names.items()}
    levels = [lv for lv in levels if os.path.exists(f"{out}/video/{sl(lv)}/run.mp4")]
    parts, tot = [], 0.0
    for i, lv in enumerate(levels, 1):
        o, s = clip(out, lv, rec, i, len(levels), int(tot))
        if o is None: continue
        parts.append(o); tot += s
        print(lv, f"{s:.2f}" if MODE != "score" else f"{int(s):,}", flush=True)
    open(f"{out}/video_list.txt", "w").write("".join(f"file '{os.path.abspath(p)}'\n" for p in parts))
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i", f"{out}/video_list.txt",
                    "-c", "copy", f"{out}/{os.environ.get('NAME', 'tas')}.mp4"], check=True)
    print(f"wrote {out}/{os.environ.get('NAME', 'tas')}.mp4: {len(parts)} levels, sum " + (f"of scores {int(tot):,}" if MODE == "score" else f"of IL estimates {tot:.2f} s"))


if __name__ == "__main__":
    main()
