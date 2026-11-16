#!/usr/bin/env python3
"""Video script: the harness script with `cursor X Y` (the shot's window pixel, from the plan) before each
`shotx`, so the recording shows the cursor moving to each aim point. game_check / tas_play ignore the line.

usage: cursor_script.py SCRIPT PLAN OUT
"""
import re, sys


def main():
    script, plan, out = sys.argv[1:4]
    px = [(int(m.group(1)), int(m.group(2))) for m in re.finditer(r"^\d+: pixel \((\d+), (\d+)\)", open(plan).read(), re.M)]
    lines, k = [], 0
    for line in open(script).read().splitlines():
        if line.startswith("shotx") and k < len(px):
            lines.append(f"cursor {px[k][0]} {px[k][1]}"); k += 1
        lines.append(line)
    open(out, "w").write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
