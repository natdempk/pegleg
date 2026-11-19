// One-shot seed scan on the GPU (CUDA): exact port of oneshot.rs Scanner::eval for the classes that need no
// per-tick positions (free, multi, and tula / splork on static boards). One thread per seed. The spec file is
// written by oneshot.rs export_spec; output: "hist h0..h25", then "e SEED BEST K" for every seed at the floor.
// Build: nvcc -O3 -arch=sm_86 -fmad=false -o oneshot_gpu oneshot_gpu.cu  (no FMA: the f32 green-distance test
// must round like the CPU's)
// usage: oneshot_gpu SPEC FROM TO [FROM TO ...]   (several ranges share one CUDA init and spec upload;
// each prints "range FROM TO", "hist ...", "e ..." lines). OSG2 adds Tula / Splork on boards with moving pegs.
#include <cstdio>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <vector>
typedef unsigned long long u64;
typedef unsigned int u32;
typedef unsigned char u8;

struct Spec {
    u32 cid, npeg, nvars, nfixed, floor, near_, nlib;
    u8 vars[256];
    u64 fixed[4], coll[4], var[4], orangeable[4];
    float pos0[256][2];
    u32 norder;
    u64 *sets; u32 *sz, *kk;            // order
    u32 *inv_off, *inv;                 // per peg: positions in order
    u32 *near_off, *nearl;              // tula static: pegs by distance
    u64 (*nstat)[4]; u64 (*nmax)[4];    // splork static
    u32 nconts; u64 *pre, *cset; u32 *cinv_off, *cinv;  // multi: (sz, k) pairs
    u32 *hit_off; u8 *hits;             // library shots' ordered hits
    u32 moving, nrows, hit_tot; int t0;          // tula / splork on boards with moving pegs (OSG2)
    u32 *obj; float (*rows)[256][2]; unsigned short* ticks;  // object order (tula ties), positions per tick, hit ticks
    u64 (*mblast)[4];                   // splork moving: blast set per (row, peg), built once on the GPU
    u8* mnear; u32 mncand;              // tula moving: per (row, peg) the coll orangeable pegs by distance (mncand each)
};

__device__ __forceinline__ bool bget(const u64* b, u32 i) { return (b[i >> 6] >> (i & 63)) & 1; }
__device__ __forceinline__ void bset(u64* b, u32 i) { b[i >> 6] |= 1ull << (i & 63); }
__device__ __forceinline__ u32 pc_and(const u64* a, const u64* b) {
    return __popcll(a[0] & b[0]) + __popcll(a[1] & b[1]) + __popcll(a[2] & b[2]) + __popcll(a[3] & b[3]);
}
__device__ __forceinline__ u32 tas_mix(u32 a, u32 b) {
    u32 s = (a * 0x9E3779B1u) ^ (b * 0x85EBCA6Bu + 0x68E31DA4u);
    s ^= s >> 15; s *= 0x2C1B3C6Du; s ^= s >> 12;
    return s ? s : 1;
}
__device__ __forceinline__ u32 temper(u32 y) {
    y ^= y >> 11; y ^= (y << 7) & 0x9d2c5680u; y ^= (y << 15) & 0xefc60000u; y ^= y >> 18;
    return y & 0x7fffffffu;
}
__device__ __forceinline__ void mt_init(u32* mt, u32 seed, int upto) {
    mt[0] = seed ? seed : 0x1105;
    for (int i = 1; i < upto; i++) mt[i] = (mt[i - 1] ^ (mt[i - 1] >> 30)) * 0x6c078965u + (u32)i;
}
__device__ __forceinline__ void mt_twist(u32* mt) {
    for (int i = 0; i < 624; i++) {
        u32 y = (mt[i] & 0x80000000u) | (mt[i + 1 < 624 ? i + 1 : 0] & 0x7fffffffu);
        int j = i + 397; if (j >= 624) j -= 624;
        mt[i] = mt[j] ^ (y >> 1) ^ ((y & 1) ? 0x9908b0dfu : 0u);
    }
}
// the hook's 1000-swap shuffle of v[0..n) with MT seeded tas_mix(seed, call)
__device__ void shuffle(u32 seed, u32 call, u8* v, u32 n, u32* mt) {
    if (n < 2) return;
    mt_init(mt, tas_mix(seed, call), 624);
    u64 m = (~0ull) / n + 1;
    int k = 0; u32 a = 0;
    for (int blk = 0; blk < 4 && k < 2000; blk++) {
        mt_twist(mt);
        for (int i = 0; i < 624 && k < 2000; i++, k++) {
            u32 r = (u32)__umul64hi(m * (u64)temper(mt[i]), (u64)n);
            if (k & 1) { u8 t = v[a]; v[a] = v[r]; v[r] = t; } else a = r;
        }
    }
}

