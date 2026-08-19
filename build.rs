//! Compila `ui/updater.slint` → Rust (via `slint::include_modules!()` no main).
//! Mantém o .slint como arquivo de verdade (LSP do Slint, diff limpo).

fn main() {
    slint_build::compile("ui/updater.slint").expect("falha ao compilar ui/updater.slint");
}
