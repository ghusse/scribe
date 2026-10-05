#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod controller;
mod secrets;
mod services;
mod settings;

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("erreur au lancement de Scribe");
}
