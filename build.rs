//! Compila `ui/updater.slint` → Rust (via `slint::include_modules!()` no main).
//! Mantém o .slint como arquivo de verdade (LSP do Slint, diff limpo).

use std::process::Command;

/// **O quê:** grava no binário o SHA do commit e se a árvore estava suja (`SCHEMATIZE_UPDGUI_SHA` e
/// `SCHEMATIZE_UPDGUI_DIRTY`), e compila o `.slint`.
///
/// **Onde:** o cargo, antes de compilar o crate. A decisão de como MOSTRAR é pura, em
/// `src/procedencia.rs`.
fn main() {
    // O SHA do commit, para o `--version` dizer de QUAL build ele é. `--short=12`: 7 colide em
    // repo grande, 40 não cabe numa linha. Vazio = sem git (tarball): falhar o build por isso
    // trocaria um diagnóstico por um bloqueio.
    let sha = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("cargo:rustc-env=SCHEMATIZE_UPDGUI_SHA={sha}");

    // **Árvore suja.** Binário construído com mudança NÃO commitada não é o HEAD: sem esta
    // marca o `--version` mentia o SHA do commit anterior. Só arquivos RASTREADOS contam
    // (`--untracked-files=no`): um rascunho solto na pasta não entra no binário e não deve
    // marcá-lo. Falha do git => "limpo" (sem git já cai em "fonte sem git" pelo SHA vazio).
    let sujo = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .is_some_and(|o| !o.stdout.iter().all(u8::is_ascii_whitespace));
    println!("cargo:rustc-env=SCHEMATIZE_UPDGUI_DIRTY={}", if sujo { "1" } else { "" });

    // Sem isto o valor congela no primeiro build e a versão passa a MENTIR.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");

    // O veredito "sujo" depende do CONTEÚDO da árvore, que não passa por `.git/HEAD`: editar um
    // arquivo rastreado não mexe em HEAD nem em refs. Por isso vigiamos o que vira binário —
    // `src`, `ui`, `Cargo.*`, este `build.rs` (diretório = mtime recursivo) — e `.git/index`
    // (`git add`/`reset`/`stash`). NÃO vigiamos a árvore inteira (`.`): `target/` muda a cada
    // build e o script rodaria sempre. Caminho inexistente em `rerun-if-changed` conta como
    // "mudou" e re-roda TODO build, então só se emite o que existe.
    for caminho in ["src", "ui", "Cargo.toml", "Cargo.lock", "build.rs", ".git/index"] {
        if std::path::Path::new(caminho).exists() {
            println!("cargo:rerun-if-changed={caminho}");
        }
    }

    slint_build::compile("ui/updater.slint").expect("falha ao compilar ui/updater.slint");
}
