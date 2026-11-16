//! Emulation of the game's x87 arithmetic.
//!
//! The game is MSVC x87 code. Every C-level `float` assignment is stored to memory (FSTP dword),
//! i.e. rounded to f32, while intermediates inside one expression stay in FPU registers at the
//! FPU's precision-control setting:
//!   * PC=53 (CW 0x27F): the game with 3D acceleration off (no Direct3D device). Register math == IEEE f64.
//!     This is the mode the playback runs in and the default here.
//!   * PC=24 (CW 0x07F): with 3D acceleration on, Direct3D switches the FPU to 24-bit precision.
//!     Register math == each op rounded to 24-bit mantissa.
//! `X` is a register value; `f32` is a memory value. Ops on X round per the compile-time mode
//! (cargo feature `pc24`). Converting X→f32 (`.r()`) is an FSTP float.
//!
//! Caveat (pc24): ops mixing an f64 constant are emulated as f64-op-then-round-to-f32, which can
//! double-round in rare cases (x87 rounds the exact result once).

use std::ops::{Add, Div, Mul, Neg, Sub};

pub const PC24: bool = cfg!(feature = "pc24");

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct X(pub f64);

#[inline(always)]
fn pc(v: f64) -> f64 {
    if PC24 { v as f32 as f64 } else { v }
}

impl X {
    #[inline(always)]
    pub fn d(v: f64) -> X { X(v) }
    /// FSTP float: round to f32.
    #[inline(always)]
    pub fn r(self) -> f32 { self.0 as f32 }
    #[inline(always)]
    pub fn abs(self) -> X { X(self.0.abs()) }
}

#[inline(always)]
pub fn x(v: f32) -> X { X(v as f64) }

macro_rules! binop {
    ($tr:ident, $f:ident, $op:tt) => {
        impl $tr<X> for X { type Output = X; #[inline(always)] fn $f(self, o: X) -> X { X(pc(self.0 $op o.0)) } }
        impl $tr<f32> for X { type Output = X; #[inline(always)] fn $f(self, o: f32) -> X { X(pc(self.0 $op o as f64)) } }
        impl $tr<X> for f32 { type Output = X; #[inline(always)] fn $f(self, o: X) -> X { X(pc(self as f64 $op o.0)) } }
    };
}
binop!(Add, add, +);
binop!(Sub, sub, -);
binop!(Mul, mul, *);
binop!(Div, div, /);

impl Neg for X {
    type Output = X;
    #[inline(always)]
    fn neg(self) -> X { X(-self.0) }
}

/// Register op on two memory floats.
#[inline(always)]
pub fn mul(a: f32, b: f32) -> X { x(a) * b }
#[inline(always)]
pub fn add(a: f32, b: f32) -> X { x(a) + b }
#[inline(always)]
pub fn sub(a: f32, b: f32) -> X { x(a) - b }

/// CRT sqrt (_CIsqrt @59e6a0) of an f32 memory value, result stored to f32. Correctly rounded
/// in both modes (sqrt to f64 then f32 is innocuous; PC24 FSQRT rounds directly).
#[inline(always)]
pub fn sqrt_r(v: f32) -> f32 { v.sqrt() }

// FCOM between memory floats and register values: exact comparison.
impl PartialEq<X> for f32 {
    fn eq(&self, o: &X) -> bool { (*self as f64) == o.0 }
}
impl PartialOrd<X> for f32 {
    fn partial_cmp(&self, o: &X) -> Option<std::cmp::Ordering> { (*self as f64).partial_cmp(&o.0) }
}
impl PartialEq<f32> for X {
    fn eq(&self, o: &f32) -> bool { self.0 == *o as f64 }
}
impl PartialOrd<f32> for X {
    fn partial_cmp(&self, o: &f32) -> Option<std::cmp::Ordering> { self.0.partial_cmp(&(*o as f64)) }
}
