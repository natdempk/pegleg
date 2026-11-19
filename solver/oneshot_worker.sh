#!/bin/sh
# One-shot search worker (solver/src/bin/oneshot.rs): build the missing shot libraries of the given levels, then
# scan the queue (`oneshot next`) chunk by chunk until DIR/STOP exists. Machines share DIR by rsync of lib/,
# ledger/, cands/ and verified.tsv (each writes only its own ledger/TAG.tsv and cands/TAG.tsv); ORDER desc takes
# chunks from the top of the seed space, so two machines on one (level, class) do not overlap between syncs, and
# workers on one machine skip each other's chunks through DIR/claims.
# DIR/classes (optional, re-read every chunk): the classes to scan, e.g. free,tula,splork,multi.
# Env: ONESHOT_GPU=BIN scans on the GPU (solver/gpu/oneshot_gpu; only GPU-capable pairs); ONESHOT_NEXT=flags for
# `next` (e.g. --prefer-cpu: pairs the GPU cannot do first).
# CPU workers first build missing continuation libraries (multi; `next --build-only`): the GPU waits on them.
# ONESHOT_SCAN=0: a CPU worker builds (libraries, continuation libraries) first and scans only when idle and no
# DIR/cpu_busy.TAG marker is present (so the cores never sit idle until the job is done).
# GPU workers scan up to ONESHOT_BATCH (8) chunks of a pair per process (one CUDA init + spec upload), one chunk
# while the pair is still in phase A (no ledger line yet).
# Progress goes to DIR/worker.log (shared) and DIR/worker.TAG.log (every chunk).
# usage: solver/oneshot_worker.sh DIR TAG asc|desc LEVEL...   (RAYON_NUM_THREADS and nice are the caller's)
#   e.g. RAYON_NUM_THREADS=8 nohup nice solver/oneshot_worker.sh out/oneshot cpu1 asc 1-1 1-2 > /dev/null 2>&1 &
cd "$(dirname "$0")/.."
D=$1; TAG=$2; ORD=$3; shift 3
B=solver/target/release/oneshot
mkdir -p $D
echo $$ > $D/worker.$TAG.pid
log() { echo "$(date '+%m-%d %H:%M') oneshot $TAG: $*" >> $D/worker.log; }
for lv in "$@"; do
  [ -e $D/STOP ] && exit 0
  [ -s $D/lib/$lv.lib ] || log "lib $($B lib $D $lv --angles ${ONESHOT_ANGLES:-200000} --delays ${ONESHOT_DELAYS:-8} 2>>$D/worker.$TAG.err | tr '\t' ' ')"
done
[ $# -gt 0 ] && log "libraries done, scanning the queue"
O=; [ "$ORD" = desc ] && O=--desc
G=; NG=; [ -n "$ONESHOT_GPU" ] && { G="--gpu $ONESHOT_GPU"; NG=--gpu; }
while [ ! -e $D/STOP ]; do
  CL="$(cat $D/classes 2>/dev/null || echo free,tula,splork)"
  if [ -z "$ONESHOT_GPU" ]; then
    b=$($B next $D --build-only --classes "$CL")
    if [ -n "$b" ]; then set -- $b; log "$($B cont $D $1 $2 2>>$D/worker.$TAG.err)"; continue; fi
    # ONESHOT_SCAN=0: libraries first; with none to build the CPU scans too, unless DIR/cpu_busy.TAG exists (e.g.
    # another job is using this machine's cores)
    [ "$ONESHOT_SCAN" = 0 ] && [ -e $D/cpu_busy.$TAG ] && { sleep 60; continue; }
  fi
  n=$($B next $D $O $NG $ONESHOT_NEXT --classes "$CL")
  if [ -z "$n" ]; then sleep 300; continue; fi
  set -- $n
  M=1; [ -n "$ONESHOT_GPU" ] && [ "$4" != "1.000e9" ] && M=${ONESHOT_BATCH:-8}
  out=$($B scan $D $1 $2 --chunks 0-255 $O --max $M --batch $M --host $TAG $G 2>&1)
  r=$(echo "$out" | grep chunk | tr '\n' ';')
  echo "$(date '+%m-%d %H:%M') $r" >> $D/worker.$TAG.log
  case "$r" in *"best 25"*) log "$r";; esac
  # no chunk scanned (GPU failure, ...): report it and back off instead of retrying the same pair at once
  if [ -z "$r" ]; then log "scan $1 $2 scanned nothing: $(echo "$out" | grep -v '^ *$' | tail -2 | tr '\n' ' ')"; sleep 120; fi
done
log "stopped (STOP file)"
