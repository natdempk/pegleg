// Peggle oracle: proxy bass.dll that forwards all BASS exports to bass_orig.dll and hooks the
// game's physics to log per-frame ball state. Build: play/build.sh (i686-w64-mingw32-gcc).
// PEGGLE_OUT=DIR: where sound.txt, snd/ and peggle_oracle_info.txt go (default: the game's working directory).
// Log format (little-endian), one record per event:
//   u32 magic ('PRE ' before Ball::Update, 'POST' after CollisionMgr::Collide,
//              'GUN ' at Gun::Fire entry: body = gun bytes +0x100..+0x290, ret field = fire flag)
//              'MOV ' after PhysObj::Update for moving holes/pegs: body = object bytes 0..0x190)
//              'HIT ' at entry of the CCD bounce FUN_004775f0 (one per accepted contact; real balls only):
//                     body u32 obj, u32 objtype, f32 objpos[2], contact[2], normal[2], svel[2], angv[2],
//                     vin[2] (ball vel at iteration start), ballpos[2], ballvel[2], objbbox[4], ballbbox[4]
//              'PCL ' (only with PEGGLE_TRACE_CCD=1) every poly-collide call FUN_0047dfe0 after the last real
//                     Ball::Update: body u32 obj, u32 ret, f32 objpos[2], pos[2], vel[2], toi_in, toi_out,
//                     contact[2], normal[2], objbbox[4]
//   u32 seq, u32 ball_ptr, u32 ret_addr, u16 fpu_cw, u16 mxcsr_lo, u32 info, u8 ball[0x190]
//   info: PRE records = LogicMgr (+0xc delay) & 0xffff | (+0x354 hit-list count) << 16; else GetTickCount
#include <windows.h>
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <stdarg.h>
#include <stdlib.h>
#include <math.h>

#define BALL_SIZE 0x190
static const char *out_dir(void) { const char *o = getenv("PEGGLE_OUT"); return o && *o ? o : "."; }
static FILE *g_log;
static uint32_t g_seq;
static void *g_tramp_update, *g_tramp_collide, *g_tramp_fire, *g_tramp_physupd, *g_tramp_bounce, *g_tramp_pcl;
static int g_trace_ccd, g_last_real;
static volatile int g_frames, g_fired, g_bframes;
static int g_fire_at = -1, g_aim_held, g_mode = 1, g_char = -1, g_autoclick, g_humanfire, g_mouse_shot;  // physics ticks (bucket hole updates); Gun::Fire count

static uint8_t *logic(void);
static void log_blob(uint32_t magic, uint8_t *p, uint32_t extra);
static void dump_pegs(void);
static void dump_board(void);
static void dump_logic(void);
static void check_logic(void);
static void log_rec(uint32_t magic, uint8_t *ball, uint32_t ret) {
    if (!g_log || !ball) return;
    if (magic != 0x204e5547 /*'GUN '*/ && magic != 0x20564f4d /*'MOV '*/ && magic != 0x20544948 /*'HIT '*/
        && magic != 0x204c4350 /*'PCL '*/) {
        if (ball[0x163] == 0) return;              // skip predictor/temp balls (aim guide, zen, lookahead)
        if (*(uint32_t *)(ball + 0x10) != 2) return; // only real balls (internal type 2)
    }
    uint16_t cw; uint32_t mx;
    __asm__ volatile ("fnstcw %0" : "=m"(cw));
    __asm__ volatile ("stmxcsr %0" : "=m"(mx));
    uint32_t info = GetTickCount();
    if (magic == 0x20455250) {
        uint8_t *lm = logic();
        info = lm ? ((*(uint32_t *)(lm + 0xc) & 0xffff) | (*(uint32_t *)(lm + 0x354) << 16)) : 0xffffffff;
    }
    uint32_t hdr[6] = { magic, g_seq++, (uint32_t)(uintptr_t)ball, ret, (uint32_t)cw | ((mx & 0xffff) << 16), info };
    fwrite(hdr, sizeof hdr, 1, g_log);
    fwrite(ball, BALL_SIZE, 1, g_log);
    if ((g_seq & 63) == 0) fflush(g_log);
}

typedef void (__attribute__((fastcall)) *update_fn)(void *ball);
typedef char (__attribute__((thiscall)) *collide_fn)(void *self, void *ball, void *p3);

static void __attribute__((fastcall)) hook_update(void *ball) {
    g_last_real = ((uint8_t *)ball)[0x163] && *(uint32_t *)((uint8_t *)ball + 0x10) == 2;
    if (g_last_real && g_log) check_logic();
    log_rec(0x20455250 /*'PRE '*/, (uint8_t *)ball, (uint32_t)(uintptr_t)__builtin_return_address(0));
    ((update_fn)g_tramp_update)(ball);
}
// With PEGGLE_BHPLOG=1, while the spooky-trial callback is installed (DAT_00650a6c == FUN_00451e10),
// 'PRD ' records before and after each collide: body +0 phase (0 before, 1 after), +4 ball x, y,
// +12 vel x, y, +20 Ball+0x141, +24 count (DAT_00650914).
static int g_prdlog = -1;
static void prd_rec(int ph, uint8_t *b) {
    uint8_t buf[BALL_SIZE]; memset(buf, 0, sizeof buf); uint32_t *u = (uint32_t *)buf;
    u[0] = ph; memcpy(buf + 4, b + 0xec, 8); memcpy(buf + 12, b + 0xfc, 8); u[5] = b[0x141]; u[6] = *(volatile uint32_t *)0x650914;
    log_blob(0x20445250 /*'PRD '*/, buf, 0);
}
// Ball collide FUN_00454f90 returns "hit something" (AL). Only the gun's aim guide FUN_00482910 reads it
// (0x482b6d): the step of the guide's first hit goes to Gun+0x198 (with the point at +0x1a4/+0x1a8), which
// the beginner aim assist's ball filter reads (FUN_004604c0, AIMgr mode 2: stage 1-1, last 6 balls). The
// playback has always reported a hit here (Gun+0x198 = 0), so the assist keeps the player's aim; the
// simulator models that (`Game::assist_balls`). Returning a constant keeps it independent of the compiler.
static char __attribute__((thiscall)) hook_collide(void *self, void *ball, void *p3) {
    if (g_prdlog < 0) g_prdlog = getenv("PEGGLE_BHPLOG") != 0;
    int prd = g_prdlog && ball && *(volatile uint32_t *)0x650a6c == 0x451e10;
    if (prd) prd_rec(0, (uint8_t *)ball);
    ((collide_fn)g_tramp_collide)(self, ball, p3);
    if (prd) prd_rec(1, (uint8_t *)ball);
    log_rec(0x54534f50 /*'POST'*/, (uint8_t *)ball, (uint32_t)(uintptr_t)__builtin_return_address(0));
    return 1;
}

// Gun::Fire FUN_00484530 (thiscall, 2 args): log gun object bytes 0x100..0x290 ('GUN ' record).
typedef void *(__attribute__((thiscall)) *fire_fn)(void *self, void *out, int flag);
static void *__attribute__((thiscall)) hook_fire(void *self, void *out, int flag) {
    dump_logic();
    dump_pegs();
    dump_board();
    log_rec(0x204e5547, (uint8_t *)self + 0x100, (uint32_t)flag);
    g_fired++;
    void *r = ((fire_fn)g_tramp_fire)(self, out, flag);
    if (g_log) fflush(g_log);
    return r;
}

// PhysObj::Update FUN_00479a30 (fastcall obj): after it runs, log moving holes (type 1) / pegs (type 3)
// that have a mover (+0x48) and the moving flag (+0x26). Gives the per-object frame counter +0x4c.
static void __attribute__((fastcall)) hook_physupd(void *obj) {
    ((update_fn)g_tramp_physupd)(obj);
    uint8_t *o = (uint8_t *)obj;
    uint32_t t = *(uint32_t *)(o + 0x10);
    if (t == 1 && *(uint32_t *)(o + 0x48) && o[0x26]) g_frames++;
    static int movall = -1;
    if (movall < 0) movall = getenv("PEGGLE_MOVALL") != 0;   // PEGGLE_MOVALL=1: every moving object (e.g. flippers)
    if ((t == 1 || t == 3 || movall) && *(uint32_t *)(o + 0x48) && o[0x26])
        log_rec(0x20564f4d /*'MOV '*/, o, (uint32_t)(uintptr_t)__builtin_return_address(0));
}

// CCD bounce FUN_004775f0 (thiscall ball, 6 args, RET 0x18): log each accepted contact.
typedef void (__attribute__((thiscall)) *bounce_fn)(void *ball, float *vin, uint8_t *obj, float *svel,
                                                    float *contact, float *normal, float *angv);
static void __attribute__((thiscall)) hook_bounce(void *ball, float *vin, uint8_t *obj, float *svel,
                                                  float *contact, float *normal, float *angv) {
    uint8_t *b = (uint8_t *)ball;
    if (g_log && b[0x163] && *(uint32_t *)(b + 0x10) == 2) {
        uint8_t buf[BALL_SIZE]; memset(buf, 0, sizeof buf);
        uint32_t *u = (uint32_t *)buf; float *f = (float *)buf;
        uint32_t t = obj ? *(uint32_t *)(obj + 0x10) : 0;
        u[0] = (uint32_t)(uintptr_t)obj; u[1] = t;
        if (obj) {
            int ox = t == 5 ? 0x114 : 0xec, oy = t == 5 ? 0x118 : (t == 4 ? 0xf4 : 0xf0);
            f[2] = *(float *)(obj + ox); f[3] = *(float *)(obj + oy);
        }
        f[4] = contact[0]; f[5] = contact[1]; f[6] = normal[0]; f[7] = normal[1];
        f[8] = svel[0]; f[9] = svel[1]; f[10] = angv[0]; f[11] = angv[1];
        f[12] = vin[0]; f[13] = vin[1];
        f[14] = *(float *)(b + 0xec); f[15] = *(float *)(b + 0xf0);
        f[16] = *(float *)(b + 0xfc); f[17] = *(float *)(b + 0x100);
        if (obj) memcpy(f + 18, obj + 0x14, 16); // obj bbox
        memcpy(f + 22, b + 0x14, 16);            // ball bbox
        log_rec(0x20544948 /*'HIT '*/, buf, 0);
    }
    ((bounce_fn)g_tramp_bounce)(ball, vin, obj, svel, contact, normal, angv);
}

// Poly collide FUN_0047dfe0 (thiscall poly, 8 args, RET 0x20).
typedef char (__attribute__((thiscall)) *pcl_fn)(uint8_t *poly, float *pos, float *vel, float r, float *toi,
                                                 float *contact, float *normal, float *svel, float *angv);
static char __attribute__((thiscall)) hook_pcl(uint8_t *poly, float *pos, float *vel, float r, float *toi,
                                               float *contact, float *normal, float *svel, float *angv) {
    float toi_in = *toi;
    char ret = ((pcl_fn)g_tramp_pcl)(poly, pos, vel, r, toi, contact, normal, svel, angv);
    if (g_log && g_trace_ccd && g_last_real) {
        uint8_t buf[BALL_SIZE]; memset(buf, 0, sizeof buf);
        uint32_t *u = (uint32_t *)buf; float *f = (float *)buf;
        u[0] = (uint32_t)(uintptr_t)poly; u[1] = (uint8_t)ret;
        f[2] = *(float *)(poly + 0x114); f[3] = *(float *)(poly + 0x118);
        f[4] = pos[0]; f[5] = pos[1]; f[6] = vel[0]; f[7] = vel[1]; f[8] = toi_in; f[9] = *toi;
        f[10] = contact[0]; f[11] = contact[1]; f[12] = normal[0]; f[13] = normal[1];
        memcpy(f + 14, poly + 0x14, 16); // obj bbox
        log_rec(0x204c4350 /*'PCL '*/, buf, 0);
    }
    return ret;
}

// Patch `target` with JMP to `hook`; return trampoline executing the `n` stolen bytes then jumping back.
static void *install(uint32_t target, void *hook, int n) {
    uint8_t *t = (uint8_t *)target;
    uint8_t *tr = VirtualAlloc(NULL, 32, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE);
    memcpy(tr, t, n);
    tr[n] = 0xE9; *(int32_t *)(tr + n + 1) = (int32_t)((t + n) - (tr + n + 5));
    DWORD old; VirtualProtect(t, n, PAGE_EXECUTE_READWRITE, &old);
    t[0] = 0xE9; *(int32_t *)(t + 1) = (int32_t)((uint8_t *)hook - (t + 5));
    for (int i = 5; i < n; i++) t[i] = 0x90;
    VirtualProtect(t, n, old, &old);
    FlushInstructionCache(GetCurrentProcess(), t, n);
    return tr;
}


// ---------------------------------------------------------------------------------------------
// In-process harness (PEGGLE_SCRIPT). Runs on the main thread from a PeekMessageA IAT hook, between
// frames. Script lines:
//   level S L      start adventure stage S (0..10) level L (0..4): app+0x760=1,+0x764=S,+0x768=L,
//                  then StartLevel (board exists) or ShowBoard(app,0,1)
//   shot A         aim at angle A (radians, float) and fire; waits until the shot is over
//   shotx HEX      same, exact f32 bits
//   cursor X Y     (videos) window pixel of the next shot: the OS cursor glides there before the click
//                  and the launcher follows it, landing exactly on the shot's angle (visual only: mouse
//                  messages are kept from the game while a script runs)
//   wait N         wait N game ticks
//   quit           flush logs and exit
// Clock scaling: PEGGLE_SPEED=K multiplies timeGetTime/GetTickCount/QPC and divides Sleep.
// Game structures: app = *(0x687394); loaded flag app+0x4fa; board = app+0x7b8; logic = board+0x154
// (state +4: 1 = aiming; tick +8; aim target +0xe0 (float); fire request +0xed); gun = logic+0x104 → +0xbc.
static DWORD g_main_tid;
static FILE *g_script, *g_hlog;
static int g_driving;          // PEGGLE_SCRIPT given: real mouse input is ignored
static double g_speed = 1.0;
static DWORD g_wait_ms;
static int g_hstate;          // 0 idle/next cmd, 1 wait level ready, 2 aiming, 3 in shot, 4 wait ticks
static int g_wait_until, g_aim_since, g_shot_start, g_shots;
static float g_aim;
static int g_fire_seen, g_level_over;
static void *shown_dialog(void);
static int g_cmd_tick;
// --- video cursor: smooth move to the next shot's window pixel ---
static int g_cur_on;                         // a `cursor` command was seen (video scripts)
static float g_cur_x = 400, g_cur_y = 300;   // current window pixel
static float g_cur_x0, g_cur_y0, g_cur_x1, g_cur_y1, g_cur_off0, g_cur_off1, g_cur_bend;
static int g_cur_t0 = -1, g_cur_t1 = -1;     // move ticks [t0, t1]
static float g_prev_aim = -1;
static POINT g_cur_org; static int g_cur_org_ok;

