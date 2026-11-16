//! Peggle level (.dat) parser.
//!
//! The format is SexyApp `DataSync` serialization (read mode). Every function here mirrors a
//! `Sync()` method in the game executable; addresses are noted so the parser can be re-checked against it.
//!
//! Encoding rules (DataSync, FUN_004bbf70 & friends):
//! * bools are bit-packed LSB-first; a new byte is fetched when the bit cursor is 0, the cursor
//!   wraps after 8 bits, and *any* non-bool read or an explicit flush resets the cursor to 0.
//! * i32/f32 little-endian 4 bytes, i16/u16 2 bytes, i8/u8 1 byte, strings = u16 len + bytes.
//! * object pointers: u32 tag (0 = null, 1 = new object follows, n>=2 = back-reference to the
//!   (n-3)th registered object — empirically IDs start at 3, ID 2 is presumably the root list).
//!   Objects (PhysObjs and Movers share one table) are registered before their body is read.

use serde::Serialize;
use std::fmt;

#[derive(Debug)]
pub struct ParseError(pub String);
impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ParseError {}
type R<T> = Result<T, ParseError>;

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
    bit: u32,
    bitbyte: u8,
    version: i32,
    /// registered objects, in order (for back-references)
    reg: Vec<RegRef>,
}

#[derive(Clone, Copy, Debug)]
enum RegRef {
    Obj(usize),
    Mover(usize),
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> R<&'a [u8]> {
        self.bit = 0;
        if self.p + n > self.d.len() {
            return Err(ParseError(format!("unexpected EOF at {:#x} reading {n}", self.p)));
        }
        let s = &self.d[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> R<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn i8(&mut self) -> R<i8> {
        Ok(self.u8()? as i8)
    }
    fn u16(&mut self) -> R<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn i16(&mut self) -> R<i16> {
        Ok(self.u16()? as i16)
    }
    fn i32(&mut self) -> R<i32> {
        let b = self.bytes(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn f32(&mut self) -> R<f32> {
        Ok(f32::from_bits(self.i32()? as u32))
    }
    fn string(&mut self) -> R<String> {
        let n = self.u16()? as usize;
        Ok(String::from_utf8_lossy(self.bytes(n)?).into_owned())
    }
    /// FUN_004bc290
    fn bool(&mut self) -> R<bool> {
        if self.bit == 0 {
            self.bitbyte = self.u8()?;
        }
        let v = self.bitbyte & (1 << self.bit) != 0;
        self.bit += 1;
        if self.bit > 7 {
            self.bit = 0;
        }
        Ok(v)
    }
    fn bools<const N: usize>(&mut self) -> R<[bool; N]> {
        let mut out = [false; N];
        for b in out.iter_mut() {
            *b = self.bool()?;
        }
        Ok(out)
    }
    /// FUN_004bbeb0
    fn flush(&mut self) {
        self.bit = 0;
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize)]
pub struct PegInfo {
    /// +0x10. 1 = normal (blue/variable) peg, 2 = orange (fixed goal peg?), ... (TBD)
    pub peg_type: u8,
    pub b14: bool,
    /// +0x15 peg_variable: eligible to be randomly chosen as orange/purple/green
    pub variable: bool,
    /// +0x16 peg_crumble
    pub crumble: bool,
    pub b17: bool,
    pub i18: i32,
    pub i20: i32,
    pub u24: u8,
    pub u1c: u8,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Mover {
    /// +0x10 mover_type (signed byte)
    pub mover_type: i32,
    /// +0x5c/+0x60 center
    pub cx: f32,
    pub cy: f32,
    /// +0x1c mover_time? (u16)
    pub time: u16,
    pub s14: i32,
    pub s18: i32,
    pub f20: f32,
    pub f24: f32,
    pub s28: i32,
    pub u2c: u16,
    pub u34: u16,
    pub b30: i32,
    pub b38: i32,
    pub f40: f32,
    pub f3c: f32,
    pub f64: f32,
    pub f44: f32,
    /// nested sub-mover (+0x48) with its own offset +0x4c/+0x50
    pub sub: Option<(f32, f32, MoverRef)>,
    /// +0x54/+0x58 (defaults to center)
    pub px: f32,
    pub py: f32,
}

/// Index into `Level::movers`.
pub type MoverRef = usize;
/// Index into `Level::objects`.
pub type ObjRef = usize;

/// Fields common to every PhysObj (FUN_00480750).
#[derive(Debug, Clone, Default, Serialize)]
pub struct PhysObj {
    pub collidable: bool,      // +0x24
    pub visible: bool,         // +0x25
    pub moving: bool,          // +0x26
    pub background: bool,      // +0x27
    pub foreground: bool,      // +0x28
    pub foreground2: bool,     // +0x29
    pub baseobj: bool,         // +0x2a
    pub ballstopreset: bool,   // +0x2b
    pub drawsort: bool,        // +0x2c
    pub drawfloat: bool,       // +0x2d
    pub b30: bool,             // +0x30
    pub rolly: f32,            // +0x38
    pub bouncy: f32,           // +0x34
    pub maxbouncevel: f32,     // +0x3c
    pub i4c: i32,              // +0x4c
    pub solidcolor: i32,       // +0x40
    pub outlinecolor: i32,     // +0x44 (default 0xffffff)
    pub image: String,         // +0x94
    pub imagedx: f32,          // +0xb8
    pub imagedy: f32,          // +0xbc
    pub imagerot: f32,         // +0xc0
    pub ic8: i32,              // +0xc8
    pub id: String,            // +0x5c
    pub i50: i32,              // +0x50
    pub sound: u8,             // +0x54
    pub logic: String,         // +0x78
    pub subid: i32,            // +0xb0
    pub flipperflags: u8,      // +0xcc
    pub peg: Option<PegInfo>,  // +0xd0
    pub mover: Option<MoverRef>, // +0x48
}

#[derive(Debug, Clone, Serialize)]
pub enum Shape {
    /// entry type 1 (or unknown): bare PhysObj
    Generic,
    /// entry type 2: Line/rod (FUN_004814f0)
    Line { x1: f32, y1: f32, x2: f32, y2: f32, f_fc: f32, f_100: f32, b111: bool },
    /// entry type 3: Poly (FUN_00487d00 with vertices)
    Poly(PolyData),
    /// entry type 4/5: Ball. `peg` = type 5 circle peg (internal type 3); else a real ball (type 2)
    Circle(CircleData),
    /// entry type 6: Brick (curved/straight peg-brick)
    Brick { poly: PolyData, brick: BrickData },
    /// entry type 7: Gun (Poly subclass)
    Gun { poly: PolyData, gun: GunData },
    /// entry type 8: Hole / teleport
    Hole(HoleData),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PolyData {
    pub b12c: bool,
    pub convex: bool,     // +0x12d? (editor: poly_convex)
    pub imagetile: bool,  // +0x12f
    pub imagemirror: bool, // +0x130
    pub rotation: f32,    // +0x11c (radians?)
    pub f120: f32,
    pub scale: f32,       // +0x124 (default 1.0)
    pub normaldir: i32,   // +0x134 (default 1)
    pub x: f32,           // +0x114
    pub y: f32,           // +0x118
    pub verts: Vec<(f32, f32)>, // +0xe4 (only stored for plain Polys in level files)
    pub growtype: u8,     // +0x138 (ver>0x22)
    pub i13c: i32,
    pub u140: u16,
    pub b144: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct BrickData {
    pub rightangle_flag: bool, // +0x181
    pub leftangle_flag: bool,  // +0x182
    pub texflip: bool,         // +0x180
    pub f160: f32,
    pub f164: f32,
    pub brick_type: u8,        // +0x16c (default 0; 5 = ?)
    pub subdiv: u8,            // +0x168 (default 2)
    pub leftangle: f32,        // +0x150
    pub rightangle: f32,       // +0x154
    pub angle: f32,            // +0x14c
    pub thickness: f32,        // +0x158 (default DAT_005d7278)
    pub radius: f32,           // +0x148
    pub rotation: f32,         // +0x15c
    pub u170: [u8; 4],
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CircleData {
    pub peg: bool,
    pub b140: bool,
    pub b160: bool,
    pub b163: bool,
    pub b161: bool,
    pub x: f32,      // +0xec
    pub y: f32,      // +0xf0
    pub radius: f32, // +0x13c
    /// Extra state only present for real balls (internal type 2)
    pub ball_extra: Option<BallExtra>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct BallExtra {
    pub f: [f32; 6],
    pub ie4: i32,
    pub f10c: f32,
    pub f110: f32,
    pub pts: [(f32, f32); 4],
    pub i148: i32,
    pub i14c: i32,
    pub sub: Option<ObjRef>,
    pub i150: i32,
    pub i158: i32,
    pub i154: i32,
    pub i178: i32,
    pub i17c: i32,
    pub i180: i32,
    pub other_ball: Option<ObjRef>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct GunData {
    pub b194: bool,
    pub b195: bool,
    pub b1a1: bool,
    pub b1a2: bool,
    pub i14c: i32,
    pub f1b0: [f32; 4],
    pub f150: f32,
    pub ball: Option<ObjRef>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct HoleData {
    pub b10c: bool,
    pub i104: i32,
    pub i108: i32,
    pub u0ec: u16,
    pub i0f0: i32,
    pub i0e8: i32,
    pub target: Option<ObjRef>,
    pub x: f32,
    pub y: f32,
    pub f0fc: f32,
    pub f100: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Object {
    /// entry type from the file (1..8)
    pub entry_type: i32,
    pub phys: PhysObj,
    pub shape: Shape,
}

#[derive(Debug, Clone, Serialize)]
pub struct Level {
    pub version: i32,
    pub is_level: bool,
    pub objects: Vec<Object>,
    pub movers: Vec<Mover>,
    /// top-level object list (references into `objects`); normally 0..n in order
    pub top: Vec<Option<ObjRef>>,
}

pub fn parse(data: &[u8]) -> Result<Level, ParseError> {
    let mut r = Reader { d: data, p: 0, bit: 0, bitbyte: 0, version: 0, reg: vec![] };
    // FUN_004227c0
    let version = r.i32()?;
    if version > 0x42 {
        return Err(ParseError(format!("version {version:#x} too new")));
    }
    r.version = version;
    let is_level = r.u8()? != 0; // DAT_00650a5c = 1
    if !is_level {
        return Err(ParseError("not in level mode (DAT_00650a5c != 1); unsupported".into()));
    }
    let mut lvl = Level { version, is_level, objects: vec![], movers: vec![], top: vec![] };
    // FUN_004204c0: list<PhysObj*>
    let count = r.i32()?;
    for _ in 0..count {
        let o = read_obj_ptr(&mut r, &mut lvl, None)?;
        lvl.top.push(o);
    }
    if r.p != data.len() {
        return Err(ParseError(format!("trailing bytes: parsed {:#x} of {:#x}", r.p, data.len())));
    }
    Ok(lvl)
}

/// FUN_004067c0 (generic PhysObj*) / FUN_00481850 (Ball*, forced_type = Some(4))
fn read_obj_ptr(r: &mut Reader, lvl: &mut Level, forced_type: Option<i32>) -> R<Option<ObjRef>> {
    let tag = r.i32()?;
    match tag {
        0 => Ok(None),
        1 => {
            let ty = match forced_type {
                Some(t) => t,
                None => r.i32()?,
            };
            let idx = lvl.objects.len();
            lvl.objects.push(Object { entry_type: ty, phys: PhysObj::default(), shape: Shape::Generic });
            r.reg.push(RegRef::Obj(idx));
            let (phys, shape) = read_obj_body(r, lvl, ty)?;
            lvl.objects[idx].phys = phys;
            lvl.objects[idx].shape = shape;
            Ok(Some(idx))
        }
        n => match r.reg.get((n - 3) as usize) {
            Some(RegRef::Obj(i)) => Ok(Some(*i)),
            other => Err(ParseError(format!("bad object back-ref {n}: {other:?}"))),
        },
    }
}

fn read_obj_body(r: &mut Reader, lvl: &mut Level, ty: i32) -> R<(PhysObj, Shape)> {
    let phys = read_physobj(r, lvl)?;
    let shape = match ty {
        2 => read_line(r)?,
        3 => Shape::Poly(read_poly(r, &phys, lvl, true)?),
        4 | 5 => Shape::Circle(read_ball(r, lvl, &phys, ty == 5)?),
        6 => {
            let poly = read_poly(r, &phys, lvl, false)?;
            Shape::Brick { poly, brick: read_brick(r)? }
        }
        7 => {
            let poly = read_poly(r, &phys, lvl, false)?;
            Shape::Gun { poly, gun: read_gun(r, lvl)? }
        }
        8 => Shape::Hole(read_hole(r, lvl, &phys)?),
        _ => Shape::Generic,
    };
    Ok((phys, shape))
}

/// FUN_00480750 PhysObj::Sync
fn read_physobj(r: &mut Reader, lvl: &mut Level) -> R<PhysObj> {
    let mut o = PhysObj { outlinecolor: 0xffffff, ..Default::default() };
    let [has_rolly, has_bouncy, has_peg, has_mover, has_4c, collidable, visible, moving] = r.bools()?;
    r.flush();
    let [has_solid, has_outline, has_image, has_idx, has_idy, has_irot, background, baseobj] = r.bools()?;
    r.flush();
    let [has_c8, has_id, has_50, has_sound, ballstopreset, has_logic, foreground, has_maxbv] = r.bools()?;
    r.flush();
    let (mut has_subid, mut has_flipper) = (false, false);
    if r.version > 0xe {
        let [drawsort, fg2, hs, hf, drawfloat, b30] = r.bools()?;
        r.flush();
        o.drawsort = drawsort;
        o.foreground2 = fg2;
        o.drawfloat = drawfloat;
        o.b30 = b30;
        has_subid = hs;
        has_flipper = hf;
    }
    o.collidable = collidable;
    o.visible = visible;
    o.moving = moving;
    o.background = background;
    o.baseobj = baseobj;
    o.ballstopreset = ballstopreset;
    o.foreground = foreground;
    if has_rolly { o.rolly = r.f32()?; }
    if has_bouncy { o.bouncy = r.f32()?; }
    if has_4c { o.i4c = r.i32()?; }
    if has_solid { o.solidcolor = r.i32()?; }
    if has_outline { o.outlinecolor = r.i32()?; }
    if has_image { o.image = r.string()?; }
    if has_idx { o.imagedx = r.f32()?; }
    if has_idy { o.imagedy = r.f32()?; }
    if has_irot { o.imagerot = r.f32()?; }
    if has_c8 { o.ic8 = r.i32()?; }
    if has_id { o.id = r.string()?; }
    if has_50 { o.i50 = r.i32()?; }
    if has_sound { o.sound = r.u8()?; }
    if has_logic { o.logic = r.string()?; }
    if has_maxbv { o.maxbouncevel = r.f32()?; }
    if has_subid { o.subid = r.i32()?; }
    if has_flipper { o.flipperflags = r.u8()?; }
    if has_peg {
        o.peg = Some(read_peginfo(r)?);
    }
    if has_mover {
        o.mover = read_mover_ptr(r, lvl)?;
    }
    Ok(o)
}

/// FUN_00479560 PegInfo::Sync
fn read_peginfo(r: &mut Reader) -> R<PegInfo> {
    let mut p = PegInfo { peg_type: r.u8()?, ..Default::default() };
    let [b14, variable, has18, crumble, has20, has24, b17, has1c] = r.bools()?;
    r.flush();
    p.b14 = b14;
    p.variable = variable;
    p.crumble = crumble;
    p.b17 = b17;
    if has18 { p.i18 = r.i32()?; }
    if has20 { p.i20 = r.i32()?; }
    if has24 { p.u24 = r.u8()?; }
    if has1c { p.u1c = r.u8()?; }
    Ok(p)
}

/// FUN_00406860 (Mover* pointer)
fn read_mover_ptr(r: &mut Reader, lvl: &mut Level) -> R<Option<MoverRef>> {
    let tag = r.i32()?;
    match tag {
        0 => Ok(None),
        1 => {
            let idx = lvl.movers.len();
            lvl.movers.push(Mover::default());
            r.reg.push(RegRef::Mover(idx));
            let m = read_mover(r, lvl)?;
            lvl.movers[idx] = m;
            Ok(Some(idx))
        }
        n => match r.reg.get((n - 3) as usize) {
            Some(RegRef::Mover(i)) => Ok(Some(*i)),
            other => Err(ParseError(format!("bad mover back-ref {n}: {other:?}"))),
        },
    }
}

/// FUN_0047a9f0 Mover::Sync
fn read_mover(r: &mut Reader, lvl: &mut Level) -> R<Mover> {
    let mut m = Mover { mover_type: r.i8()? as i32, ..Default::default() };
    m.cx = r.f32()?;
    m.cy = r.f32()?;
    m.time = r.u16()?;
    let [h14, h18, h20, h24, h28, h2c, h34, h30] = r.bools()?;
    r.flush();
    let [h38, h40, h3c, h64, h48, h54, h44] = r.bools()?;
    r.flush();
    if h14 { m.s14 = r.i16()? as i32; }
    if h18 { m.s18 = r.i16()? as i32; }
    if h20 { m.f20 = r.f32()?; }
    if h24 { m.f24 = r.f32()?; }
    if h28 { m.s28 = r.i16()? as i32; }
    if h2c { m.u2c = r.u16()?; }
    if h34 { m.u34 = r.u16()?; }
    if h30 { m.b30 = r.i8()? as i32; }
    if h38 { m.b38 = r.i8()? as i32; }
    if h40 { m.f40 = r.f32()?; }
    if h3c { m.f3c = r.f32()?; }
    if h64 { m.f64 = r.f32()?; }
    if h44 { m.f44 = r.f32()?; }
    if h48 {
        let ox = r.f32()?;
        let oy = r.f32()?;
        if let Some(sub) = read_mover_ptr(r, lvl)? {
            m.sub = Some((ox, oy, sub));
        }
    }
    if h54 {
        m.px = r.f32()?;
        m.py = r.f32()?;
    } else {
        m.px = m.cx;
        m.py = m.cy;
    }
    Ok(m)
}

/// FUN_004814f0 Line::Sync (level mode)
fn read_line(r: &mut Reader) -> R<Shape> {
    let [h_fc, h_100, b111] = r.bools()?;
    r.flush();
    // Read order (+0xec, +0xf4, +0xf0, +0xf8); built via FUN_004782f0(x1, y1, x2, y2) which stores
    // x1→0xec, x2→0xf0, y1→0xf4, y2→0xf8.
    let x1 = r.f32()?;
    let y1 = r.f32()?;
    let x2 = r.f32()?;
    let y2 = r.f32()?;
    let f_fc = if h_fc { r.f32()? } else { 0.0 };
    let f_100 = if h_100 { r.f32()? } else { 0.0 };
    Ok(Shape::Line { x1, y1, x2, y2, f_fc, f_100, b111 })
}

/// FUN_00487d00 Poly::Sync. `with_verts` = its 3rd argument.
fn read_poly(r: &mut Reader, phys: &PhysObj, lvl: &Level, with_verts: bool) -> R<PolyData> {
    let has_mover = phys.mover.is_some();
    let mut p = PolyData { scale: 1.0, normaldir: 1, ..Default::default() };
    let [b12c, has134, has11c, has120, no_mover, has124, b12f, b130] = r.bools()?;
    r.flush();
    p.b12c = b12c;
    p.imagetile = b12f;
    p.imagemirror = b130;
    let (mut has138, mut has13c, mut has140) = (false, false, false);
    if r.version > 0x22 {
        let [a, b, c, b144] = r.bools()?;
        r.flush();
        has138 = a;
        has13c = b;
        has140 = c;
        p.b144 = b144;
    }
    if has11c { p.rotation = r.f32()?; }
    if has120 { p.f120 = r.f32()?; }
    if has124 { p.scale = r.f32()?; }
    if has134 { p.normaldir = r.i8()? as i32; }
    if no_mover {
        p.x = r.f32()?;
        p.y = r.f32()?;
    }
    if has_mover {
        // position/rotation come from the mover (+0x54,+0x58,+0x64)
        let m = &lvl.movers[phys.mover.unwrap()];
        p.x = m.px;
        p.y = m.py;
        p.rotation = m.f64;
    }
    let _ = no_mover;
    if with_verts {
        // FUN_00481950
        let n = r.i32()?;
        if n > 100000 {
            return Err(ParseError(format!("bad vertex count {n}")));
        }
        for _ in 0..n {
            let x = r.f32()?;
            let y = r.f32()?;
            p.verts.push((x, y));
        }
    }
    if has138 { p.growtype = r.u8()?; }
    if has13c { p.i13c = r.i32()?; }
    if has140 { p.u140 = r.u16()?; }
    Ok(p)
}

/// FUN_00488030 Brick::Sync (after Poly::Sync without vertices)
fn read_brick(r: &mut Reader) -> R<BrickData> {
    let mut b = BrickData { subdiv: 2, ..Default::default() };
    let [b181, b182, has16c, has168, has14c, has150, has154, has158] = r.bools()?;
    r.flush();
    let [has160, has164, b180] = r.bools()?;
    r.flush();
    b.rightangle_flag = b181;
    b.leftangle_flag = b182;
    b.texflip = b180;
    if has160 { b.f160 = r.f32()?; }
    if has164 { b.f164 = r.f32()?; }
    if has16c { b.brick_type = r.u8()?; }
    if has168 { b.subdiv = r.u8()?; }
    if has150 { b.leftangle = r.f32()?; }
    if has154 {
        // The game really reads this field twice (FUN_00488030).
        b.rightangle = r.f32()?;
        b.rightangle = r.f32()?;
    }
    if has14c { b.angle = r.f32()?; }
    b.thickness = if has158 { r.f32()? } else { 20.0 /* DAT_005d7278 */ };
    b.radius = r.f32()?;
    b.rotation = r.f32()?;
    for i in 0..4 {
        b.u170[i] = r.u8()?;
    }
    Ok(b)
}

/// FUN_004882c0 Gun::Sync
fn read_gun(r: &mut Reader, lvl: &mut Level) -> R<GunData> {
    let mut g = GunData::default();
    let [h14c, b194, h1b0, b195, b1a1, b1a2] = r.bools()?;
    r.flush();
    g.b194 = b194;
    g.b195 = b195;
    g.b1a1 = b1a1;
    g.b1a2 = b1a2;
    if h14c { g.i14c = r.i32()?; }
    if h1b0 {
        for i in 0..4 {
            g.f1b0[i] = r.f32()?;
        }
    }
    g.f150 = r.f32()?;
    g.ball = read_obj_ptr(r, lvl, Some(4))?;
    Ok(g)
}

/// FUN_00483410 Ball::Sync
fn read_ball(r: &mut Reader, lvl: &mut Level, phys: &PhysObj, peg: bool) -> R<CircleData> {
    let mut c = CircleData { peg, ..Default::default() };
    let [b140, no_mover, h148, b160, b163, h14c, h15c, b161] = r.bools()?;
    r.flush();
    c.b140 = b140;
    c.b160 = b160;
    c.b163 = b163;
    c.b161 = b161;
    if !no_mover {
        let m = &lvl.movers[phys.mover.ok_or_else(|| ParseError("ball w/o mover".into()))?];
        c.x = m.px;
        c.y = m.py;
    } else {
        c.x = r.f32()?;
        c.y = r.f32()?;
    }
    c.radius = r.f32()?;
    if !peg {
        // internal type 2 (real ball)
        let mut e = BallExtra::default();
        for i in 0..6 {
            e.f[i] = r.f32()?;
        }
        e.ie4 = r.i32()?;
        e.f10c = r.f32()?;
        e.f110 = r.f32()?;
        if r.version > 0xb {
            for i in 0..4 {
                e.pts[i] = (r.f32()?, r.f32()?);
            }
        }
        let (mut h150, mut h158, mut h154) = (false, false, false);
        if r.version > 0xc {
            let [_b164, _b165, a, b, c2, _b142, _b144, _b145] = r.bools()?;
            r.flush();
            h150 = a;
            h158 = b;
            h154 = c2;
        }
        if r.version > 0x17 {
            let _ie8 = r.i32()?;
        }
        let (mut h178, mut h18d) = (false, false);
        if r.version > 0x1d {
            let [_b146, b18d, _b18c, b178, _b162] = r.bools()?;
            r.flush();
            h178 = b178;
            h18d = b18d;
        }
        e.i148 = if h148 { r.i32()? } else { 1000000 };
        if h14c { e.i14c = r.i32()?; }
        if h15c { e.sub = read_obj_ptr(r, lvl, None)?; }
        if h150 { e.i150 = r.i32()?; }
        e.i158 = if h158 { r.i32()? } else { -1000 };
        if h154 { e.i154 = r.i32()?; }
        if h178 {
            e.i178 = r.i32()?;
            e.i17c = r.i32()?;
            if r.version > 0x3e {
                e.i180 = r.i32()?;
            }
        }
        if h18d {
            e.other_ball = read_obj_ptr(r, lvl, Some(4))?;
        }
        c.ball_extra = Some(e);
    }
    Ok(c)
}

/// FUN_00481210 Hole::Sync
fn read_hole(r: &mut Reader, lvl: &mut Level, phys: &PhysObj) -> R<HoleData> {
    let mut h = HoleData::default();
    let [b10c, h0ec, no_mover, h0f0, h0e4, h0e8, h_fc] = r.bools()?;
    r.flush();
    h.b10c = b10c;
    h.i104 = r.i32()?;
    h.i108 = r.i32()?;
    if h0ec { h.u0ec = r.u16()?; }
    if h0f0 { h.i0f0 = r.i32()?; }
    if h0e8 { h.i0e8 = r.i32()?; }
    if h0e4 { h.target = read_obj_ptr(r, lvl, None)?; }
    if no_mover {
        h.x = r.f32()?;
        h.y = r.f32()?;
    } else if let Some(m) = phys.mover {
        h.x = lvl.movers[m].px;
        h.y = lvl.movers[m].py;
    }
    if h_fc {
        h.f0fc = r.f32()?;
        h.f100 = r.f32()?;
    }
    Ok(h)
}