__device__ bool pass1(const Spec& S, const u64* o, const u64* skip, u32& best, u32& bi) {
    u32 need = best >= 25 ? 0 : 25 - best;
    u32 rl[32], rp[32]; int nr = 0;
    for (int x = 0; x < 4; x++) { u64 w = o[x]; while (w) { u32 p = x * 64 + __ffsll(w) - 1; w &= w - 1; if (nr < 32) { rl[nr] = S.inv_off[p + 1] - S.inv_off[p]; rp[nr] = p; nr++; } } }
    for (int i = 1; i < nr; i++) { u32 l = rl[i], p = rp[i]; int j = i - 1; while (j >= 0 && (rl[j] > l || (rl[j] == l && rp[j] > p))) { rl[j + 1] = rl[j]; rp[j + 1] = rp[j]; j--; } rl[j + 1] = l; rp[j + 1] = p; }
    bool has_skip = (skip[0] | skip[1] | skip[2] | skip[3]) != 0;
    for (u32 t = 0; t < need && t < (u32)nr; t++) {
        u32 p = rp[t];
        for (u32 q = S.inv_off[p]; q < S.inv_off[p + 1]; q++) {
            u32 j = S.inv[q];
            const u64* h = S.sets + 4 * (u64)j;
            u32 sz = S.sz[j]; if ((sz < 25 ? sz : 25) <= best) break;
            if (has_skip && pc_and(h, skip) > 0) continue;
            u32 c = pc_and(h, o);
            if (c > best) { best = c; bi = S.kk[j]; if (c >= 25) return true; }
        }
    }
    return false;
}

__device__ u32 tula_eval(const Spec& S, u32 k, const u64* o, const u64* gr) {
    u64 lit[4] = {0, 0, 0, 0};
    int left = __popcll(o[0]) + __popcll(o[1]) + __popcll(o[2]) + __popcll(o[3]);
    for (u32 q = S.hit_off[k]; q < S.hit_off[k + 1]; q++) {
        u32 p = S.hits[q];
        if (bget(lit, p)) continue;
        bset(lit, p);
        if (bget(o, p)) left--;
        if (!bget(gr, p)) continue;
        u32 kk = (u32)ceilf((float)((double)(float)left * 0.20000000298023224));
        for (u32 r = S.near_off[p]; r < S.near_off[p + 1] && kk > 0; r++) {
            u32 x = S.nearl[r];
            if (bget(o, x) && !bget(lit, x)) { bset(lit, x); left--; kk--; }
        }
    }
    return pc_and(lit, o);
}

// power_eval for boards with moving pegs: pegs at the hit's tick, distances in f64 of f32 differences (as the CPU)
__device__ __forceinline__ double d2(const float* a, const float* b) {
    double dx = (double)__fsub_rn(a[0], b[0]), dy = (double)__fsub_rn(a[1], b[1]);
    return __dadd_rn(__dmul_rn(dx, dx), __dmul_rn(dy, dy));
}
// per-tick tables (once per spec): the CPU's per-candidate distance work, done once per (row, peg) with its rules
__global__ void kPrep(Spec S) {
    u32 r = blockIdx.x, p = threadIdx.x;
    if (p >= S.npeg) return;
    const float (*ps)[2] = S.rows[r];
    if (S.cid == 2) {
        u64 b[4] = {0, 0, 0, 0};
        for (u32 c = 0; c < S.npeg; c++) if (bget(S.coll, c) && !(d2(ps[c], ps[p]) > 10000.0)) bset(b, c);
        for (int x = 0; x < 4; x++) S.mblast[(size_t)r * 256 + p][x] = b[x];
    } else {
        u8* out = S.mnear + ((size_t)r * S.npeg + p) * S.mncand;
        u32 n = 0;
        for (u32 c = 0; c < S.npeg; c++) {
            if (!bget(S.coll, c) || !bget(S.orangeable, c)) continue;
            float d = (float)d2(ps[c], ps[p]); u32 ob = S.obj[c];
            u32 j = n++;   // insertion by (distance, object order, peg)
            while (j > 0) {
                u32 y = out[j - 1]; float dy = (float)d2(ps[y], ps[p]); u32 oy = S.obj[y];
                if (dy < d || (dy == d && (oy < ob || (oy == ob && y < c)))) break;
                out[j] = (u8)y; j--;
            }
            out[j] = (u8)c;
        }
    }
}
__device__ u32 moving_eval(const Spec& S, u32 k, const u64* o, const u64* gr) {
    u64 lit[4] = {0, 0, 0, 0};
    int left = __popcll(o[0]) + __popcll(o[1]) + __popcll(o[2]) + __popcll(o[3]);
    for (u32 q = S.hit_off[k]; q < S.hit_off[k + 1]; q++) {
        u32 p = S.hits[q];
        if (bget(lit, p)) continue;
        bset(lit, p);
        if (bget(o, p)) left--;
        if (!bget(gr, p)) continue;
        int r = (int)S.ticks[q] - S.t0; if (r < 0) r = 0; if (r > (int)S.nrows - 1) r = S.nrows - 1;
        if (S.cid == 1) {
            u32 kk = (u32)ceilf((float)((double)(float)left * 0.20000000298023224));
            const u8* nl = S.mnear + ((size_t)r * S.npeg + p) * S.mncand;
            for (u32 i = 0; i < S.mncand && kk > 0; i++) {
                u32 x = nl[i];
                if (bget(o, x) && !bget(lit, x)) { bset(lit, x); left--; kk--; }
            }
        } else {
            const u64 (*bl)[4] = S.mblast + (size_t)r * 256;
            u32 stk[8]; int ns = 0; stk[ns++] = p;
            while (ns > 0) {
                u32 g = stk[--ns];
                u64 nw[4];
                for (int x = 0; x < 4; x++) { nw[x] = bl[g][x] & ~lit[x]; lit[x] |= nw[x]; }
                for (int x = 0; x < 4; x++) { u64 w = nw[x] & gr[x]; while (w && ns < 8) { stk[ns++] = x * 64 + __ffsll(w) - 1; w &= w - 1; } }
            }
        }
    }
    return pc_and(lit, o);
}