#define APP() (*(uint8_t **)0x687394)
static uint8_t *board(void) { uint8_t *a = APP(); return a ? *(uint8_t **)(a + 0x7b8) : 0; }
static uint8_t *logic(void) { uint8_t *b = board(); return b ? *(uint8_t **)(b + 0x154) : 0; }
static uint8_t *gun(void) {
    uint8_t *l = logic(); if (!l) return 0;
    uint8_t *pb = *(uint8_t **)(l + 0x104); return pb ? *(uint8_t **)(pb + 0xbc) : 0;
}
static int tick(void) { return g_frames; }

static void hlog(const char *fmt, ...) {
    if (!g_hlog) return;
    va_list ap; va_start(ap, fmt);
    fprintf(g_hlog, "[%lu] ", (unsigned long)GetTickCount());
    vfprintf(g_hlog, fmt, ap); fputc('\n', g_hlog); fflush(g_hlog);
    va_end(ap);
}
static void log_blob(uint32_t magic, uint8_t *p, uint32_t extra) {
    if (!g_log || !p) return;
    uint32_t hdr[6] = { magic, g_seq++, (uint32_t)(uintptr_t)p, extra, 0, GetTickCount() };
    fwrite(hdr, sizeof hdr, 1, g_log); fwrite(p, BALL_SIZE, 1, g_log); fflush(g_log);
}

// 'LGC ' x3: LogicMgr bytes [0,0x190), [0x190,0x320), [0x320,0x4b0) (header word 3 = slice offset).
// Logged at fire and whenever the shot score (+0x108) or shot peg count (+0x12c) changes (checked per
// real-ball update).
static void dump_logic(void) {
    uint8_t *l = logic(); if (!l || !g_log) return;
    for (uint32_t off = 0; off < 0x4b0; off += BALL_SIZE) log_blob(0x2043474c /*'LGC '*/, l + off, off);
}
static int32_t g_last_score = -1, g_last_npeg = -1;
static void check_logic(void) {
    uint8_t *l = logic(); if (!l) return;
    int32_t sc = *(int32_t *)(l + 0x108), np = *(int32_t *)(l + 0x12c);
    if (sc != g_last_score || np != g_last_npeg) { g_last_score = sc; g_last_npeg = np; dump_logic(); }
}
// 'PEG ': every board object with a PegInfo (+0xd0), 25 per record of 16 bytes:
// f32 x (+0xec or poly +0x114), f32 y, u32 PegInfo+0x10 type, u8 +0x14 hit, +0x15 variable, +0x24
// collidable(obj), +0x25 visible(obj). Header word 3 = index of the first entry.
// 'BRD ': header word 3 = board tick (board+0x1c4); body = up to 50 x (u32 board-list index, i32 mover+0x18
// amplitude, i32 mover+0x1c period) for objects whose id (std::string at obj+0x5c) starts with "freeball"
// or is "bumperhole" (the movers FUN_00440830 animates toward LogicMgr+0x134).
static const char *obj_id(uint8_t *o, unsigned *len) {
    *len = *(unsigned *)(o + 0x70); unsigned cap = *(unsigned *)(o + 0x74);
    return cap < 16 ? (const char *)(o + 0x60) : *(const char **)(o + 0x60);
}
static void dump_board(void) {
    uint8_t *l = logic(); if (!l || !g_log) return;
    uint8_t *b = *(uint8_t **)(l + 0x104); if (!b) return;
    uint8_t **head = *(uint8_t ***)(b + 0x194);
    uint8_t buf[BALL_SIZE]; memset(buf, 0, sizeof buf); int n = 0, idx = 0;
    for (uint8_t **nd = (uint8_t **)head[0]; nd != (uint8_t **)head && n < 33; nd = (uint8_t **)nd[0], idx++) {
        uint8_t *o = nd[2]; if (!o) continue;
        uint8_t *mv = *(uint8_t **)(o + 0x48);
        unsigned len; const char *id = obj_id(o, &len);
        if (len > 64 || !((len >= 8 && memcmp(id, "freeball", 8) == 0) || (len == 10 && memcmp(id, "bumperhole", 10) == 0))) continue;
        uint32_t *e = (uint32_t *)(buf + 12 * n);
        // word 0: board index | collidable (+0x24) << 24 | visible (+0x25) << 25 | has mover << 26 | 1 << 27 (v2 marker)
        e[0] = idx | (uint32_t)(o[0x24] != 0) << 24 | (uint32_t)(o[0x25] != 0) << 25 | (uint32_t)(mv != 0) << 26 | 1u << 27;
        e[1] = mv ? *(uint32_t *)(mv + 0x18) : 0; e[2] = mv ? *(uint32_t *)(mv + 0x1c) : 0; n++;
    }
    log_blob(0x20445242 /*'BRD '*/, buf, *(uint32_t *)(b + 0x1c4));
}
static void dump_pegs(void) {
    uint8_t *l = logic(); if (!l || !g_log) return;
    uint8_t *b = *(uint8_t **)(l + 0x104); if (!b) return;
    uint8_t **head = *(uint8_t ***)(b + 0x194);
    uint8_t buf[BALL_SIZE]; int n = 0, first = 0, total = 0, idx = -1;
    memset(buf, 0, sizeof buf);
    for (uint8_t **nd = (uint8_t **)head[0]; nd != (uint8_t **)head; nd = (uint8_t **)nd[0]) {
        idx++;
        uint8_t *o = nd[2]; if (!o) continue;
        uint8_t *pi = *(uint8_t **)(o + 0xd0); if (!pi) continue;
        uint32_t t = *(uint32_t *)(o + 0x10);
        float *e = (float *)(buf + 16 * n);
        e[0] = *(float *)(o + (t == 5 ? 0x114 : 0xec)); e[1] = *(float *)(o + (t == 5 ? 0x118 : 0xf0));
        // type word: PegInfo+0x10 (low 16 bits) | (board-list index + 1) << 16
        *(uint32_t *)(buf + 16 * n + 8) = (*(uint32_t *)(pi + 0x10) & 0xffff) | ((uint32_t)(idx + 1) << 16);
        buf[16 * n + 12] = pi[0x14]; buf[16 * n + 13] = pi[0x15]; buf[16 * n + 14] = o[0x24]; buf[16 * n + 15] = o[0x25];
        n++; total++;
        if (n == 25) { log_blob(0x20474550 /*'PEG '*/, buf, first); first = total; n = 0; memset(buf, 0, sizeof buf); }
    }
    if (n) log_blob(0x20474550, buf, first);
}

typedef void (__attribute__((thiscall)) *showboard_fn)(void *app, int p2, char start);
typedef void (__attribute__((fastcall)) *startlevel_fn)(void *board);

