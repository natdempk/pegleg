#!/usr/bin/env python3
"""Mix a recording's soundtrack offline from the hook's sound log (PEGGLE_SOUNDLOG=1): RUN/sound.txt lists
every DirectSound play / stop / volume / pan / frequency change with its wall-clock ms (the harness log's
clock), RUN/snd/<hash>.pcm holds each buffer's PCM. Writes RUN/sound.wav (48 kHz stereo 16-bit) aligned to
RUN/run.mp4 t=0 (wall ms of t=0 = the harness 'quit' ms minus the video duration, as in video.py).
Needs numpy.

usage: sound_mix.py RUN [RUN ...]
"""
import os, re, subprocess, sys, wave
import numpy as np

OUT_RATE = 48000


def gain(vol):  # DirectSound volume: hundredths of a dB (0 = full, -10000 = silent)
    return 0.0 if vol <= -10000 else 10 ** (vol / 2000)


def pan_gains(pan):  # pan > 0 attenuates the left channel, < 0 the right
    return (gain(-pan) if pan > 0 else 1.0), (gain(pan) if pan < 0 else 1.0)


def mix(run):
    h = open(f"{run}/harness.txt").read()
    quit_ms = int(re.search(r"\[(\d+)\] quit", h).group(1))
    dur = float(subprocess.check_output(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", f"{run}/run.mp4"]))
    v0 = quit_ms - dur * 1000
    n_out = int(dur * OUT_RATE) + 1
    out = np.zeros((n_out, 2), np.float64)
    pcm = {}
    # per buffer id: the voice playing now (start ms, data, channels, segments of (ms, freq, vol, pan))
    voices, done = {}, []
    for line in open(f"{run}/sound.txt"):
        w = line.split()
        if len(w) < 3: continue
        ms, ev = int(w[0]), w[2]
        if ev == "dplay":
            bid, hsh = w[3], w[4]
            kv = dict(zip(w[5::2], w[6::2]))
            if bid in voices: done.append(voices.pop(bid) + (ms,))
            voices[bid] = (ms, hsh, int(kv["ch"]), int(kv["bits"]), int(kv["loop"]), int(kv["pos"]),
                           [(ms, int(kv["freq"]), int(kv["vol"]), int(kv["pan"]))])
        elif ev in ("dstop", "dvol", "dpan", "dfreq", "dpos") and w[3] in voices:
            v = voices[w[3]]
            if ev == "dstop": done.append(voices.pop(w[3]) + (ms,)); continue
            t, f, vo, pa = v[6][-1]
            if ev == "dvol": v[6].append((ms, f, int(w[4]), pa))
            elif ev == "dpan": v[6].append((ms, f, vo, int(w[4])))
            elif ev == "dfreq": v[6].append((ms, int(w[4]), vo, pa))
    for v in voices.values(): done.append(v + (None,))
    for start, hsh, ch, bits, loop, pos, segs, stop in done:
        if hsh not in pcm:
            p = f"{run}/snd/{hsh}.pcm"
            if not os.path.exists(p): continue
            raw = open(p, "rb").read()
            a = (np.frombuffer(raw, np.int16).astype(np.float64) / 32768) if bits == 16 else \
                ((np.frombuffer(raw, np.uint8).astype(np.float64) - 128) / 128)
            pcm[hsh] = a.reshape(-1, ch) if ch > 1 else np.repeat(a[:, None], 2, axis=1)
        src = pcm[hsh]
        n_src = len(src)
        if n_src == 0: continue
        sp = pos // (ch * bits // 8)  # source frame position
        for k, (t, f, vo, pa) in enumerate(segs):
            t_end = segs[k + 1][0] if k + 1 < len(segs) else (stop if stop is not None else None)
            o0 = int(round((t - v0) / 1000 * OUT_RATE))
            step = f / OUT_RATE                      # source frames per output frame
            if t_end is None:
                n = int((n_src - sp) / step) + 1 if not loop else n_out - o0
            else:
                n = int((t_end - t) / 1000 * OUT_RATE)
            if not loop: n = min(n, int((n_src - sp) / step) + 1)
            n = max(0, min(n, n_out - o0))
            if n <= 0 or o0 >= n_out:
                sp += n * step; continue
            x = sp + np.arange(n) * step
            if loop: x = np.mod(x, n_src)
            i0 = np.floor(x).astype(np.int64); fr = (x - i0)[:, None]
            i0 = np.clip(i0, 0, n_src - 1); i1 = np.mod(i0 + 1, n_src) if loop else np.clip(i0 + 1, 0, n_src - 1)
            seg = src[i0] * (1 - fr) + src[i1] * fr
            gl, gr = pan_gains(pa); g = gain(vo)
            seg = seg * np.array([g * gl, g * gr])
            lo = max(0, o0); seg = seg[lo - o0:]
            out[lo:lo + len(seg)] += seg
            sp += n * step
            if not loop and sp >= n_src: break
    peak = np.abs(out).max()
    out = np.tanh(out * 0.8)  # fixed gain + soft limit (dense peg runs stack many voices)
    with wave.open(f"{run}/sound.wav", "wb") as wf:
        wf.setnchannels(2); wf.setsampwidth(2); wf.setframerate(OUT_RATE)
        wf.writeframes((out * 32767).astype(np.int16).tobytes())
    return len(done), peak


def main():
    for run in sys.argv[1:]:
        n, peak = mix(run)
        print(f"{run}: {n} voices, peak {peak:.2f} → {run}/sound.wav", flush=True)


if __name__ == "__main__":
    main()
