// Docs: docs/souls-format/test_support.md

use std::path::PathBuf;

use crate::oodle::Oodle;

pub fn oodle() -> Oodle {
    let dll = crate::locate::locate_oodle_dll().expect("oo2core_6_win64.dll should be found");
    Oodle::load(&dll).expect("Oodle should load")
}

pub fn msgbnd_path() -> PathBuf {
    crate::locate::game_item_msgbnd().expect("item msgbnd should be found in the game install")
}

/// The item binder's BND4 payload: located, read, and unwrapped from its `DCX_KRAK` envelope.
pub fn msgbnd_payload() -> Vec<u8> {
    let raw = std::fs::read(msgbnd_path()).expect("binder should be readable");
    crate::dcx::unwrap_krak(&raw, &oodle()).expect("should unwrap the DCX")
}