__device__ u32 g_minres;   // output only seeds with res >= this (see main)
__global__ void scan(Spec S, u64 from, u64 n, unsigned long long* hist, u32* out, u32* nout, u32 cap) {
    u64 t = (u64)blockIdx.x * blockDim.x + threadIdx.x;
    if (t >= n) return;
    u32 seed = (u32)(from + t);
    u32 mt[624];
    u8 v[256];
    // oranges
    for (u32 i = 0; i < S.nvars; i++) v[i] = S.vars[i];
    shuffle(seed, 0, v, S.nvars, mt);
    u64 o[4] = {S.fixed[0], S.fixed[1], S.fixed[2], S.fixed[3]};
    u32 need = S.nfixed >= 25 ? 0 : 25 - S.nfixed;
    for (u32 i = 0; i < need && i < S.nvars; i++) bset(o, v[S.nvars - 1 - i]);
    u64 gr[4] = {0, 0, 0, 0};
    if (S.cid != 0) {
        // purple (one draw, n = 1), then greens (n = 2)
        u32 ncand = 0;
        for (u32 b = 0; b < S.npeg; b++) if (bget(S.coll, b) && !bget(o, b)) ncand++;
        u32 purple = 0xffffffffu;
        if (ncand > 0) {
            mt_init(mt, tas_mix(seed, 1), 398);
            u32 y = (mt[0] & 0x80000000u) | (mt[1] & 0x7fffffffu);
            u32 r = temper(mt[397] ^ (y >> 1) ^ ((y & 1) ? 0x9908b0dfu : 0u));
            u32 kx = r % ncand, c = 0;
            for (u32 b = 0; b < S.npeg; b++) if (bget(S.coll, b) && !bget(o, b)) { if (c == kx) { purple = b; break; } c++; }
        }
        u32 nv = 0;
        for (u32 b = 0; b < S.npeg; b++) if (bget(S.coll, b) && !bget(o, b) && b != purple && S.pos0[b][1] < 370.0f) v[nv++] = b;
        if (nv == 0) for (u32 b = 0; b < S.npeg; b++) if (bget(S.coll, b) && !bget(o, b) && b != purple && bget(S.var, b)) v[nv++] = b;
        if (nv > 0) {
            shuffle(seed, 2, v, nv, mt);
            int needg = 2;
            float lx = -1000.0f, ly = -1000.0f;
            for (int pass = 0; pass < 2 && needg > 0; pass++) {
                for (u32 i = 0; i < nv; i++) {
                    if (needg <= 0) break;
                    u32 b = v[i];
                    if (bget(gr, b)) continue;
                    if (pass == 0) {
                        float dx = __fsub_rn(lx, S.pos0[b][0]), dy = __fsub_rn(ly, S.pos0[b][1]);
                        float s = __fadd_rn(__fmul_rn(dx, dx), __fmul_rn(dy, dy));
                        if (22500.0f > s) continue;
                        lx = S.pos0[b][0]; ly = S.pos0[b][1];
                    }
                    bset(gr, b); needg--;
                }
            }
        }
    }
    u32 best = S.floor ? S.floor - 1 : 0, bi = 0xffffffffu;
    const u64 zero[4] = {0, 0, 0, 0};
    bool done = false;
    if (S.cid == 3) {
        done = pass1(S, o, gr, best, bi);
        for (u32 g = 0; g < S.npeg && !done; g++) {
            if (!bget(gr, g)) continue;
            for (u32 q = S.cinv_off[g]; q < S.cinv_off[g + 1]; q++) {
                u32 sz = S.cinv[2 * q], k = S.cinv[2 * q + 1];
                if ((sz < 25 ? sz : 25) <= best) break;
                if (pc_and(S.pre + 4 * (u64)k, gr) > 0) continue;
                u32 c = pc_and(S.cset + 4 * (u64)k, o);
                if (c > best) { best = c; bi = S.nlib + k; if (best >= 25) { done = true; break; } }
            }
        }
    } else {
        u64 o2[4] = {o[0], o[1], o[2], o[3]}; u32 extra = 0;
        if (S.cid == 1) extra = 10;
        if (S.cid == 2) {
            u64 bb[4] = {0, 0, 0, 0};
            for (u32 g = 0; g < S.npeg; g++) if (bget(gr, g)) for (int x = 0; x < 4; x++) bb[x] |= S.nmax[g][x];
            for (int x = 0; x < 4; x++) o2[x] &= ~bb[x];
            extra = pc_and(o, bb);
        }
        done = pass1(S, o, zero, best, bi);
        if (S.cid != 0) {
            for (u32 g = 0; g < S.npeg && !done; g++) {
                if (!bget(gr, g)) continue;
                for (u32 q = S.inv_off[g]; q < S.inv_off[g + 1]; q++) {
                    u32 j = S.inv[q];
                    const u64* h = S.sets + 4 * (u64)j;
                    u32 sz = S.sz[j], k = S.kk[j];
                    if ((sz < 25 ? sz : 25) + extra <= best) break;
                    u32 c0 = pc_and(h, o);
                    if (c0 + extra <= best) continue;
                    if (S.cid == 2 && pc_and(h, o2) + extra <= best) continue;
                    if (S.cid == 1 && c0 + 5 * pc_and(h, gr) <= best) continue;
                    u32 c;
                    if (S.cid == 2) {
                        u64 u[4], act[4], dn[4] = {0, 0, 0, 0};
                        for (int x = 0; x < 4; x++) { u[x] = h[x]; act[x] = h[x] & gr[x]; }
                        while (true) {
                            int x0 = -1; for (int x = 0; x < 4; x++) if (act[x] & ~dn[x]) { x0 = x; break; }
                            if (x0 < 0) break;
                            u32 b = x0 * 64 + __ffsll(act[x0] & ~dn[x0]) - 1;
                            bset(dn, b);
                            for (int x = 0; x < 4; x++) { u[x] |= S.nstat[b][x]; act[x] |= S.nstat[b][x] & gr[x]; }
                        }
                        c = pc_and(u, o);
                    } else c = tula_eval(S, k, o, gr);
                    if (c > best) { best = c; bi = k; if (best >= 25) { done = true; break; } }
                }
            }
        }
    }
    u32 res = bi == 0xffffffffu ? 0 : best;
    atomicAdd(&hist[res > 25 ? 25 : res], 1ull);
    if (bi != 0xffffffffu && res >= g_minres) { u32 i = atomicAdd(nout, 1u); if (i < cap) { out[3 * i] = seed; out[3 * i + 1] = res; out[3 * i + 2] = bi; } }
}

