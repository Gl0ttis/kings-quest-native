//! refemu-pc as a library: the 8086 core, the IBM PC with the CGA, and a runner that
//! stops at each pass, for the tools built on it (`kqroute-pc`).

pub mod cpu;
pub mod machine;
pub mod runner;
