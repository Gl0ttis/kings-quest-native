//! refemu-pcjr as a library: the 8086 core, the PCjr, and a runner that stops at each
//! pass, for the tools built on it (`kqroute-pcjr`).

/// The 8086 core, shared with the other reference machine (crates/kq1-x86).
pub use kq1_x86 as cpu;
pub mod machine;
pub mod runner;