// ---- v2: bucketed pass 2 (power classes). State per seed of a batch: oranges, greens (lo, hi), best, bi.
struct St { u64 o[4]; u32 best, bi; u8 g[2]; u8 done, pad; };

__device__ void colours(const Spec& S, u32 seed, u64* o, u64* gr, u32* mt, u8* v) {
    for (u32 i = 0; i < S.nvars; i++) v[i] = S.vars[i];
    shuffle(seed, 0, v, S.nvars, mt);
    for (int x = 0; x < 4; x++) { o[x] = S.fixed[x]; gr[x] = 0; }
    u32 need = S.nfixed >= 25 ? 0 : 25 - S.nfixed;
    for (u32 i = 0; i < need && i < S.nvars; i++) bset(o, v[S.nvars - 1 - i]);
    u32 ncand = 0;
    for (u32 b = 0; b < S.npeg; b++) if (bget(S.coll, b) && !bget(o, b)) ncand++;
    u32 purple = 0xffffffffu;
    if (ncand > 0) {
        mt_init(mt, tas_mix(seed, 1), 398);
        u32 y = (mt[0] & 0x80000000u) | (mt[1] & 0x7fffffffu);
        u32 r = temper(mt[397] ^ (y >> 1) ^ ((y & 1) ? 0x9908b0dfu : 0u));
        u32 kx = r % ncand, c = 0;
        for (u32 b = 0; b < S.npeg; b++) if (bget(S.coll, b) && !bget(o, b)) { if (c == kx) { purple = b; break; } c++; }
    }
    u32 nv = 0;
    for (u32 b = 0; b < S.npeg; b++) if (bget(S.coll, b) && !bget(o, b) && b != purple && S.pos0[b][1] < 370.0f) v[nv++] = b;
    if (nv == 0) for (u32 b = 0; b < S.npeg; b++) if (bget(S.coll, b) && !bget(o, b) && b != purple && bget(S.var, b)) v[nv++] = b;
    if (nv == 0) return;
    shuffle(seed, 2, v, nv, mt);
    int needg = 2;
    float lx = -1000.0f, ly = -1000.0f;
    for (int pass = 0; pass < 2 && needg > 0; pass++) {
        for (u32 i = 0; i < nv; i++) {
            if (needg <= 0) break;
            u32 b = v[i];
            if (bget(gr, b)) continue;
            if (pass == 0) {
                float dx = __fsub_rn(lx, S.pos0[b][0]), dy = __fsub_rn(ly, S.pos0[b][1]);
                float s = __fadd_rn(__fmul_rn(dx, dx), __fmul_rn(dy, dy));
                if (22500.0f > s) continue;
                lx = S.pos0[b][0]; ly = S.pos0[b][1];
            }
            bset(gr, b); needg--;
        }
    }
}