// Live ThunderDialogs (ctor 0x497280 / dtor 0x497550). Harness presses OK (1000) on the newest one
// via ThunderDialog::ButtonDepress FUN_00540e10 (this = dialog+0x88, ButtonListener subobject).
static void *g_dialogs[32]; static int g_ndialogs, g_dialog_age;
static void *g_tramp_dlg_ctor, *g_tramp_dlg_dtor;
// Style-shot award LogicMgr FUN_0046b100(x, y, style, ball) (ret 0x10): 'STY ' record body
// +0 style id, +4 x, +8 y, +12 LogicMgr tick (+8), +16 board+0x1a4, +20 board+0x1b0, +24 board+0x1b4;
// extra = board tick.
static void *g_tramp_style;
// AI search step FUN_00460380 (fastcall AIMgr): with PEGGLE_AILOG=1, after each step log 'AIP ' records
// (25 balls each: pos x,y, vel x,y at +0xec/+0xf0/+0xfc/+0x100; extra = step | first index << 16) and,
// at steps 1 and AIMgr+0x50, full 'AIB0' (ball+0) / 'AIB1' (ball+0x190) dumps (extra = step | index << 16).
static void *g_tramp_aistep;
static void __attribute__((fastcall)) hook_aistep(uint8_t *ai) {
    ((update_fn)g_tramp_aistep)(ai);
    uint8_t **head = *(uint8_t ***)(ai + 0x34);
    uint32_t step = *(uint32_t *)(ai + 0x58), k = 0, n = 0, first = 0;
    int full = step == 1 || step == *(uint32_t *)(ai + 0x50);
    uint8_t buf[BALL_SIZE]; memset(buf, 0, sizeof buf);
    for (uint8_t **nd = (uint8_t **)head[0]; nd != (uint8_t **)head && k < 5000; nd = (uint8_t **)nd[0], k++) {
        uint8_t *b = nd[2]; if (!b) continue;
        if (full) {
            log_blob(0x30424941 /*'AIB0'*/, b, step | (k << 16));
            log_blob(0x31424941 /*'AIB1'*/, b + 0x190, step | (k << 16));
        }
        memcpy(buf + 16 * n, b + 0xec, 8); memcpy(buf + 16 * n + 8, b + 0xfc, 8);
        if (++n == 25) { log_blob(0x20504941 /*'AIP '*/, buf, step | (first << 16)); first = k + 1; n = 0; memset(buf, 0, sizeof buf); }
    }
    if (n) log_blob(0x20504941, buf, step | (first << 16));
}
typedef void (__attribute__((thiscall)) *style_fn)(void *, float, float, uint32_t, void *);
static void __attribute__((thiscall)) hook_style(void *self, float x, float y, uint32_t style, void *ball) {
    uint8_t buf[BALL_SIZE]; memset(buf, 0, sizeof buf);
    *(uint32_t *)buf = style; *(float *)(buf + 4) = x; *(float *)(buf + 8) = y;
    *(uint32_t *)(buf + 12) = *(uint32_t *)((uint8_t *)self + 8);
    uint8_t *l = logic(); uint8_t *b = l ? *(uint8_t **)(l + 0x104) : 0;
    if (b) { *(uint32_t *)(buf + 16) = *(uint32_t *)(b + 0x1a4); *(uint32_t *)(buf + 20) = *(uint32_t *)(b + 0x1b0);
             *(uint32_t *)(buf + 24) = *(uint32_t *)(b + 0x1b4); }
    log_blob(0x20595453 /*'STY '*/, buf, b ? *(uint32_t *)(b + 0x1c4) : 0);
    ((style_fn)g_tramp_style)(self, x, y, style, ball);
}
// Long Shot check LogicMgr FUN_0046b910(ball, peg) (thiscall, ret 8): after it, 'LSH ' record body
// +0 entry count of the +0xb8 contact log, +4.. up to 20 entries (tick, type, x, y as in the game),
// +0x150 ball x, y, +0x158 peg type; extra = LogicMgr tick (+8).
static void *g_tramp_lsh;
typedef void (__attribute__((thiscall)) *lsh_fn)(void *, void *, void *);
static void __attribute__((thiscall)) hook_lsh(void *self, void *ball, void *peg) {
    ((lsh_fn)g_tramp_lsh)(self, ball, peg);
    uint8_t *l = (uint8_t *)self, buf[BALL_SIZE]; memset(buf, 0, sizeof buf);
    uint8_t *b0 = *(uint8_t **)(l + 0xb8), *b1 = *(uint8_t **)(l + 0xbc);
    uint32_t n = b0 ? (uint32_t)(b1 - b0) / 16 : 0;
    *(uint32_t *)buf = n;
    if (b0) memcpy(buf + 4, b0, (n < 20 ? n : 20) * 16);
    if (ball) memcpy(buf + 0x150, (uint8_t *)ball + 0xec, 8);
    uint8_t *pi = peg ? *(uint8_t **)((uint8_t *)peg + 0xd0) : 0;
    if (pi) *(uint32_t *)(buf + 0x158) = *(uint32_t *)(pi + 0x10);
    log_blob(0x2048534c /*'LSH '*/, buf, *(uint32_t *)(l + 8));
}
// BallHitPeg LogicMgr FUN_0046f480(ball, peg, char direct) (thiscall, ret 0xc): at entry, 'BHP ' record
// body +0 ball, +4 peg, +8 direct (4th argument), +12 Ball+0xe8, +16 PegInfo+0x14 (already hit), +20 ball x, y,
// +28 peg type, +32 Ball+0x188; extra = LogicMgr tick (+8).
static void *g_tramp_bhp;
typedef void (__attribute__((thiscall)) *bhp_fn)(void *, void *, void *, int);
static void __attribute__((thiscall)) hook_bhp(void *self, void *ball, void *peg, int direct) {
    uint8_t buf[BALL_SIZE]; memset(buf, 0, sizeof buf); uint32_t *u = (uint32_t *)buf;
    u[0] = (uint32_t)(uintptr_t)ball; u[1] = (uint32_t)(uintptr_t)peg; u[2] = direct & 0xff;
    uint8_t *b = (uint8_t *)ball, *pi = peg ? *(uint8_t **)((uint8_t *)peg + 0xd0) : 0;
    if (b) { u[3] = *(uint32_t *)(b + 0xe8); memcpy(buf + 20, b + 0xec, 8); u[8] = *(uint32_t *)(b + 0x188); }
    if (pi) { u[4] = pi[0x14]; u[7] = *(uint32_t *)(pi + 0x10); }
    log_blob(0x20504842 /*'BHP '*/, buf, *(uint32_t *)((uint8_t *)self + 8));
    ((bhp_fn)g_tramp_bhp)(self, ball, peg, direct);
}
// Spooky trial / predictor hit callback FUN_00451e10(ball, obj) (cdecl): with PEGGLE_BHPLOG=1, 'SPK '
// record body +0 obj, +4 count before (DAT_00650914), +8 count after, +12 ball x, y, +20 vel x, y,
// +28 obj type (+0x10), +32 PegInfo hit flag (+0x14); extra = LogicMgr tick.
static void *g_tramp_spk;
typedef void (*spk_fn)(void *, void *);
static void hook_spk(void *ball, void *obj) {
    uint32_t before = *(volatile uint32_t *)0x650914;
    ((spk_fn)g_tramp_spk)(ball, obj);
    uint8_t buf[BALL_SIZE]; memset(buf, 0, sizeof buf); uint32_t *u = (uint32_t *)buf;
    u[0] = (uint32_t)(uintptr_t)obj; u[1] = before; u[2] = *(volatile uint32_t *)0x650914;
    if (ball) { memcpy(buf + 12, (uint8_t *)ball + 0xec, 8); memcpy(buf + 20, (uint8_t *)ball + 0xfc, 8); }
    if (obj) { u[7] = *(uint32_t *)((uint8_t *)obj + 0x10); uint8_t *pi = *(uint8_t **)((uint8_t *)obj + 0xd0); if (pi) u[8] = pi[0x14] | 0x100; }
    uint8_t *l = logic();
    log_blob(0x204b5053 /*'SPK '*/, buf, l ? *(uint32_t *)(l + 8) : 0);
}
// Ball tube update FUN_00443cf0 (fastcall, InterfaceMgr): with PEGGLE_TUBELOG=1, a 'TUB ' record after
// each call while the tube is moving: +0 +0x58 (removal timer), +4 +0x5c (add offset), +8 +0x60 (add delay),
// +12 +0xe8, +16 +0xec, +20 +0x160 (settled), +24 LogicMgr+0x138, +28 LogicMgr state, +32 ball count,
// +36 spacing FUN_00436720(), +40 floor FUN_004366a0(), +44 harness tick, +48.. (pos, vel) floats of up to 25 balls;
// extra = LogicMgr tick. The settled call after a moving one is logged too.
static void *g_tramp_tube; static int g_tubelog, g_tube_moving;
static void __attribute__((fastcall)) hook_tube(uint8_t *im) {
    ((update_fn)g_tramp_tube)(im);
    if (!g_tubelog || !im) return;
    int moving = !im[0x160] || *(int *)(im + 0x58) || *(int *)(im + 0x60);
    if (!moving && !g_tube_moving) return;
    g_tube_moving = moving;
    uint8_t buf[BALL_SIZE]; memset(buf, 0, sizeof buf); uint32_t *u = (uint32_t *)buf;
    u[0] = *(uint32_t *)(im + 0x58); u[1] = *(uint32_t *)(im + 0x5c); u[2] = *(uint32_t *)(im + 0x60);
    u[3] = *(uint32_t *)(im + 0xe8); u[4] = *(uint32_t *)(im + 0xec); u[5] = im[0x160];
    uint8_t *l = logic();
    if (l) { u[6] = *(uint32_t *)(l + 0x138); u[7] = *(uint32_t *)(l + 4); }
    uint8_t *v0 = *(uint8_t **)(im + 0x168), *v1 = *(uint8_t **)(im + 0x16c);
    int n = v0 ? (int)((v1 - v0) >> 3) : 0; u[8] = n;
    u[9] = ((int (*)(void))0x436720)();
    u[10] = ((int (__attribute__((fastcall)) *)(void *))0x4366a0)(im);
    u[11] = g_frames;
    if (n > 25) n = 25;
    if (n > 0) memcpy(buf + 48, v0, n * 8);
    log_blob(0x20425554 /*'TUB '*/, buf, l ? *(uint32_t *)(l + 8) : 0);
}
typedef void *(__attribute__((thiscall)) *dlgctor_fn)(void *, int, int, int, int, int, int);
static void *__attribute__((thiscall)) hook_dlg_ctor(void *self, int a, int b, int c, int d, int e, int f) {
    void *r = ((dlgctor_fn)g_tramp_dlg_ctor)(self, a, b, c, d, e, f);
    if (g_ndialogs < 32) g_dialogs[g_ndialogs++] = self;
    hlog("dialog frm %d tick %d vt %p", g_bframes, g_frames, *(void **)self);
    g_dialog_age = 0;
    return r;
}
static void __attribute__((fastcall)) hook_dlg_dtor(void *self) {
    for (int i = 0; i < g_ndialogs; i++) if (g_dialogs[i] == self) { g_dialogs[i] = g_dialogs[--g_ndialogs]; break; }
    ((update_fn)g_tramp_dlg_dtor)(self);
}
typedef void (__attribute__((thiscall)) *btn_fn)(void *, int);
static void *shown_dialog(void) {
    void *d = 0;
    uint8_t *app = APP();
    typedef void *(__attribute__((thiscall)) *getdlg_fn)(void *, int);
    getdlg_fn getdlg = (getdlg_fn)(*(void ***)app)[0x124 / 4];   // app->GetDialog(id), map at app+0x324
    for (int i = g_ndialogs - 1; i >= 0; i--) {               // only dialogs actually shown (registered)
        int id = *(int *)((uint8_t *)g_dialogs[i] + 0x88 + 0xb4);
        if (getdlg(app, id) == g_dialogs[i]) { d = g_dialogs[i]; break; }
    }
    return d;
}
// force: dismiss now (the per-frame click path, PEGGLE_CLICKS=1: a dialog is closed at the first frame start it is
// visible, as a player's click between frames would; counting message-loop calls instead made the frame depend on
// wall-clock timing)
static void dismiss_dialogs_f(int force) {
    void *d = shown_dialog();
    if (d && *(unsigned *)d == 0x601334) d = 0;   // never auto-dismiss EndLevelDialog
    if (!d) { g_dialog_age = 0; return; }
    if (++g_dialog_age < (g_autoclick ? 3 : 100) && !force) return;      // let it finish appearing
    if (*(unsigned *)d == 0x60208c && g_char >= 0) {
        // CharacterDialog: player 0's buttons = vector at +0x228 (begin +4, end +8) of 12-byte entries,
        // selected flag at +4 (FUN_004a0f90 applies the selected index on close)
        uint8_t *b0 = *(uint8_t **)((uint8_t *)d + 0x22c), *e0 = *(uint8_t **)((uint8_t *)d + 0x230);
        int n = b0 ? (int)((e0 - b0) / 12) : 0;
        for (int k = 0; k < n; k++) b0[12 * k + 4] = (k == g_char);
        hlog("character dialog: %d buttons, selected %d", n, g_char);
    }
    hlog("dismiss dialog %p vt %08x id %d", d, *(unsigned *)d, *(int *)((uint8_t *)d + 0x88 + 0xb4));
    uint8_t *bl = (uint8_t *)d + 0x88;     // ButtonListener subobject; slot 2 = ButtonDepress
    ((btn_fn)(*(void ***)bl)[2])(bl, 1000);
    g_dialog_age = 0;
}

static void dismiss_dialogs(void) { dismiss_dialogs_f(0); }

// LogicMgr::MouseDown FUN_00472810(this, x, y, clickCount, p5, p6): the real click path (closes tips,
// sets fire request +0xed when aiming). Position only stored at +0x98/+0x9c.
typedef int (__attribute__((thiscall)) *mousedown_fn)(void *, int, int, int, char, char);
// LogicMgr::MouseUp FUN_0045ebb0(this, x, y, clickCount, p5): releases the flippers (FUN_0045ea70(0)) when the
// press went through MouseDown's in-flight path (LogicMgr+0x1e)
typedef int (__attribute__((thiscall)) *mouseup_fn)(void *, int, int, int, char);
// `flip TICK 1|0` script lines (before a shotx): flipper presses / releases for that shot, made at frame starts
// during the flight like a player's (MouseDown flips while LogicMgr+0x1e4[player] >= 1, FUN_0045ea70)
#define MAXFLIP 4096   // flip lines per shot (long flipper juggles: oneshot class flip plans use 100+)
static int g_flip_t[MAXFLIP], g_flip_d[MAXFLIP], g_nflip_pend, g_nflip, g_flip_i;
static int g_flip_pt[MAXFLIP], g_flip_pd[MAXFLIP];
static int g_last_click;
static void click(uint8_t *l) {
    if (tick() == g_last_click) return;
    g_last_click = tick();
    ((mousedown_fn)0x472810)(l, 400, 300, 1, 0, 0);
}

// ---- TAS determinism ----
// PEGGLE_RNG_SEED=S: the level-start / turn-end colour routines (FUN_00457960 oranges, FUN_00458110
// purple, FUN_00457dc0 greens, FUN_004582f0 power re-roll) are called through patched call sites that
// first reseed the global MT (0x686840) with mix(S, n), n = call index since the last `level` command.
// Peg colours then depend only on (S, level, shot sequence), not on wall-clock/effects RNG use.
// PEGGLE_PROGRESS=N: profile furthest level = N (stage*5+level) at each `level` command; greens need
// N>1 and purple N>2 (FUN_00457dc0 / FUN_00458110 gate on it). A fresh profile has neither.
// `shot ANGLE TICK`: fire exactly at bucket tick TICK (fire happens inside the aiming update
// FUN_00466f10, the same place the game's own queued inputs are replayed), gun angle set directly.
typedef void (__attribute__((thiscall)) *seed_fn)(void *mt, uint32_t seed);
static uint32_t g_rng_seed; static int g_rng_on, g_rng_ctr, g_progress = -1;
static int g_rdy_done, g_interactive;   // -1 untouched, -2 = stage's master (PEGGLE_CHAR=stage), else index
static void reseed(void) {
    uint32_t s = g_rng_seed * 0x9E3779B1u ^ ((uint32_t)g_rng_ctr * 0x85EBCA6Bu + 0x68E31DA4u);
    s ^= s >> 15; s *= 0x2C1B3C6Du; s ^= s >> 12; if (!s) s = 1;
    ((seed_fn)0x4fb640)((void *)0x686840, s);
    hlog("reseed %d -> %u", g_rng_ctr, s); g_rng_ctr++;
}
// Keyed reseed for in-shot random effects the sim reproduces: key = (site, board tick Board+0x1c4).
static uint32_t tas_mix(uint32_t a, uint32_t b) {
    uint32_t s = a * 0x9E3779B1u ^ (b * 0x85EBCA6Bu + 0x68E31DA4u);
    s ^= s >> 15; s *= 0x2C1B3C6Du; s ^= s >> 12; return s ? s : 1;
}
static void reseed_key(uint32_t site) {
    uint8_t *b = board(); uint32_t bt = b ? *(uint32_t *)(b + 0x1c4) : 0;
    uint32_t s = tas_mix(g_rng_seed ^ site, bt);
    ((seed_fn)0x4fb640)((void *)0x686840, s);
    hlog("reseed site %x btick %u -> %u", site, bt, s);
}
typedef void (__attribute__((thiscall)) *spooky_fn)(void *, void *);
static void __attribute__((thiscall)) w_45eca0(void *lm, void *ball) { reseed_key(0x5B00); ((spooky_fn)0x45eca0)(lm, ball); }
typedef uint32_t (*rand_fn)(void);
// Lucky Spin wheel (FUN_0046ecb0): result = Rand() & 3
static uint32_t w_rand_lucky(void) { reseed_key(0x5B01); return ((rand_fn)0x4fce50)(); }
// Fire (FUN_0046ac70): LogicMgr+0x2c = Rand(), +0x30 = Rand() — Lucky Spin's random-power index (FUN_00451ac0)
static uint32_t w_rand_shot(void) { reseed_key(0x5B03); return ((rand_fn)0x4fce50)(); }
// Level load (FUN_0040f400): character 10 ("random", e.g. adventure stage 10 → app+0x764 = 10) is Rand() % n,
// rerolled while equal to the previous pick. Keyed by (seed, stage, level) so stage-10 powers are reproducible.
static uint32_t w_rand_char(void) {
    uint8_t *a = APP(); uint32_t k = a ? (uint32_t)(*(int *)(a + 0x764) * 5 + *(int *)(a + 0x768)) : 0;
    uint32_t sd = tas_mix(g_rng_seed ^ 0x5B04, k);
    ((seed_fn)0x4fb640)((void *)0x686840, sd);
    hlog("reseed site 5b04 level key %u -> %u", k, sd);
    return ((rand_fn)0x4fce50)();
}
// Lucky Spin slot machine dialog open (FUN_004023f0 → FUN_0049cd40: Rand()%360 start angle, Rand()%100 spin)
typedef void (__attribute__((thiscall)) *slot_fn)(void *, void *, void *);
static void __attribute__((thiscall)) w_4023f0(void *bd, void *ball, void *peg) { reseed_key(0x5B01); ((slot_fn)0x4023f0)(bd, ball, peg); }
// Zen shot search (FUN_00467b80(LogicMgr, 0) → AI search FUN_004604c0, uses Rand): make it a function of (seed, tick)
typedef int (__attribute__((thiscall)) *zen_fn)(void *, int);
static int __attribute__((thiscall)) w_467b80(void *lm, int p) { reseed_key(0x5B02); return ((zen_fn)0x467b80)(lm, p); }
#define RESEED_WRAP(name, fn) static void __attribute__((fastcall)) name(void *t) { reseed(); ((update_fn)(fn))(t); }
RESEED_WRAP(w_457960, 0x457960) RESEED_WRAP(w_458110, 0x458110) RESEED_WRAP(w_457dc0, 0x457dc0) RESEED_WRAP(w_4582f0, 0x4582f0)
static void patch_call(uint32_t site, uint32_t expect, void *fn) {
    uint8_t *p = (uint8_t *)site;
    if (p[0] != 0xE8 || site + 5 + *(int32_t *)(p + 1) != expect) return;
    DWORD o; VirtualProtect(p, 5, PAGE_EXECUTE_READWRITE, &o);
    *(int32_t *)(p + 1) = (int32_t)((uint32_t)(uintptr_t)fn - (site + 5));
    VirtualProtect(p, 5, o, &o);
}
// Green/purple candidates are collected by iterating LogicMgr's pointer-keyed std::set (+0x364), so
// their order depends on heap addresses. Before the green shuffle (call FUN_0043d430 at 0x457fb2,
// vector in ESI) and the purple pick (Rand at 0x45827c, vector begin/end at caller EBP-0x30/-0x2c) we
// sort the candidate vector by board-list index (= level load order, same order as 'PEG ' records).
static int board_index(uint8_t *obj) {
    uint8_t *l = logic(); if (!l) return 0;
    uint8_t *b = *(uint8_t **)(l + 0x104); if (!b) return 0;
    uint8_t **head = *(uint8_t ***)(b + 0x194); int i = 0;
    for (uint8_t **nd = (uint8_t **)head[0]; nd != (uint8_t **)head; nd = (uint8_t **)nd[0], i++)
        if (nd[2] == obj) return i;
    return 1 << 20;
}
static void sort_objvec(uint8_t **beg, uint8_t **end) {
    int n = (int)(end - beg); if (n < 2 || n > 4096) return;
    int key[4096]; for (int i = 0; i < n; i++) key[i] = board_index(beg[i]);
    for (int i = 1; i < n; i++) {             // insertion sort (n is small)
        uint8_t *o = beg[i]; int k = key[i], j = i - 1;
        while (j >= 0 && key[j] > k) { beg[j + 1] = beg[j]; key[j + 1] = key[j]; j--; }
        beg[j + 1] = o; key[j + 1] = k;
    }
}
void __attribute__((cdecl, used)) tas_sort_vecobj(uint8_t *vec) { sort_objvec(*(uint8_t ***)(vec + 4), *(uint8_t ***)(vec + 8)); }
int __attribute__((cdecl, used)) tas_rand_purple(uint8_t *ebp) {
    sort_objvec(*(uint8_t ***)(ebp - 0x30), *(uint8_t ***)(ebp - 0x2c));
    return ((int (*)(void))0x4fce50)();
}
void tas_green_stub(void); void tas_purple_stub(void);
__asm__(".globl _tas_green_stub\n_tas_green_stub:\n push %esi\n call _tas_sort_vecobj\n add $4, %esp\n"
        " mov $0x43d430, %eax\n jmp *%eax\n"
        ".globl _tas_purple_stub\n_tas_purple_stub:\n push %ebp\n call _tas_rand_purple\n add $4, %esp\n ret\n");

