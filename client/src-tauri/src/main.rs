// Windows のリリースビルドでコンソールウィンドウを出さない (Tauri の定番設定)。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    voice_client_lib::run();
}