__global__ void kA(Spec S, u64 from, u32 n, St* st, u32* cnt) {
    u32 t = blockIdx.x * blockDim.x + threadIdx.x;
    if (t >= n) return;
    u32 seed = (u32)(from + t);
    u32 mt[624]; u8 v[256];
    St s; u64 gr[4];
    colours(S, seed, s.o, gr, mt, v);
    s.g[0] = s.g[1] = 0xff; int ng = 0;
    for (int x = 0; x < 4; x++) { u64 w = gr[x]; while (w) { u32 b = x * 64 + __ffsll(w) - 1; w &= w - 1; if (ng < 2) s.g[ng++] = (u8)b; } }
    s.best = S.floor ? S.floor - 1 : 0; s.bi = 0xffffffffu;
    const u64 zero[4] = {0, 0, 0, 0};
    s.done = pass1(S, s.o, S.cid == 3 ? gr : zero, s.best, s.bi) ? 1 : 0;
    st[t] = s;
    if (!s.done && s.g[0] != 0xff) atomicAdd(&cnt[s.g[0]], 1u);
}

// seeds of phase ph (0: lower green, 1: higher green) into per-green buckets
__global__ void kScatter(u32 n, const St* st, int ph, u32* fill, u32* idx) {
    u32 t = blockIdx.x * blockDim.x + threadIdx.x;
    if (t >= n) return;
    const St& s = st[t];
    if (s.done || s.g[ph] == 0xff) return;
    u32 p = atomicAdd(&fill[s.g[ph]], 1u);
    idx[p] = t;
}
__global__ void kCount(u32 n, const St* st, int ph, u32* cnt) {
    u32 t = blockIdx.x * blockDim.x + threadIdx.x;
    if (t >= n) return;
    const St& s = st[t];
    if (!s.done && s.g[ph] != 0xff) atomicAdd(&cnt[s.g[ph]], 1u);
}