// Real-time accounting (speedrun timing). Board::Update FUN_00410e10 runs once per 10 ms app update and
// calls the physics step 1x, or 8x (ModVal Board.cpp105) while the right button is held and the LogicMgr
// state is not 2 (shot in flight; 4x there only with the "scott" cheat, Board+0x11e). Its first call
// (0x410e13 → 0x53e990) is wrapped to count app frames; PEGGLE_FF=1 makes FUN_0053e370 ("right button
// down") return 1. Logged: 'frm F tick T state S' whenever the LogicMgr state changes, plus dialog
// creation ('dialog F vt ...').
static int g_frm_state = -99, g_frm_tick0;
static void frm_runlog(uint8_t *b);
static uint64_t g_dlgmask;
static void __attribute__((fastcall)) w_53e990(void *b) {
    g_bframes++;
    frm_runlog((uint8_t *)b);
    uint8_t *l = logic();
    int st = l ? *(int *)(l + 4) : -1;
    if (st != g_frm_state) {
        hlog("frm %d tick %d state %d", g_bframes, g_frames, st);
        g_frm_state = st;
    }
    if (g_humanfire && l && g_fire_at >= 0 && g_hstate == 2 && *(int *)(l + 4) == 1 && g_aim_held > 2
        && *(int *)(l + 0x138) == 0 && tick() >= g_fire_at) {
        // PEGGLE_HUMANFIRE=1: the click is made between frames (as a player's mouse click is), so a
        // fire tick that is not a frame start lands on the next frame start
        int late = tick() - g_fire_at;
        ((mousedown_fn)0x472810)(l, 400, 300, 1, 0, 0);
        g_shot_start = tick(); g_shots++; g_fire_seen = g_fired; g_last_click = tick();
        hlog("fire %d angle %.9g (0x%08x) tick %d sched %d frm %d%s (frame start)", g_shots, g_aim, *(unsigned *)&g_aim,
             g_shot_start, g_fire_at, g_bframes, late ? " LATE" : "");
        g_hstate = 3; g_fire_at = -1; g_prev_aim = g_aim;
    }
    while (l && st == 2 && g_flip_i < g_nflip && tick() >= g_flip_t[g_flip_i]) {
        int d = g_flip_d[g_flip_i];
        if (d) ((mousedown_fn)0x472810)(l, 400, 300, 1, 0, 0);
        else ((mouseup_fn)0x45ebb0)(l, 400, 300, 1, 0);
        hlog("flip %s tick %d sched %d frm %d flippers %d (frame start)", d ? "down" : "up", tick(), g_flip_t[g_flip_i], g_bframes,
             *(int *)(l + 0x1e4 + *(int *)(l + 0x128) * 4));
        g_flip_i++;
    }
    if (g_autoclick && l) {
        // a player's clicks, made between frames: skip the level intro (state 8), cancel the near-win
        // slow-mo (state 2 with Board+0xc1 and LogicMgr+0xf9), and confirm the level end (state 5, +0xfd)
        uint8_t *bd = (uint8_t *)b;
        const char *why = 0;
        if (st == 8 && !l[0x100]) why = "intro";
        // (not while the flipper power is on: that click would flip, LogicMgr::MouseDown → FUN_0045ea70)
        else if (st == 2 && bd[0xc1] && l[0xf9] && *(int *)(l + 0x1e4 + *(int *)(l + 0x128) * 4) < 1) why = "slowmo";
        else if (st == 5 && l[0xfd] && *(int *)(l + 8) < 100) why = "end";
        if (st == 5 && ((g_bframes % 25) == 0 || why)) { // also when confirming the end (it may quit before a 25th frame)
            uint8_t *x160 = *(uint8_t **)(bd + 0x160);
            uint8_t *sw = 0; // score widget: FUN_00436e20(this) compares this+0x94 with LogicMgr+0x174
            hlog("st5 frm %d l8 %d fd %d 160.10 %d total %d", g_bframes, *(int *)(l + 8), l[0xfd], x160 ? *(int *)(x160 + 0x10) : -1, *(int *)(l + 0x174));
            (void)sw;
        }
        if (shown_dialog()) { hlog("autoclick dialog frm %d tick %d state %d", g_bframes, g_frames, st); dismiss_dialogs_f(1); }
        if (why) {
            hlog("autoclick %s frm %d tick %d btick %d", why, g_bframes, g_frames, *(int *)(bd + 0x1c4));
            ((mousedown_fn)0x472810)(l, 400, 300, 1, 0, 0);
        }
    }
    {   // profile character (+0x40) watch
        static int last = -99; uint8_t *app = APP(); uint8_t *pf = app ? *(uint8_t **)(app + 0x878) : 0;
        int c = pf ? *(int *)(pf + 0x40) : -1;
        if (c != last) { hlog("profile char %d frm %d (logic power %d)", c, g_bframes, l ? *(int *)(l + 0x1c4) : -1); last = c; }
    }
    {   // dialogs shown (app->GetDialog(id) for ids 0..63)
        uint8_t *app = APP();
        if (app) {
            typedef void *(__attribute__((thiscall)) *getdlg_fn)(void *, int);
            getdlg_fn getdlg = (getdlg_fn)(*(void ***)app)[0x124 / 4];
            uint64_t m = 0;
            for (int id = 0; id < 64; id++) if (getdlg(app, id)) m |= 1ull << id;
            if (m != g_dlgmask) { hlog("dialogs frm %d tick %d mask %llx", g_bframes, g_frames, (unsigned long long)m); g_dlgmask = m; }
        }
    }
    ((update_fn)0x53e990)(b);
}
// Per-frame run-length log (PEGGLE_FRMLOG=1), taken at the next frame's start: board tick delta over the
// previous frame, LogicMgr state, Board+0xc1 (slow-mo on), +0xc8, +0xcc, +0x104.
static int g_frmlog, g_rl_start = -1, g_rl_key[6], g_prev_bt;
static void frm_runlog(uint8_t *b) {
    if (!g_frmlog || !b) return;
    uint8_t *l = *(uint8_t **)(b + 0x154);
    int bt = *(int *)(b + 0x1c4);
    int key[6] = { bt - g_prev_bt, l ? *(int *)(l + 4) : -1, b[0xc1] | b[0xc3] << 1, *(int *)(b + 0xc8), *(int *)(b + 0xd0), *(int *)(b + 0x104) };
    g_prev_bt = bt;
    if (g_rl_start < 0 || memcmp(key, g_rl_key, sizeof key) != 0) {
        hlog("frl %d btick %d d %d st %d c1 %d c8 %d d0 %d 104 %d d4 %d d8 %d cc %d", g_bframes, bt, key[0], key[1], key[2], key[3], key[4], key[5], *(int *)(b + 0xd4), *(int *)(b + 0xd8), *(int *)(b + 0xcc));
        memcpy(g_rl_key, key, sizeof key); g_rl_start = g_bframes;
    }
}
static void frames_init(FILE *info) {
    patch_call(0x410e13, 0x53e990, (void *)w_53e990);
    g_frmlog = getenv("PEGGLE_FRMLOG") != 0;
    { const char *ac = getenv("PEGGLE_CLICKS"); g_autoclick = ac && atoi(ac); }
    { const char *hf = getenv("PEGGLE_HUMANFIRE"); g_humanfire = hf && atoi(hf); }
    const char *ff = getenv("PEGGLE_FF");
    if (ff && atoi(ff)) {
        uint8_t *p = (uint8_t *)0x53e370;
        DWORD o; VirtualProtect(p, 3, PAGE_EXECUTE_READWRITE, &o);
        p[0] = 0xB0; p[1] = 0x01; p[2] = 0xC3;   // mov al,1; ret
        VirtualProtect(p, 3, o, &o);
        FlushInstructionCache(GetCurrentProcess(), p, 3);
        if (info) fprintf(info, "fast forward held\n");
    }
}

