//! refemu-pcjr as a library: the 8086 core, the PCjr, and a runner that stops at each
//! pass, for the tools built on it (`kqroute-pcjr`).

pub mod cpu;
pub mod machine;
pub mod runner;
