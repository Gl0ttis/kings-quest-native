//! The picture painter, shared with the IBM PCjr port (crates/kq1-ibm: the PC engine
//! draws the PCjr's pictures the same way), and where the PC engine keeps the frame it
//! draws round every room picture.

pub use kq1_ibm::pic::*;

/// The frame the engine draws before every room picture: a black, priority-0 rectangle
/// round the edge, which fills stop at. The sequence is the PCjr's (`DS:$1981` there),
/// at `DS:$1FCF` in the PC engine's data segment.
pub const FRAME: usize = 0x1FCF;