static void tas_init(FILE *info) {
    const char *s = getenv("PEGGLE_RNG_SEED");
    if (s) {
        g_rng_seed = (uint32_t)strtoul(s, 0, 0); g_rng_on = 1;
        patch_call(0x45e0f4, 0x457960, (void *)w_457960);
        patch_call(0x45e0fb, 0x458110, (void *)w_458110); patch_call(0x46ea80, 0x458110, (void *)w_458110);
        patch_call(0x45e102, 0x457dc0, (void *)w_457dc0); patch_call(0x46ea9c, 0x457dc0, (void *)w_457dc0);
        patch_call(0x4613bb, 0x4582f0, (void *)w_4582f0);
        patch_call(0x457fb2, 0x43d430, (void *)tas_green_stub);
        patch_call(0x45827c, 0x4fce50, (void *)tas_purple_stub);
        patch_call(0x458885, 0x43d430, (void *)tas_green_stub);   // flipper orange relocation shuffle
        patch_call(0x45f1bf, 0x45eca0, (void *)w_45eca0);         // spooky ball respawn search (out of bounds)
        patch_call(0x4703f9, 0x45eca0, (void *)w_45eca0);         // spooky ball respawn search (fever hole FUN_00470230)
        patch_call(0x46ed54, 0x4fce50, (void *)w_rand_lucky);     // lucky spin wheel result
        patch_call(0x46adad, 0x4fce50, (void *)w_rand_shot);      // fire: LogicMgr+0x2c/+0x30 rolls
        patch_call(0x40f660, 0x4fce50, (void *)w_rand_char);      // level load: random character (stage 10)
        patch_call(0x46edb6, 0x4023f0, (void *)w_4023f0);         // lucky spin slot machine (normal path)
        patch_call(0x46d2ac, 0x467b80, (void *)w_467b80);         // zen shot search
        if (info) fprintf(info, "tas: rng seed %u\n", g_rng_seed);
    }
    const char *pg = getenv("PEGGLE_PROGRESS"); if (pg) g_progress = atoi(pg);
    g_interactive = getenv("PEGGLE_INTERACTIVE") != 0;
    const char *ch = getenv("PEGGLE_CHAR"); if (ch) g_char = strcmp(ch, "stage") == 0 ? -2 : atoi(ch);
}
static void *g_tramp_aimupd;
// Gun SetAngle FUN_004843a0(gun, a, instant=1): sets +0x150 and the slew target +0x154, re-places the
// loaded ball (FUN_00483da0) and pushes the aim-change stamp +0x18c back 1000 frames (so Gun::Fire never
// substitutes the older angle +0x188). The game itself calls it this way when firing (0x46d8d1).
typedef int (__attribute__((thiscall)) *setangle_fn)(void *, float, int);
static void gun_set_angle(uint8_t *g, float a) {
    if (*(float *)(g + 0x150) == a && *(float *)(g + 0x154) == a) return;
    if (*(float *)(g + 0x150) == a) *(float *)(g + 0x150) = a + 1.0f;   // SetAngle returns early on equal
    ((setangle_fn)0x4843a0)(g, a, 1);
}
// geometric aim of a window pixel from the gun pivot (window (400, 30)): angle of (dx, -dy)
static float cur_geom(float x, float y) {
    float a = atan2f(-(y - 30.0f), x - 400.0f);
    if (a < 0) a += 6.2831853f;
    return a;
}
static float wrapd(float d) { while (d > 3.14159265f) d -= 6.2831853f; while (d < -3.14159265f) d += 6.2831853f; return d; }
static void cur_put(float x, float y) {
    g_cur_x = x; g_cur_y = y;
    if (!g_cur_org_ok) {
        HWND w = GetForegroundWindow(); if (!w) w = GetActiveWindow();
        POINT p = {0, 0};
        if (w && ClientToScreen(w, &p)) g_cur_org = p; else { g_cur_org.x = 112; g_cur_org.y = 76; }
        g_cur_org_ok = 1;
        hlog("cursor origin %ld %ld (hwnd %p)", g_cur_org.x, g_cur_org.y, w);
    }
    SetCursorPos(g_cur_org.x + (int)(x + 0.5f), g_cur_org.y + (int)(y + 0.5f));
}
// plan the glide for the shot just armed: start late enough to arrive 8 ticks before the click
static void cur_plan(float tx, float ty) {
    float dx = tx - g_cur_x, dy = ty - g_cur_y, dist = sqrtf(dx * dx + dy * dy);
    int dur = 30 + (int)(dist / 6.0f); if (dur > 100) dur = 100;  // ticks (fast-forward runs them ~2x wall speed)
    int now = tick(), end = g_fire_at >= 0 ? g_fire_at - 8 : now + dur;
    if (end - now < dur) dur = end - now;
    g_cur_x0 = g_cur_x; g_cur_y0 = g_cur_y; g_cur_x1 = tx; g_cur_y1 = ty;
    // launcher offset from the cursor's geometric angle (the mouse aim bends it): carried from the
    // previous shot's angle to this shot's exact angle so the launcher never jumps
    g_cur_off0 = g_prev_aim >= 0 ? wrapd(g_prev_aim - cur_geom(g_cur_x0, g_cur_y0)) : 0;
    g_cur_off1 = wrapd(g_aim - cur_geom(tx, ty));
    g_cur_bend = (float)(((unsigned)(g_shots + 1) * 2654435761u) >> 16 & 0xff) / 255.0f * 0.24f - 0.12f;  // slight arc
    if (dur < 3) { g_cur_t0 = g_cur_t1 = -1; cur_put(tx, ty); return; }
    g_cur_t1 = end; g_cur_t0 = end - dur;
}
// per aiming tick: cursor position and launcher angle; returns 1 while the glide is in progress
static int cur_step(float *vis) {
    int t = tick();
    if (g_cur_t1 < 0 || t >= g_cur_t1) {
        if (g_cur_t1 >= 0) { cur_put(g_cur_x1, g_cur_y1); g_cur_t0 = g_cur_t1 = -1; }
        return 0;
    }
    if (t < g_cur_t0) { *vis = g_prev_aim >= 0 ? g_prev_aim : g_aim; return 1; }  // resting before the glide
    float u = (float)(t - g_cur_t0) / (float)(g_cur_t1 - g_cur_t0);
    float e = u * u * (3.0f - 2.0f * u);                    // ease in-out
    float dx = g_cur_x1 - g_cur_x0, dy = g_cur_y1 - g_cur_y0;
    float arc = g_cur_bend * 4.0f * e * (1.0f - e);         // perpendicular arc, zero at both ends
    float x = g_cur_x0 + dx * e - dy * arc, y = g_cur_y0 + dy * e + dx * arc;
    cur_put(x, y);
    float a = cur_geom(x, y) + g_cur_off0 + (g_cur_off1 - g_cur_off0) * e;
    // keep out of the upward cone (7..173 degrees): allowed [173, 367] degrees
    while (a < 0) a += 6.2831853f; while (a >= 6.2831853f) a -= 6.2831853f;
    const float lo = 173.0f * 3.14159265f / 180.0f, hi = 7.0f * 3.14159265f / 180.0f;
    if (a > hi && a < lo) a = (a < 1.5707963f) ? hi : lo;
    *vis = a;
    return 1;
}
static int g_firedbg = -1;
static void __attribute__((fastcall)) hook_aimupd(uint8_t *l) {
    if (g_firedbg < 0) g_firedbg = getenv("PEGGLE_FIREDBG") != 0;
    // The gun caches its launch direction in its own update, so the aim is written every aiming tick
    // from the moment the shot command is active and the fire waits until it has been held >= 2 ticks.
    uint8_t *g0 = (g_fire_at >= 0 && g_hstate == 2 && l == logic()) ? gun() : 0;
    float vis;
    if (g0 && g_mouse_shot) {
        if (*(float *)(g0 + 0x150) == *(float *)(l + 0xe0)) g_aim_held++; else g_aim_held = 0;
    }
    else if (g0 && g_cur_on && cur_step(&vis)) { *(float *)(l + 0xe0) = vis; gun_set_angle(g0, vis); g_aim_held = 0; }
    else if (g0) { *(float *)(l + 0xe0) = g_aim; gun_set_angle(g0, g_aim); g_aim_held++; }
    else g_aim_held = 0;
    if (g0 && !g_humanfire && tick() >= g_fire_at && g_aim_held > 2 && *(int *)(l + 0x138) == 0) {
        uint8_t *g = g0;
        {
            float fa = g_mouse_shot ? *(float *)(l + 0xe0) : g_aim;
            if (!g_mouse_shot) { *(float *)(l + 0xe0) = g_aim; gun_set_angle(g, g_aim); }
            int late = tick() - g_fire_at;
            ((mousedown_fn)0x472810)(l, 400, 300, 1, 0, 0);
            g_shot_start = tick(); g_shots++; g_fire_seen = g_fired; g_last_click = tick();
            hlog("fire %d angle %.9g (0x%08x) tick %d sched %d%s", g_shots, fa, *(unsigned *)&fa,
                 g_shot_start, g_fire_at, late ? " LATE" : "");
            g_hstate = 3; g_fire_at = -1; g_mouse_shot = 0; g_prev_aim = fa;
        }
    }
    ((update_fn)g_tramp_aimupd)(l);
    if (g_firedbg && l == logic() && g_hstate == 3 && g_fired == g_fire_seen && tick() - g_shot_start < 130) {
        uint8_t *g = gun();
        hlog("firedbg t %d state %d ed %d ec %d f0 %d 138 %d 244 %d gun1c0 %d", tick(), *(int *)(l + 4), l[0xed], l[0xec],
             *(int *)(l + 0xf0), *(int *)(l + 0x138), l[0x244 + *(int *)(l + 0x128)], g ? *(int *)(g + 0x1c0) : -1);
    }
}

