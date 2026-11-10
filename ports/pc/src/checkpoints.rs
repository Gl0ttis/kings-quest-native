//! The checkpoints: addresses in the original engine's code segment, routine entries and
//! loop heads, whose count the port's clock replays the timer's ticks against (the PCjr
//! port's src/engine/clock.rs). The PC's addresses of the PCjr's checkpoints
//! (docs/engine.md).

pub const CHECKPOINTS: [u16; 22] = [
    0x12A0, // poll_key_bios: INT 16h, bypassing the ring
    0x2209, // next_parse
    0x24B1, // read_line
    0x2816, // wait_key: one game cycle per loop
    0x293A, // game_cycle: one pass
    0x2AAD, // update_actor
    0x2EB4, // key_ring_pop
    0x2EF4, // run_action
    0x2F03, // run_action: each opcode
    0x3FB7, // countdowns and the draw pass
    0x48E3, // draw_room_picture
    0x4CA3, // save_restore_dialog
    0x078E, // save_slot
    0x0867, // load_slot
    0x08D4, // key_poll (INT 16h AH=1)
    0x08DF, // key_wait (INT 16h AH=0)
    0x0785, // wait_enter
    0x1143, // match_parser_rules
    0x1150, // match_cycle_rules
    0x115A, // the matcher: each rule
    0x5018, // title_wait
    0x1FB6, // read_sectors
];