#define TILE 128
// one block = up to 128 seeds sharing green g; the green's list streams through shared memory in tiles
__global__ void kB(Spec S, St* st, const u32* idx, const u32* bg, const u32* bstart, const u32* bcount) {
    __shared__ u64 tset[TILE][4];
    __shared__ u64 tpre[TILE][4];
    __shared__ u32 tsz[TILE], tk[TILE];
    u32 g = bg[blockIdx.x], start = bstart[blockIdx.x], cnt = bcount[blockIdx.x];
    bool mine = threadIdx.x < cnt;
    u32 si = mine ? idx[start + threadIdx.x] : 0;
    St s; if (mine) s = st[si]; else { s.done = 1; s.best = 99; }
    u64 gr[4] = {0, 0, 0, 0};
    if (mine) { bset(gr, s.g[0]); if (s.g[1] != 0xff) bset(gr, s.g[1]); }
    u64 o2[4] = {s.o[0], s.o[1], s.o[2], s.o[3]}; u32 extra = 0;
    if (mine && S.cid == 1) extra = 10;
    if (mine && S.cid == 2) {
        u64 bb[4] = {0, 0, 0, 0};
        for (int q = 0; q < 2; q++) if (s.g[q] != 0xff) for (int x = 0; x < 4; x++) bb[x] |= S.nmax[s.g[q]][x];
        for (int x = 0; x < 4; x++) o2[x] &= ~bb[x];
        extra = pc_and(s.o, bb);
    }
    bool active = mine && !s.done;
    u32 lo, hi;
    if (S.cid == 3) { lo = S.cinv_off[g]; hi = S.cinv_off[g + 1]; } else { lo = S.inv_off[g]; hi = S.inv_off[g + 1]; }
    for (u32 base = lo; base < hi; base += TILE) {
        if (!__syncthreads_or(active)) break;
        u32 e = base + threadIdx.x;
        if (threadIdx.x < TILE && e < hi) {
            if (S.cid == 3) {
                u32 sz = S.cinv[2 * e], k = S.cinv[2 * e + 1];
                tsz[threadIdx.x] = sz; tk[threadIdx.x] = k;
                for (int x = 0; x < 4; x++) { tset[threadIdx.x][x] = S.cset[4 * (u64)k + x]; tpre[threadIdx.x][x] = S.pre[4 * (u64)k + x]; }
            } else {
                u32 j = S.inv[e];
                tsz[threadIdx.x] = S.sz[j]; tk[threadIdx.x] = S.kk[j];
                for (int x = 0; x < 4; x++) tset[threadIdx.x][x] = S.sets[4 * (u64)j + x];
            }
        }
        __syncthreads();
        u32 m = hi - base < TILE ? hi - base : TILE;
        for (u32 q = 0; q < m && active; q++) {
            u32 sz = tsz[q], k = tk[q];
            const u64* h = tset[q];
            if (S.cid == 3) {
                if ((sz < 25 ? sz : 25) <= s.best) { active = false; break; }
                if (pc_and(tpre[q], gr) > 0) continue;
                u32 c = pc_and(h, s.o);
                if (c > s.best) { s.best = c; s.bi = S.nlib + k; if (c >= 25) { s.done = 1; active = false; } }
                continue;
            }
            if ((sz < 25 ? sz : 25) + extra <= s.best) { active = false; break; }
            u32 c0 = pc_and(h, s.o);
            if (c0 + extra <= s.best) continue;
            if (S.cid == 2 && pc_and(h, o2) + extra <= s.best) continue;
            if (S.cid == 1 && c0 + 5 * pc_and(h, gr) <= s.best) continue;
            u32 c;
            if (S.moving) c = moving_eval(S, k, s.o, gr);
            else if (S.cid == 2) {
                u64 u[4], act[4], dn[4] = {0, 0, 0, 0};
                for (int x = 0; x < 4; x++) { u[x] = h[x]; act[x] = h[x] & gr[x]; }
                while (true) {
                    int x0 = -1; for (int x = 0; x < 4; x++) if (act[x] & ~dn[x]) { x0 = x; break; }
                    if (x0 < 0) break;
                    u32 b = x0 * 64 + __ffsll(act[x0] & ~dn[x0]) - 1;
                    bset(dn, b);
                    for (int x = 0; x < 4; x++) { u[x] |= S.nstat[b][x]; act[x] |= S.nstat[b][x] & gr[x]; }
                }
                c = pc_and(u, s.o);
            } else c = tula_eval(S, k, s.o, gr);
            if (c > s.best) { s.best = c; s.bi = k; if (c >= 25) { s.done = 1; active = false; } }
        }
        __syncthreads();
    }
    if (mine) st[si] = s;
}

__global__ void kF(u64 from, u32 n, const St* st, unsigned long long* hist, u32* out, u32* nout, u32 cap) {
    u32 t = blockIdx.x * blockDim.x + threadIdx.x;
    if (t >= n) return;
    const St& s = st[t];
    u32 res = s.bi == 0xffffffffu ? 0 : s.best;
    atomicAdd(&hist[res > 25 ? 25 : res], 1ull);
    if (s.bi != 0xffffffffu && res >= g_minres) { u32 i = atomicAdd(nout, 1u); if (i < cap) { out[3 * i] = (u32)(from + t); out[3 * i + 1] = res; out[3 * i + 2] = s.bi; } }
}

template <class T> T* up(const std::vector<u8>& b, size_t& p, size_t n) {
    T* d; cudaMalloc(&d, n * sizeof(T) + 16); cudaMemcpy(d, b.data() + p, n * sizeof(T), cudaMemcpyHostToDevice); p += n * sizeof(T); return d;
}
static u32 rd32(const std::vector<u8>& b, size_t& p) { u32 x; memcpy(&x, b.data() + p, 4); p += 4; return x; }