static void clock_block_begin(void);
static void clock_block_end(void);
static void harness_step(void) {
    uint8_t *a = APP();
    if (!a || !a[0x4fa]) return;           // resources not loaded yet
    uint8_t *l = logic();
    if (g_hstate >= 1 && g_hstate <= 3) dismiss_dialogs();
    switch (g_hstate) {
    case 0: {
        char line[256];
        // ready for the next command: dump the board state once ('RDY ' marker, then LGC/PEG/BRD)
        // LogicMgr+0x138 != 0: a hint dialog is up (1) or its 30-tick close cooldown runs (2..29); clicks are
        // ignored then (FUN_00472810), so the board is not "ready" until it is 0.
        if (!g_rdy_done && l && *(int *)(l + 4) == 1 && *(int *)(l + 0x138) != 0 && !g_level_over) { dismiss_dialogs(); return; }
        if (!g_rdy_done && l && (*(int *)(l + 4) == 1 || g_level_over)) {
            g_rdy_done = 1;
            uint8_t buf[BALL_SIZE] = {0}; *(int *)buf = tick(); *(int *)(buf + 4) = g_shots; *(int *)(buf + 8) = g_level_over;
            {   // +12: bucket counter (first moving type-1 hole with a mover), +16: LogicMgr state
                uint8_t *bd = *(uint8_t **)(l + 0x104); *(int *)(buf + 12) = -1; *(int *)(buf + 16) = *(int *)(l + 4);
                uint8_t **head = bd ? *(uint8_t ***)(bd + 0x194) : 0;
                // +20: counter of the LAST moving object with a mover in board order (a level object if any), -1 if none
                *(int *)(buf + 20) = -1; int found_b = 0, found_l = 0, idx = 0;
                for (uint8_t **nd = head ? (uint8_t **)head[0] : 0; nd && nd != (uint8_t **)head; nd = (uint8_t **)nd[0], idx++) {
                    uint8_t *o = nd[2];
                    if (!o || !*(uint32_t *)(o + 0x48)) continue;
                    if (!found_b && *(uint32_t *)(o + 0x10) == 1) {
                        unsigned len; const char *id = obj_id(o, &len);   // the free-ball bucket hole (base object "freeball")
                        if (len == 8 && memcmp(id, "freeball", 8) == 0) { *(int *)(buf + 12) = *(int *)(o + 0x4c); found_b = 1; }
                    }
                    if (o[0x26]) { *(int *)(buf + 20) = *(int *)(o + 0x4c); found_l = 1; }  // last moving object wins
                }
            }
            log_blob(0x20594452 /*'RDY '*/, buf, (uint32_t)tick());
            dump_logic(); dump_pegs(); dump_board();
        }
        if (!g_script) { hlog("script end"); g_hstate = 9; return; }
        static float pend_x = -1, pend_y = -1;
    next_line:
        if (!fgets(line, sizeof line, g_script)) {
            // PEGGLE_INTERACTIVE: freeze the game here until the driver appends a command
            if (!g_interactive) { hlog("script end"); g_hstate = 9; return; }
            hlog("waiting for command at tick %d", tick());
            DWORD t0 = GetTickCount();
            long off = ftell(g_script);
            clock_block_begin();
            for (;;) {
                Sleep(50);
                // reopen each poll: the bind-mounted file's appended data is not seen through the old handle
                fclose(g_script); g_script = fopen(getenv("PEGGLE_SCRIPT"), "r");
                if (!g_script) continue;
                fseek(g_script, off, SEEK_SET);
                if (fgets(line, sizeof line, g_script) && strchr(line, '\n')) break;
                if (GetTickCount() - t0 > 1800000) { hlog("interactive timeout"); ExitProcess(2); }
            }
            clock_block_end();
        }
        {   float cx, cy;
            if (sscanf(line, "cursor %f %f", &cx, &cy) == 2) { pend_x = cx; pend_y = cy; g_cur_on = 1; goto next_line; }
            int ft; unsigned fd;   // `flip TICK 1|0`: queued for the next shot (no ready dump)
            if (sscanf(line, "flip %d %u", &ft, &fd) == 2) {
                if (g_nflip_pend < MAXFLIP) { g_flip_pt[g_nflip_pend] = ft; g_flip_pd[g_nflip_pend] = (int)fd; g_nflip_pend++; }
                hlog("cmd %s", line);
                goto next_line;
            }
            int ax, ay;            // `aimtest X Y`: call WidgetManager::MouseMove (FUN_00543270) and log Board offset + LogicMgr+0xe0
            if (sscanf(line, "aimtest %d %d", &ax, &ay) == 2) {
                uint8_t *b = board(), *wm = *(uint8_t **)(a + 800);
                typedef int (__attribute__((thiscall)) *wmmove_fn)(void *, int, int);
                if (wm) ((wmmove_fn)0x543270)(wm, ax, ay);
                int dirty = l ? (int)l[0x1f] : -1;
                if (l && l[0x1f]) { typedef void (__attribute__((thiscall)) *aim_fn)(void *, char); ((aim_fn)0x43ece0)(l, 0); }
                float ang = l ? *(float *)(l + 0xe0) : -1.0f;
                uint32_t bits; memcpy(&bits, &ang, 4);
                int bx = b ? *(int *)(b + 0x1a8) : -999, by = b ? *(int *)(b + 0x1ac) : -999;
                int wmx = wm ? *(int *)(wm + 0xe0) : -999, wmy = wm ? *(int *)(wm + 0xe4) : -999;
                hlog("aimtest win %d %d wm %d %d bofs %d %d dirty %d angle %.9g (0x%08x)", ax, ay, wmx, wmy, bx, by, dirty, ang, bits);
                goto next_line;
            }
        }
        g_rdy_done = 0;
        hlog("cmd %s", line);
        int s, lv, n; float f; unsigned u;
        if (sscanf(line, "level %d %d", &s, &lv) == 2) {
            *(int *)(a + 0x760) = g_mode; *(int *)(a + 0x764) = s; *(int *)(a + 0x768) = lv;
            g_rng_ctr = 0;
            if (g_progress >= 0 && *(uint8_t **)(a + 0x878)) {
                uint8_t *prof = *(uint8_t **)(a + 0x878);
                *(int *)(prof + 0x28) = g_progress / 5; *(int *)(prof + 0x2c) = g_progress % 5;
            }
            // character (FUN_0040f400): adventure with profile stage < 10 uses the stage's master; else
            // the profile's chosen character +0x40 (0 Bjorn .. 9 Hu; 10 = random, avoid).
            if (*(uint8_t **)(a + 0x878)) {
                int ch = g_char >= 0 ? g_char : (g_char == -2 ? (s < 10 ? s : 0) : -1);
                if (ch >= 0) { *(int *)(*(uint8_t **)(a + 0x878) + 0x40) = ch; hlog("character %d (readback %d)", ch, *(int *)(*(uint8_t **)(a + 0x878) + 0x40)); }
            }
            uint8_t *b = board();
            hlog("level %d %d (board %p profile %p) frm %d tick %d", s, lv, b, *(void **)(a + 0x878), g_bframes, g_frames);
            if (b) ((startlevel_fn)0x42da00)(b); else ((showboard_fn)0x42f860)(a, 0, 1);
            uint32_t sl[2] = { (uint32_t)s, (uint32_t)lv };
            uint8_t buf[BALL_SIZE] = {0}; memcpy(buf, sl, 8);
            // level descriptor = FUN_00405ba0(app); name = std::string at desc+0x14 (4-byte prefix; buf/ptr +0x18, size +0x28, cap +0x2c)
            typedef uint8_t *(__attribute__((fastcall)) *getlvl_fn)(void *);
            uint8_t *desc = ((getlvl_fn)0x405ba0)(a);
            if (desc) {
                uint8_t *str = desc + 0x18; unsigned len = *(unsigned *)(str + 0x10), cap = *(unsigned *)(str + 0x14);
                const char *name = cap < 16 ? (const char *)str : *(const char **)str;
                if (len > 200) len = 200;
                memcpy(buf + 8, name, len);
                hlog("level name %.*s", len, name);
            }
            log_blob(0x204c564c /*'LVL '*/, buf, 0);
            g_hstate = 1; g_cmd_tick = 0; g_level_over = 0;
        } else if (sscanf(line, "mouseshot %d %d %d", &s, &lv, &n) == 3) {
            if (g_level_over) return;
            uint8_t *wm = *(uint8_t **)(a + 800);
            typedef int (__attribute__((thiscall)) *wmmove_fn)(void *, int, int);
            if (wm) ((wmmove_fn)0x543270)(wm, s, lv);
            if (l && l[0x1f]) { typedef void (__attribute__((thiscall)) *aim_fn)(void *, char); ((aim_fn)0x43ece0)(l, 0); }
            g_fire_at = n; g_mouse_shot = 1; g_aim_since = -1; g_hstate = 2;
            memcpy(g_flip_t, g_flip_pt, sizeof g_flip_t); memcpy(g_flip_d, g_flip_pd, sizeof g_flip_d);
            g_nflip = g_nflip_pend; g_nflip_pend = 0; g_flip_i = 0;
        } else if (strncmp(line, "wheelshot ", 10) == 0 || strncmp(line, "keyshot ", 8) == 0) {
            int wx, wy, p1, p2, at;
            if (!g_level_over && sscanf(strchr(line, ' ') + 1, "%d %d %d %d %d", &wx, &wy, &p1, &p2, &at) == 5) {
                uint8_t *wm = *(uint8_t **)(a + 800), *b = board();
                typedef int (__attribute__((thiscall)) *wmmove_fn)(void *, int, int);
                if (wm) ((wmmove_fn)0x543270)(wm, wx, wy);
                if (l && l[0x1f]) { typedef void (__attribute__((thiscall)) *aim_fn)(void *, char); ((aim_fn)0x43ece0)(l, 0); }
                if (line[0] == 'w') {
                    typedef int (__attribute__((thiscall)) *wheel_fn)(void *, int);
                    for (int k = 0; k < p2; k++) ((wheel_fn)0x43d6a0)(l, p1);
                } else {
                    typedef void (__attribute__((thiscall)) *key_fn)(void *, int);
                    for (int k = 0; k < p2; k++) ((key_fn)0x402930)(b, p1);
                }
                float ang = l ? *(float *)(l + 0xe0) : -1.0f; uint32_t bits; memcpy(&bits, &ang, 4);
                hlog("inputshot %c win %d %d p %d %d -> angle %.9g (0x%08x)", line[0], wx, wy, p1, p2, ang, bits);
                g_fire_at = at; g_mouse_shot = 1; g_aim_since = -1; g_hstate = 2;
                memcpy(g_flip_t, g_flip_pt, sizeof g_flip_t); memcpy(g_flip_d, g_flip_pd, sizeof g_flip_d);
                g_nflip = g_nflip_pend; g_nflip_pend = 0; g_flip_i = 0;
            }
        } else if (strncmp(line, "slewtest ", 9) == 0) {
            unsigned u1, u2;
            if (sscanf(line + 9, "%x %x", &u1, &u2) == 2 && gun()) {
                uint8_t *g = gun(); float a1, a2; memcpy(&a1, &u1, 4); memcpy(&a2, &u2, 4);
                *(float *)(g + 0x150) = a1; *(float *)(g + 0x154) = a2;
                typedef void (__attribute__((fastcall)) *gupd_fn)(void *);
                for (int k = 0; k < 8; k++) {
                    ((gupd_fn)0x484120)(g);
                    float c = *(float *)(g + 0x150); uint32_t cb; memcpy(&cb, &c, 4);
                    hlog("slew %d angle %.9g (0x%08x)", k + 1, c, cb);
                }
                goto next_line;
            }
        } else if (sscanf(line, "shotx %x", &u) == 1 || sscanf(line, "shot %f", &f) == 1) {
            if (g_level_over) return;           // level finished: ignore remaining shots
            if (strncmp(line, "shotx", 5) == 0) memcpy(&f, &u, 4);
            { char w[32]; int at; g_fire_at = (sscanf(line, "%15s %15s %d", w, w + 16, &at) == 3) ? at : -1; }
            g_aim = f; g_mouse_shot = 0; g_aim_since = -1; g_hstate = 2;
            memcpy(g_flip_t, g_flip_pt, sizeof g_flip_t); memcpy(g_flip_d, g_flip_pd, sizeof g_flip_d);
            g_nflip = g_nflip_pend; g_nflip_pend = 0; g_flip_i = 0;
            if (pend_x >= 0) { cur_plan(pend_x, pend_y); pend_x = pend_y = -1; }
        } else if (sscanf(line, "mode %d", &n) == 1) {
            g_mode = n;                       // app+0x760 for later `level` commands (1 adventure)
        } else if (sscanf(line, "char %d", &n) == 1) {
            g_char = n;
        } else if (sscanf(line, "waitms %d", &n) == 1) {
            // wall-clock wait (game keeps running, e.g. the level-end screen where ticks stop): for videos
            g_wait_ms = GetTickCount() + (DWORD)n; g_hstate = 5;
        } else if (sscanf(line, "wait %d", &n) == 1) {
            g_wait_until = tick() + n; g_hstate = 4;
        } else if (strncmp(line, "quit", 4) == 0) {
            hlog("quit"); if (g_log) fflush(g_log); ExitProcess(0);
        }
        return;
    }
    case 1:                                  // wait for aiming state on the new board
        if (l && gun() && *(int *)(l + 4) == 1) {
            uint8_t *pf = *(uint8_t **)(APP() + 0x878), *bd = board();
            hlog("level ready tick %d power %d char %d prog %d mode %d", tick(), *(int *)(l + 0x1c4),
                 pf ? *(int *)(pf + 0x40) : -1, pf ? *(int *)(pf + 0x28) : -1, bd ? *(int *)(bd + 0xb4) : -1);
            g_hstate = 0;
        }
        else if (++g_cmd_tick % 2000 == 0) hlog("still waiting for level: logic %p state %d", l, l ? *(int *)(l + 4) : -1);
        return;
    case 2: {                                // aim, wait for gun to settle, then fire
        if (l && *(int *)(l + 4) == 5) { hlog("level over before shot"); g_level_over = 1; g_hstate = 0; return; }
        if (!l || *(int *)(l + 4) != 1) return;
        if (g_fire_at >= 0) return;          // exact-tick fire happens in hook_aimupd
        uint8_t *g = gun();
        *(float *)(l + 0xe0) = g_aim;
        if (*(float *)(g + 0x150) != g_aim) { g_aim_since = -1; return; }
        if (g_aim_since < 0) g_aim_since = tick();
        if (tick() - g_aim_since < 15) return;
        click(l);
        g_shot_start = tick(); g_shots++; g_fire_seen = g_fired;
        hlog("fire %d angle %.9g (0x%08x) tick %d", g_shots, g_aim, *(unsigned *)&g_aim, g_shot_start);
        g_hstate = 3;
        return;
    }
    case 3:                                  // wait until back to aiming (or level over)
        if (!l) return;
        if (g_fired == g_fire_seen) {        // fire request not yet taken by the game
            if ((tick() - g_shot_start) % 50 == 49 && *(int *)(l + 4) == 1) { *(float *)(l + 0xe0) = g_aim; click(l); }
            if (tick() - g_shot_start > 1000) { hlog("shot %d: fire request ignored (state %d)", g_shots, *(int *)(l + 4)); g_hstate = 0; }
            return;
        }
        if ((tick() - g_shot_start) % 3000 == 2999) {
            uint8_t *app = APP();
            typedef void *(__attribute__((thiscall)) *getdlg_fn)(void *, int);
            getdlg_fn getdlg = (getdlg_fn)(*(void ***)app)[0x124 / 4];
            for (int i = 0; i < g_ndialogs; i++) {
                int id = *(int *)((uint8_t *)g_dialogs[i] + 0x88 + 0xb4);
                hlog("  dialog %p vt %08x id %d reg %p", g_dialogs[i], *(unsigned *)g_dialogs[i], id, getdlg(app, id));
            }
            hlog("  state %d", *(int *)(l + 4));
        }
        if (*(int *)(l + 4) == 5) {           // logic state 5 = level over (EndLevelDialog shown as board child)
            hlog("shot %d over: level ended tick %d", g_shots, tick());
            log_blob(0x20444e45 /*'END '*/, l, (uint32_t)tick() | 0x80000000u);
            g_level_over = 1; g_hstate = 0;
            return;
        }
        if (*(int *)(l + 4) == 1) {
            hlog("shot %d over tick %d state %d", g_shots, tick(), *(int *)(l + 4));
            log_blob(0x20444e45 /*'END '*/, l, (uint32_t)tick());
            g_hstate = 0;
        } else if (tick() - g_shot_start > 30000) {
            hlog("shot %d timeout state %d", g_shots, *(int *)(l + 4)); g_hstate = 0;
        }
        return;
    case 4:
        if (tick() >= g_wait_until) g_hstate = 0;
        return;
    case 5:
        if ((int)(GetTickCount() - g_wait_ms) >= 0) g_hstate = 0;
        return;
    }
}


