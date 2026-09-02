// Release 模式下隐藏 CMD 窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    julong_codex_keysmith::run();
}