int main(int argc, char** argv) {
    if (argc < 4) { fprintf(stderr, "usage: oneshot_gpu SPEC FROM TO\n"); return 2; }
    FILE* f = fopen(argv[1], "rb"); if (!f) { perror(argv[1]); return 1; }
    std::vector<u8> b; { u8 buf[1 << 16]; size_t r; while ((r = fread(buf, 1, sizeof buf, f)) > 0) b.insert(b.end(), buf, buf + r); fclose(f); }
    if (memcmp(b.data(), "OSG2", 4)) { fprintf(stderr, "bad spec\n"); return 1; }
    size_t p = 4; Spec S;
    S.cid = rd32(b, p); S.npeg = rd32(b, p); S.nvars = rd32(b, p); S.nfixed = rd32(b, p); S.floor = rd32(b, p); S.near_ = rd32(b, p); S.nlib = rd32(b, p);
    memcpy(S.vars, b.data() + p, 256); p += 256;
    memcpy(S.fixed, b.data() + p, 32); p += 32; memcpy(S.coll, b.data() + p, 32); p += 32; memcpy(S.var, b.data() + p, 32); p += 32; memcpy(S.orangeable, b.data() + p, 32); p += 32;
    memcpy(S.pos0, b.data() + p, 2048); p += 2048;
    S.norder = rd32(b, p);
    S.sets = up<u64>(b, p, 4 * (size_t)S.norder); S.sz = up<u32>(b, p, S.norder); S.kk = up<u32>(b, p, S.norder);
    { size_t q = p + 4 * 256; u32 tot; memcpy(&tot, b.data() + q, 4); S.inv_off = up<u32>(b, p, 257); S.inv = up<u32>(b, p, tot); }
    { size_t q = p + 4 * 256; u32 tot; memcpy(&tot, b.data() + q, 4); S.near_off = up<u32>(b, p, 257); S.nearl = up<u32>(b, p, tot); }
    S.nstat = (u64(*)[4])up<u64>(b, p, 1024); S.nmax = (u64(*)[4])up<u64>(b, p, 1024);
    S.nconts = rd32(b, p);
    S.pre = up<u64>(b, p, 4 * (size_t)S.nconts); S.cset = up<u64>(b, p, 4 * (size_t)S.nconts);
    { size_t q = p + 4 * 256; u32 tot; memcpy(&tot, b.data() + q, 4); S.cinv_off = up<u32>(b, p, 257); S.cinv = up<u32>(b, p, 2 * (size_t)tot); }
    u32 nl = rd32(b, p);
    { size_t q = p + 4 * (size_t)nl; u32 tot; memcpy(&tot, b.data() + q, 4); S.hit_off = up<u32>(b, p, nl + 1); S.hits = up<u8>(b, p, tot); S.hit_tot = tot; }
    S.moving = rd32(b, p);
    S.obj = up<u32>(b, p, 256);
    S.nrows = rd32(b, p); S.t0 = (int)rd32(b, p);
    S.rows = (float(*)[256][2])up<float>(b, p, (size_t)S.nrows * 512);
    S.ticks = S.moving ? up<unsigned short>(b, p, S.hit_tot) : nullptr;
    S.mblast = 0; S.mnear = 0; S.mncand = 0;
    if (S.moving) {
        for (u32 q = 0; q < S.npeg; q++) if (((S.coll[q >> 6] & S.orangeable[q >> 6]) >> (q & 63)) & 1) S.mncand++;
        if (S.cid == 2) cudaMalloc(&S.mblast, (size_t)S.nrows * 256 * 32);
        else cudaMalloc(&S.mnear, (size_t)S.nrows * S.npeg * S.mncand + 16);
        kPrep<<<S.nrows, 256>>>(S);
        cudaError_t pe = cudaDeviceSynchronize();
        if (pe != cudaSuccess) { fprintf(stderr, "prep: %s\n", cudaGetErrorString(pe)); return 1; }
    }
    if (p != b.size()) { fprintf(stderr, "spec size mismatch %zu vs %zu\n", p, b.size()); return 1; }
    if (argc % 2) { fprintf(stderr, "ranges: FROM TO pairs\n"); return 2; }
    unsigned long long* dh; cudaMalloc(&dh, 26 * 8); cudaMemset(dh, 0, 26 * 8);
    u32 cap = 1 << 23; u32* dout; cudaMalloc(&dout, (size_t)cap * 12); u32* dn; cudaMalloc(&dn, 4); cudaMemset(dn, 0, 4);
    cudaDeviceSetLimit(cudaLimitStackSize, 4096);
    St* st = 0; u32 *cnt = 0, *fill = 0, *idx = 0, *bg = 0, *bs = 0, *bc = 0;
    for (int ra = 2; ra + 1 < argc; ra += 2) {
    u64 from = strtoull(argv[ra], 0, 10), to = strtoull(argv[ra + 1], 0, 10);
    // output pass threshold: 0 = every seed at the floor; a range whose output would overflow the buffer (easy pairs:
    // millions of seeds at a low floor) is rerun with the lowest threshold that fits (from the first pass's histogram,
    // at most the near-miss level), so the CPU still gets its near-miss candidates and top seeds
    u32 minres = 0;
    unsigned long long h[26]; u32 nout = 0;
    cudaError_t e = cudaSuccess;
    for (int pass = 0; pass < 2; pass++) {
    cudaMemcpyToSymbol(g_minres, &minres, 4);
        cudaMemset(dh, 0, 26 * 8); cudaMemset(dn, 0, 4);
        // short launches with a sync between them: the 3080 also drives the Windows desktop (no long kernels, no TDR)
            if (S.cid == 0 || getenv("ONESHOT_GPU_V1")) {
            const u64 step = 1ull << 18;
            for (u64 s = from; s < to && e == cudaSuccess; s += step) {
                u64 n = (to - s < step) ? to - s : step;
                scan<<<(unsigned)((n + 127) / 128), 128>>>(S, s, n, dh, dout, dn, cap);
                e = cudaDeviceSynchronize();
            }
        } else {
            // v2: colours + pass 1 per seed, then pass 2 bucketed by green: lower greens first, then higher (the CPU's order)
            const u32 step = 1u << 20;
            if (!st) {
                cudaMalloc(&st, (size_t)step * sizeof(St));
                cudaMalloc(&cnt, 256 * 4); cudaMalloc(&fill, 256 * 4); cudaMalloc(&idx, (size_t)step * 4);
                size_t maxb = step / 1 + 256; cudaMalloc(&bg, maxb * 4); cudaMalloc(&bs, maxb * 4); cudaMalloc(&bc, maxb * 4);
            }
            std::vector<u32> hg, hs, hcn;
            for (u64 s0 = from; s0 < to && e == cudaSuccess; s0 += step) {
                u32 n = (u32)((to - s0 < step) ? to - s0 : step);
                for (u32 sub = 0; sub < n; sub += (1u << 18)) {   // kA in short launches
                    u32 m = n - sub < (1u << 18) ? n - sub : (1u << 18);
                    cudaMemset(cnt, 0, 256 * 4);
                    kA<<<(m + 127) / 128, 128>>>(S, s0 + sub, m, st + sub, cnt);
                    e = cudaDeviceSynchronize(); if (e != cudaSuccess) break;
                }
                for (int ph = 0; ph < 2 && e == cudaSuccess; ph++) {
                    cudaMemset(cnt, 0, 256 * 4);
                    kCount<<<(n + 255) / 256, 256>>>(n, st, ph, cnt);
                    u32 hcnt[256]; cudaMemcpy(hcnt, cnt, sizeof hcnt, cudaMemcpyDeviceToHost);
                    u32 off[256], o = 0; for (int g = 0; g < 256; g++) { off[g] = o; o += hcnt[g]; }
                    cudaMemcpy(fill, off, sizeof off, cudaMemcpyHostToDevice);
                    kScatter<<<(n + 255) / 256, 256>>>(n, st, ph, fill, idx);
                    hg.clear(); hs.clear(); hcn.clear();
                    for (int g = 0; g < 256; g++) for (u32 q = 0; q < hcnt[g]; q += TILE) { hg.push_back(g); hs.push_back(off[g] + q); hcn.push_back(hcnt[g] - q < TILE ? hcnt[g] - q : TILE); }
                    u32 nb = (u32)hg.size();
                    if (nb == 0) continue;
                    cudaMemcpy(bg, hg.data(), nb * 4, cudaMemcpyHostToDevice); cudaMemcpy(bs, hs.data(), nb * 4, cudaMemcpyHostToDevice); cudaMemcpy(bc, hcn.data(), nb * 4, cudaMemcpyHostToDevice);
                    for (u32 b0 = 0; b0 < nb && e == cudaSuccess; b0 += 2048) {   // short launches
                        u32 m = nb - b0 < 2048 ? nb - b0 : 2048;
                        kB<<<m, TILE>>>(S, st, idx, bg + b0, bs + b0, bc + b0);
                        e = cudaDeviceSynchronize();
                    }
                }
                if (e == cudaSuccess) { kF<<<(n + 255) / 256, 256>>>(s0, n, st, dh, dout, dn, cap); e = cudaDeviceSynchronize(); }
            }
        }
    if (e != cudaSuccess) break;
    cudaMemcpy(h, dh, sizeof h, cudaMemcpyDeviceToHost);
    cudaMemcpy(&nout, dn, 4, cudaMemcpyDeviceToHost);
    if (nout <= cap) break;
    unsigned long long above = 0; u32 t = 26;
    while (t > 1 && above + h[t - 1] <= cap) { above += h[t - 1]; t--; }
    fprintf(stderr, "output %u > cap: rerun with threshold %u (near %u)\n", nout, t, S.near_);
    minres = t;
    }
    if (e != cudaSuccess) { fprintf(stderr, "cuda: %s\n", cudaGetErrorString(e)); return 1; }
    if (nout > cap) { fprintf(stderr, "output overflow (%u)\n", nout); return 1; }
    std::vector<u32> out((size_t)nout * 3); cudaMemcpy(out.data(), dout, (size_t)nout * 12, cudaMemcpyDeviceToHost);
    printf("range %llu %llu\nhist", from, to); for (int i = 0; i < 26; i++) printf(" %llu", h[i]); printf("\n");
    for (u32 i = 0; i < nout; i++) printf("e %u %u %u\n", out[3 * i], out[3 * i + 1], out[3 * i + 2]);
    fflush(stdout);
    }
    return 0;
}