// --- sound log (PEGGLE_SOUNDLOG=1, videos): the game resolves BASS by name from this proxy, so these
// exports wrap the calls it makes, log them (wall ms = hlog clock, board frame) to /work/sound.txt and
// forward to bass_orig. In Peggle BASS only loads the music (the effects use DirectSound, below); loaded
// data is dumped once per hash to /work/snd/.
static FILE *g_snd; static int g_snd_on = -1;
static HMODULE g_bass;
static FARPROC borig(const char *n) {
    if (!g_bass) g_bass = LoadLibraryA("bass_orig.dll");
    return g_bass ? GetProcAddress(g_bass, n) : 0;
}
static int snd_on(void) {
    if (g_snd_on < 0) {
        const char *e = getenv("PEGGLE_SOUNDLOG"); g_snd_on = e && atoi(e);
        if (g_snd_on) { char p[300]; sprintf(p, "%s\\sound.txt", out_dir()); g_snd = fopen(p, "w"); sprintf(p, "%s\\snd", out_dir()); CreateDirectoryA(p, 0); if (!g_snd) g_snd_on = 0; }
    }
    return g_snd_on;
}
static void slog(const char *fmt, ...) {
    if (!snd_on()) return;
    va_list ap; va_start(ap, fmt);
    fprintf(g_snd, "%lu %d ", (unsigned long)GetTickCount(), g_bframes);
    vfprintf(g_snd, fmt, ap); fputc('\n', g_snd); fflush(g_snd);
    va_end(ap);
}
static unsigned long long fnv(const uint8_t *p, size_t n) {
    unsigned long long h = 1469598103934665603ull;
    for (size_t i = 0; i < n; i++) { h ^= p[i]; h *= 1099511628211ull; }
    return h;
}
// hash of the data a load call names (memory block or file), dumped once to /work/snd/<hash>
static unsigned long long snd_data(int mem, const void *file, unsigned long long off, unsigned long long len) {
    if (!snd_on()) return 0;
    uint8_t *buf = 0; size_t n = 0;
    if (mem) { if (!len) return 0; buf = (uint8_t *)file + off; n = (size_t)len; }
    else {
        FILE *f = fopen((const char *)file, "rb"); if (!f) return 0;
        fseek(f, 0, SEEK_END); long sz = ftell(f); fseek(f, (long)off, SEEK_SET);
        n = len ? (size_t)len : (size_t)(sz - (long)off);
        buf = malloc(n); if (!buf) { fclose(f); return 0; }
        n = fread(buf, 1, n, f); fclose(f);
    }
    unsigned long long h = fnv(buf, n);
    char path[320]; sprintf(path, "%s\\snd\\%016llx", out_dir(), h);
    FILE *o = fopen(path, "rb");
    if (o) fclose(o); else if ((o = fopen(path, "wb"))) { fwrite(buf, 1, n, o); fclose(o); }
    if (!mem) free(buf);
    return h;
}
typedef BOOL (WINAPI *init_t)(int, DWORD, DWORD, HWND, const void *);
BOOL WINAPI BASS_Init(int dev, DWORD freq, DWORD flags, HWND win, const void *g) {
    init_t f = (init_t)borig("BASS_Init");
    { typedef DWORD (WINAPI *gv_t)(void); gv_t gv = (gv_t)borig("BASS_GetVersion"); slog("version %08lx", gv ? (unsigned long)gv() : 0ul); }
    BOOL r = f(dev, freq, flags, win, g);
    slog("init dev %d freq %lu r %d", dev, (unsigned long)freq, r);   // music only, not played in levels
    return r;
}
// BASS 2.2: 32-bit file offsets (stack layout checked live)
typedef DWORD (WINAPI *sload_t)(BOOL, const void *, DWORD, DWORD, DWORD, DWORD);
DWORD WINAPI BASS_SampleLoad(BOOL mem, const void *file, DWORD off, DWORD len, DWORD max, DWORD flags) {
    DWORD h = ((sload_t)borig("BASS_SampleLoad"))(mem, file, off, len, max, flags);
    if (snd_on() && h) slog("sload %lu %016llx flags %lx", (unsigned long)h, snd_data(mem, file, off, len), (unsigned long)flags);
    return h;
}
typedef DWORD (WINAPI *screate_t)(BOOL, const void *, DWORD, DWORD, DWORD);
DWORD WINAPI BASS_StreamCreateFile(BOOL mem, const void *file, DWORD off, DWORD len, DWORD flags) {
    DWORD h = ((screate_t)borig("BASS_StreamCreateFile"))(mem, file, off, len, flags);
    if (snd_on() && h) slog("stream %lu %016llx flags %lx", (unsigned long)h, snd_data(mem, file, off, len), (unsigned long)flags);
    return h;
}
typedef DWORD (WINAPI *mload_t)(BOOL, const void *, DWORD, DWORD, DWORD, DWORD);
DWORD WINAPI BASS_MusicLoad(BOOL mem, const void *file, DWORD off, DWORD len, DWORD flags, DWORD freq) {
    DWORD h = ((mload_t)borig("BASS_MusicLoad"))(mem, file, off, len, flags, freq);
    if (snd_on() && h) slog("music %lu %016llx flags %lx freq %lu", (unsigned long)h, snd_data(mem, file, off, len), (unsigned long)flags, (unsigned long)freq);
    return h;
}
typedef BOOL (WINAPI *cattr_t)(DWORD, DWORD *, DWORD *, int *);
typedef DWORD (WINAPI *sgetch_t)(DWORD, BOOL);
DWORD WINAPI BASS_SampleGetChannel(DWORD s, BOOL onlynew) {
    DWORD c = ((sgetch_t)borig("BASS_SampleGetChannel"))(s, onlynew);
    if (snd_on() && c) {
        DWORD fr = 0, vol = 0; int pan = 0; ((cattr_t)borig("BASS_ChannelGetAttributes"))(c, &fr, &vol, &pan);
        slog("chan %lu sample %lu freq %lu vol %lu pan %d", (unsigned long)c, (unsigned long)s, (unsigned long)fr, (unsigned long)vol, pan);
    }
    return c;
}
typedef BOOL (WINAPI *b1_t)(DWORD);
typedef BOOL (WINAPI *b2_t)(DWORD, DWORD);
BOOL WINAPI BASS_ChannelPlay(DWORD c, BOOL restart) {
    slog("play %lu restart %d", (unsigned long)c, restart); return ((b2_t)borig("BASS_ChannelPlay"))(c, restart);
}
BOOL WINAPI BASS_ChannelStop(DWORD c) { slog("stop %lu", (unsigned long)c); return ((b1_t)borig("BASS_ChannelStop"))(c); }
BOOL WINAPI BASS_ChannelPause(DWORD c) { slog("pause %lu", (unsigned long)c); return ((b1_t)borig("BASS_ChannelPause"))(c); }
BOOL WINAPI BASS_SampleStop(DWORD s) { slog("sstop %lu", (unsigned long)s); return ((b1_t)borig("BASS_SampleStop"))(s); }
BOOL WINAPI BASS_ChannelSetFlags(DWORD c, DWORD fl) { slog("flags %lu %lx", (unsigned long)c, (unsigned long)fl); return ((b2_t)borig("BASS_ChannelSetFlags"))(c, fl); }
typedef BOOL (WINAPI *sattr_t)(DWORD, int, int, int);
BOOL WINAPI BASS_ChannelSetAttributes(DWORD c, int fr, int vol, int pan) {
    slog("attr %lu freq %d vol %d pan %d", (unsigned long)c, fr, vol, pan); return ((sattr_t)borig("BASS_ChannelSetAttributes"))(c, fr, vol, pan);
}
typedef BOOL (WINAPI *slide_t)(DWORD, int, int, int, DWORD);
BOOL WINAPI BASS_ChannelSlideAttributes(DWORD c, int fr, int vol, int pan, DWORD ms) {
    slog("slide %lu freq %d vol %d pan %d ms %lu", (unsigned long)c, fr, vol, pan, (unsigned long)ms);
    return ((slide_t)borig("BASS_ChannelSlideAttributes"))(c, fr, vol, pan, ms);
}
typedef BOOL (WINAPI *mattr_t)(DWORD, DWORD, DWORD);
BOOL WINAPI BASS_MusicSetAttribute(DWORD m, DWORD a, DWORD v) {
    slog("mattr %lu %lu %lu", (unsigned long)m, (unsigned long)a, (unsigned long)v); return ((mattr_t)borig("BASS_MusicSetAttribute"))(m, a, v);
}
typedef BOOL (WINAPI *setvol_t)(DWORD);
BOOL WINAPI BASS_SetVolume(DWORD v) { slog("volume %lu", (unsigned long)v); return ((setvol_t)borig("BASS_SetVolume"))(v); }
BOOL WINAPI BASS_StreamFree(DWORD h) { slog("free %lu", (unsigned long)h); return ((b1_t)borig("BASS_StreamFree"))(h); }
BOOL WINAPI BASS_MusicFree(DWORD h) { slog("free %lu", (unsigned long)h); return ((b1_t)borig("BASS_MusicFree"))(h); }
BOOL WINAPI BASS_SampleFree(DWORD h) { slog("sfree %lu", (unsigned long)h); return ((b1_t)borig("BASS_SampleFree"))(h); }


// --- fake DirectSound (PEGGLE_SOUNDLOG=1): SexyApp's DSoundManager plays the sound effects through
// dsound.dll (BASS only loads the music). Mixing live under emulation slows the game several times, so
// with sound logging DirectSoundCreate returns this stand-in instead: buffers keep their PCM, report
// play status from the wall clock, and every play/stop/volume/pan/frequency change goes to sound.txt
// (PCM dumped once per hash to /work/snd/<hash>.pcm) for an offline mix in sync with the video.
typedef struct { WORD fmt, ch; DWORD rate, bps; WORD align, bits, cb; } WFX;
typedef struct { DWORD size, flags, bytes, res; WFX *wfx; } DSBD;
typedef struct { int refs; uint8_t *p; DWORD n; unsigned long long hash; int dirty; } DsData;
typedef struct DsBuf { void **vt; int refs, id, primary; WFX wfx; DsData *d;
    LONG vol, pan; DWORD freq; int playing, loop; DWORD t0, pos0; } DsBuf;
typedef struct { void **vt; int refs; } DsObj;
static int g_ds_ids;
static void ds_hash(DsBuf *b) {
    DsData *d = b->d; if (!d || !d->dirty) return;
    d->hash = fnv(d->p, d->n); d->dirty = 0;
    char path[320]; sprintf(path, "%s\\snd\\%016llx.pcm", out_dir(), d->hash);
    FILE *o = fopen(path, "rb");
    if (o) fclose(o); else if ((o = fopen(path, "wb"))) { fwrite(d->p, 1, d->n, o); fclose(o); }
}
static DWORD ds_rate(DsBuf *b) { return b->freq ? b->freq : b->wfx.rate; }
// bytes played since Play at the current frequency
static DWORD ds_played(DsBuf *b) {
    unsigned long long ms = GetTickCount() - b->t0;
    return (DWORD)(b->pos0 + ms * ds_rate(b) / 1000 * (b->wfx.align ? b->wfx.align : 1));
}
static HRESULT WINAPI dsb_qi(DsBuf *b, const void *iid, void **out) { (void)iid; *out = b; b->refs++; return 0; }
static ULONG WINAPI dsb_addref(DsBuf *b) { return ++b->refs; }
static ULONG WINAPI dsb_release(DsBuf *b) {
    if (--b->refs > 0) return b->refs;
    if (b->d && --b->d->refs == 0) { free(b->d->p); free(b->d); }
    free(b); return 0;
}
static HRESULT WINAPI dsb_getcaps(DsBuf *b, DWORD *c) { c[1] = 0; c[2] = b->d ? b->d->n : 0; return 0; }
static HRESULT WINAPI dsb_getpos(DsBuf *b, DWORD *play, DWORD *wr) {
    DWORD n = b->d && b->d->n ? b->d->n : 1, x = b->playing ? ds_played(b) : b->pos0;
    x = b->loop ? x % n : (x < n ? x : n);
    if (play) *play = x; if (wr) *wr = x; return 0;
}
static HRESULT WINAPI dsb_getformat(DsBuf *b, WFX *w, DWORD sz, DWORD *got) { if (w && sz >= 16) memcpy(w, &b->wfx, sz < sizeof(WFX) ? sz : sizeof(WFX)); if (got) *got = sizeof(WFX); return 0; }
static HRESULT WINAPI dsb_getvol(DsBuf *b, LONG *v) { *v = b->vol; return 0; }
static HRESULT WINAPI dsb_getpan(DsBuf *b, LONG *v) { *v = b->pan; return 0; }
static HRESULT WINAPI dsb_getfreq(DsBuf *b, DWORD *v) { *v = ds_rate(b); return 0; }
static HRESULT WINAPI dsb_getstatus(DsBuf *b, DWORD *st) {
    if (b->playing && !b->loop && b->d && ds_played(b) >= b->d->n) b->playing = 0;
    *st = b->playing ? (1 | (b->loop ? 4 : 0)) : 0; return 0;
}
static HRESULT WINAPI dsb_init(DsBuf *b, void *ds, DSBD *d) { (void)b; (void)ds; (void)d; return 0; }
static HRESULT WINAPI dsb_lock(DsBuf *b, DWORD off, DWORD bytes, void **p1, DWORD *n1, void **p2, DWORD *n2, DWORD flags) {
    DsData *d = b->d;
    if (!d || !d->n) { *p1 = 0; *n1 = 0; if (p2) *p2 = 0; if (n2) *n2 = 0; return 0; }
    if (flags & 2) { off = 0; bytes = d->n; }              // DSBLOCK_ENTIREBUFFER
    off %= d->n; if (bytes > d->n) bytes = d->n;
    DWORD a = d->n - off < bytes ? d->n - off : bytes;
    *p1 = d->p + off; *n1 = a;
    if (p2) *p2 = bytes > a ? d->p : 0; if (n2) *n2 = bytes - a;
    return 0;
}
static HRESULT WINAPI dsb_play(DsBuf *b, DWORD r1, DWORD prio, DWORD flags) {
    (void)r1; (void)prio;
    if (b->primary) return 0;
    ds_hash(b);
    b->playing = 1; b->loop = (flags & 1) != 0; b->t0 = GetTickCount();
    slog("dplay %d %016llx rate %lu ch %u bits %u freq %lu vol %ld pan %ld loop %d pos %lu", b->id, b->d ? b->d->hash : 0ull,
         (unsigned long)b->wfx.rate, b->wfx.ch, b->wfx.bits, (unsigned long)ds_rate(b), (long)b->vol, (long)b->pan, b->loop, (unsigned long)b->pos0);
    return 0;
}
static HRESULT WINAPI dsb_setpos(DsBuf *b, DWORD pos) { b->pos0 = pos; if (b->playing) { b->t0 = GetTickCount(); slog("dpos %d %lu", b->id, (unsigned long)pos); } return 0; }
static HRESULT WINAPI dsb_setformat(DsBuf *b, WFX *w) { (void)b; (void)w; return 0; }
static HRESULT WINAPI dsb_setvol(DsBuf *b, LONG v) { if (v != b->vol && b->playing) slog("dvol %d %ld", b->id, (long)v); b->vol = v; return 0; }
static HRESULT WINAPI dsb_setpan(DsBuf *b, LONG v) { if (v != b->pan && b->playing) slog("dpan %d %ld", b->id, (long)v); b->pan = v; return 0; }
static HRESULT WINAPI dsb_setfreq(DsBuf *b, DWORD v) {
    if (b->playing) { b->pos0 = ds_played(b); b->t0 = GetTickCount(); if (v != b->freq) slog("dfreq %d %lu", b->id, (unsigned long)(v ? v : b->wfx.rate)); }
    b->freq = v; return 0;
}
static HRESULT WINAPI dsb_stop(DsBuf *b) {
    if (b->playing) { slog("dstop %d", b->id); b->pos0 = b->d && b->d->n ? ds_played(b) % b->d->n : 0; }
    b->playing = 0; return 0;
}
static HRESULT WINAPI dsb_unlock(DsBuf *b, void *p1, DWORD n1, void *p2, DWORD n2) { (void)p1; (void)n1; (void)p2; (void)n2; if (b->d) b->d->dirty = 1; return 0; }
static HRESULT WINAPI dsb_restore(DsBuf *b) { (void)b; return 0; }
static void *g_dsb_vt[21] = { dsb_qi, dsb_addref, dsb_release, dsb_getcaps, dsb_getpos, dsb_getformat, dsb_getvol, dsb_getpan,
    dsb_getfreq, dsb_getstatus, dsb_init, dsb_lock, dsb_play, dsb_setpos, dsb_setformat, dsb_setvol, dsb_setpan, dsb_setfreq,
    dsb_stop, dsb_unlock, dsb_restore };
static DsBuf *dsb_new(void) { DsBuf *b = calloc(1, sizeof *b); b->vt = g_dsb_vt; b->refs = 1; b->id = ++g_ds_ids; return b; }
static HRESULT WINAPI ds_qi(DsObj *o, const void *iid, void **out) { (void)iid; *out = o; o->refs++; return 0; }
static ULONG WINAPI ds_addref(DsObj *o) { return ++o->refs; }
static ULONG WINAPI ds_release(DsObj *o) { return --o->refs; }
static HRESULT WINAPI ds_create(DsObj *o, DSBD *d, DsBuf **out, void *unk) {
    (void)o; (void)unk;
    DsBuf *b = dsb_new();
    if (d->flags & 1) b->primary = 1;                       // DSBCAPS_PRIMARYBUFFER
    else {
        if (d->wfx) memcpy(&b->wfx, d->wfx, 16 + (d->wfx->fmt == 1 ? 0 : 2));
        b->d = calloc(1, sizeof(DsData)); b->d->refs = 1; b->d->n = d->bytes; b->d->p = calloc(1, d->bytes ? d->bytes : 1);
    }
    *out = b; return 0;
}
static HRESULT WINAPI ds_getcaps(DsObj *o, DWORD *c) { (void)o; memset(c + 1, 0, (c[0] > 4 ? c[0] - 4 : 0)); return 0; }
static HRESULT WINAPI ds_dup(DsObj *o, DsBuf *src, DsBuf **out) {
    (void)o;
    DsBuf *b = dsb_new(); b->wfx = src->wfx; b->d = src->d; if (b->d) b->d->refs++;
    b->vol = src->vol; b->pan = src->pan; b->freq = src->freq;
    *out = b; return 0;
}
static HRESULT WINAPI ds_coop(DsObj *o, HWND w, DWORD l) { (void)o; (void)w; (void)l; return 0; }
static HRESULT WINAPI ds_ok0(DsObj *o) { (void)o; return 0; }
static HRESULT WINAPI ds_ok1(DsObj *o, void *a) { (void)o; (void)a; return 0; }
static void *g_ds_vt[11] = { ds_qi, ds_addref, ds_release, ds_create, ds_getcaps, ds_dup, ds_coop, ds_ok0, ds_ok1, ds_ok1, ds_ok1 };
static HRESULT WINAPI fake_DirectSoundCreate(const void *guid, DsObj **out, void *unk) {
    (void)guid; (void)unk;
    DsObj *o = calloc(1, sizeof *o); o->vt = g_ds_vt; o->refs = 1; *out = o;
    slog("dsound fake"); return 0;
}
static FARPROC (WINAPI *real_GetProcAddress)(HMODULE, LPCSTR);
static FARPROC WINAPI my_GetProcAddress(HMODULE m, LPCSTR n) {
    if ((uintptr_t)n > 0xffff && strcmp(n, "DirectSoundCreate") == 0 && snd_on()) return (FARPROC)fake_DirectSoundCreate;
    return real_GetProcAddress(m, n);
}

// --- IAT hooks ---
static BOOL (WINAPI *real_PeekMessageA)(LPMSG, HWND, UINT, UINT, UINT);
static DWORD (WINAPI *real_timeGetTime)(void);
static DWORD (WINAPI *real_GetTickCount)(void);
static BOOL (WINAPI *real_QPC)(LARGE_INTEGER *);
static void (WINAPI *real_Sleep)(DWORD);
static DWORD g_t0_tgt, g_t0_gtc; static LONGLONG g_t0_qpc;
// Virtual clock: time the harness spends blocked (waiting for an interactive command) is removed from
// every game clock, so the game does not run a burst of catch-up updates when it resumes.
static LONGLONG g_qpf = 1;

static BOOL WINAPI my_PeekMessageA(LPMSG m, HWND h, UINT a, UINT b, UINT r) {
    if (GetCurrentThreadId() == g_main_tid && g_driving) {
        // the app only updates while its window has focus (app+0x4ce, set from WM_ACTIVATE/GetActiveWindow;
        // FUN_00529b00 derives the active flag +0x4d2): a click on another window would pause the run, so keep
        // it set while a script runs
        uint8_t *app = APP();
        if (app && !app[0x4ce]) { app[0x4ce] = 1; ((update_fn)0x529b00)(app); }
    }
    if (GetCurrentThreadId() == g_main_tid && (g_script || g_hstate)) harness_step();
    BOOL ok = real_PeekMessageA(m, h, a, b, r);
    // the script drives the game through direct calls: drop real mouse input while one runs, so a mouse moved
    // over the window can't turn the launcher (e.g. during the aim-assist pause) and video cursor glides
    // (SetCursorPos) don't reach the game
    if (ok && g_driving && m && ((m->message >= WM_MOUSEFIRST && m->message <= WM_MOUSELAST) || m->message == WM_NCMOUSEMOVE
                                || m->message == WM_MOUSELEAVE)) m->message = WM_NULL;
    return ok;
}
static volatile LONGLONG g_pause_qpc; static volatile DWORD g_pause_ms;
static DWORD WINAPI my_timeGetTime(void) { DWORD t = real_timeGetTime(); return g_t0_tgt + (DWORD)((t - g_t0_tgt - g_pause_ms) * g_speed); }
static DWORD WINAPI my_GetTickCount(void) { DWORD t = real_GetTickCount(); return g_t0_gtc + (DWORD)((t - g_t0_gtc - g_pause_ms) * g_speed); }
static BOOL WINAPI my_QPC(LARGE_INTEGER *p) {
    BOOL r = real_QPC(p); p->QuadPart = g_t0_qpc + (LONGLONG)((p->QuadPart - g_t0_qpc - g_pause_qpc) * g_speed); return r;
}
static LONGLONG g_block_q0;
static void clock_block_begin(void) { LARGE_INTEGER q; QueryPerformanceCounter(&q); g_block_q0 = q.QuadPart; }
static void clock_block_end(void) {
    LARGE_INTEGER q; QueryPerformanceCounter(&q);
    LONGLONG d = q.QuadPart - g_block_q0;
    g_pause_qpc += d; g_pause_ms = (DWORD)(g_pause_qpc * 1000 / g_qpf);
}
static void WINAPI my_Sleep(DWORD ms) { real_Sleep(g_speed > 1.0 ? (DWORD)(ms / g_speed) : ms); }

static void *iat_patch(uint32_t slot, void *fn) {
    void **p = (void **)slot; void *old = *p; DWORD o;
    VirtualProtect(p, 4, PAGE_READWRITE, &o); *p = fn; VirtualProtect(p, 4, o, &o);
    return old;
}

static void harness_init(FILE *info) {
    g_main_tid = GetCurrentThreadId();
    const char *sp = getenv("PEGGLE_SPEED"); if (sp) g_speed = atof(sp);
    const char *sc = getenv("PEGGLE_SCRIPT");
    if (sc) { g_driving = 1; g_script = fopen(sc, "r"); g_hlog = fopen(getenv("PEGGLE_HLOG") ? getenv("PEGGLE_HLOG") : "peggle_harness.txt", "w"); }
    if (g_script || g_speed != 1.0) {
        real_PeekMessageA = iat_patch(0x5d23c4, (void *)my_PeekMessageA);
        real_GetProcAddress = iat_patch(0x5d206c, (void *)my_GetProcAddress);
        {   // SexyApp screensaver idle check FUN_005293c0: if (now - last input) > SPI screensaver timeout - 2 min
            // it sets DAT_006873c0 = 1, and UpdateAppStep FUN_0052b1f0 then returns without updating until real
            // input arrives. The harness never sends real input (and the sped-up virtual clock runs ahead of
            // the system's last-input time), so long runs froze for good. NOP the `movb $1, 0x6873c0` at 0x5294d9.
            static const uint8_t SS_SIG[7] = {0xc6, 0x05, 0xc0, 0x73, 0x68, 0x00, 0x01};
            uint8_t *q = (uint8_t *)0x5294d9;
            if (memcmp(q, SS_SIG, 7) == 0) {
                DWORD o; VirtualProtect(q, 7, PAGE_EXECUTE_READWRITE, &o); memset(q, 0x90, 7); VirtualProtect(q, 7, o, &o);
                if (info) fprintf(info, "harness: screensaver idle check disabled\n");
            } else if (info) fprintf(info, "harness: screensaver patch signature mismatch\n");
        }
        { LARGE_INTEGER f; QueryPerformanceFrequency(&f); g_qpf = f.QuadPart ? f.QuadPart : 1; }
        {   // always with a script: the virtual clock also hides harness pauses
            real_timeGetTime = iat_patch(0x5d23e8, (void *)my_timeGetTime);
            real_GetTickCount = iat_patch(0x5d20cc, (void *)my_GetTickCount);
            real_QPC = iat_patch(0x5d2150, (void *)my_QPC);
            real_Sleep = iat_patch(0x5d20d8, (void *)my_Sleep);
            g_t0_tgt = real_timeGetTime(); g_t0_gtc = real_GetTickCount();
            LARGE_INTEGER q; real_QPC(&q); g_t0_qpc = q.QuadPart;
        }
        if (info) fprintf(info, "harness: script %s speed %g peek %p\n", sc ? sc : "-", g_speed, (void *)real_PeekMessageA);
    }
}

static const uint8_t UPDATE_SIG[6] = {0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x18};
static const uint8_t COLLIDE_SIG[5] = {0x55, 0x8B, 0xEC, 0x6A, 0xFF};
static const uint8_t PHYSUPD_SIG[6] = {0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x08};
static const uint8_t FIRE_SIG[5] = {0x55, 0x8B, 0xEC, 0x6A, 0xFF};

BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID r) {
    if (reason == DLL_PROCESS_ATTACH) {
        const char *path = getenv("PEGGLE_LOG");
        g_log = fopen(path ? path : "peggle_oracle.bin", "wb");
        char ip[300]; sprintf(ip, "%s\\peggle_oracle_info.txt", out_dir()); FILE *info = fopen(ip, "w");
        if (memcmp((void *)0x47a450, UPDATE_SIG, 6) == 0 && memcmp((void *)0x454f90, COLLIDE_SIG, 5) == 0) {
            g_tramp_update = install(0x47a450, (void *)hook_update, 6);
            g_tramp_collide = install(0x454f90, (void *)hook_collide, 5);
            if (memcmp((void *)0x484530, FIRE_SIG, 5) == 0) g_tramp_fire = install(0x484530, (void *)hook_fire, 5);
            if (memcmp((void *)0x479a30, PHYSUPD_SIG, 6) == 0) g_tramp_physupd = install(0x479a30, (void *)hook_physupd, 6);
            static const uint8_t BOUNCE_SIG[6] = {0x55, 0x8b, 0xec, 0x83, 0xec, 0x10};
            if (memcmp((void *)0x4775f0, BOUNCE_SIG, 6) == 0) g_tramp_bounce = install(0x4775f0, (void *)hook_bounce, 6);
            static const uint8_t PCL_SIG[6] = {0x55, 0x8b, 0xec, 0x83, 0xec, 0x40};
            g_trace_ccd = getenv("PEGGLE_TRACE_CCD") != 0;
            if (g_trace_ccd && memcmp((void *)0x47dfe0, PCL_SIG, 6) == 0) g_tramp_pcl = install(0x47dfe0, (void *)hook_pcl, 6);
            static const uint8_t DCT[5] = {0x55, 0x8B, 0xEC, 0x6A, 0xFF}, DDT[2] = {0xC7, 0x01};
            if (memcmp((void *)0x497280, DCT, 5) == 0) g_tramp_dlg_ctor = install(0x497280, (void *)hook_dlg_ctor, 5);
            if (memcmp((void *)0x497550, DDT, 2) == 0) g_tramp_dlg_dtor = install(0x497550, (void *)hook_dlg_dtor, 6);
            if (memcmp((void *)0x46b100, DCT, 5) == 0) g_tramp_style = install(0x46b100, (void *)hook_style, 5);
            static const uint8_t LSH_SIG[6] = {0x55, 0x8b, 0xec, 0x83, 0xec, 0x14};
            if (memcmp((void *)0x46b910, LSH_SIG, 6) == 0) g_tramp_lsh = install(0x46b910, (void *)hook_lsh, 6);
            if (getenv("PEGGLE_BHPLOG") && memcmp((void *)0x46f480, DCT, 5) == 0) g_tramp_bhp = install(0x46f480, (void *)hook_bhp, 5);
            static const uint8_t SPK_SIG[6] = {0x55, 0x8b, 0xec, 0x83, 0xec, 0x10};
            if (getenv("PEGGLE_BHPLOG") && memcmp((void *)0x451e10, SPK_SIG, 6) == 0) g_tramp_spk = install(0x451e10, (void *)hook_spk, 6);
            g_tubelog = getenv("PEGGLE_TUBELOG") != 0;
            { static const uint8_t TUB_SIG[6] = {0x55, 0x8b, 0xec, 0x83, 0xec, 0x2c};
              if (g_tubelog && memcmp((void *)0x443cf0, TUB_SIG, 6) == 0) g_tramp_tube = install(0x443cf0, (void *)hook_tube, 6); }
            static const uint8_t AIS_SIG[6] = {0x55, 0x8b, 0xec, 0x83, 0xec, 0x0c};
            if (getenv("PEGGLE_AILOG") && memcmp((void *)0x460380, AIS_SIG, 6) == 0) g_tramp_aistep = install(0x460380, (void *)hook_aistep, 6);
            if (info) fprintf(info, "hooks installed\n");
        } else if (info) fprintf(info, "signature mismatch; not hooking (wrong exe?)\n");
        harness_init(info);
        // the DSoundManager (app+0x4b4) is created before the game loads bass.dll; with no audio device its
        // IDirectSound (+0x3094) is null. Give it the logging stand-in (and a primary buffer, +0x2004).
        if (snd_on()) {
            uint8_t *app = APP(), *mgr = app ? *(uint8_t **)(app + 0x4b4) : 0;
            if (mgr && !*(void **)(mgr + 0x3094)) {
                DsObj *o; fake_DirectSoundCreate(0, &o, 0);
                *(void **)(mgr + 0x3094) = o;
                DSBD d = { sizeof(DSBD), 1, 0, 0, 0 }; DsBuf *pb; ds_create(o, &d, &pb, 0);
                *(void **)(mgr + 0x2004) = pb;
                if (info) fprintf(info, "fake dsound installed in manager %p\n", (void *)mgr);
            } else if (info) fprintf(info, "fake dsound: manager %p ds %p\n", (void *)mgr, mgr ? *(void **)(mgr + 0x3094) : 0);
        }
        tas_init(info);
        frames_init(info);
        static const uint8_t AIM_SIG[6] = {0x55, 0x8b, 0xec, 0x83, 0xec, 0x14};
        if (memcmp((void *)0x466f10, AIM_SIG, 6) == 0) g_tramp_aimupd = install(0x466f10, (void *)hook_aimupd, 6);
        if (info) fclose(info);
    } else if (reason == DLL_PROCESS_DETACH) {
        if (g_log) fclose(g_log);
    }
    return TRUE;
}
