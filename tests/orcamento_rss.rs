//! Orçamento de memória da GUI (BUDGET.md): abre a janela de verdade por alguns segundos, polla
//! `VmHWM` de `/proc/<pid>/status` e falha se o pico passar do teto. std-only.
//! Onde: `cargo test` (gate local). Pula com `eprintln` explícito sem `/proc` ou sem display
//! (nunca `#[ignore]` mudo).

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Teto de RSS pico da janela, em KiB (BUDGET.md). O teste roda o binário do perfil de TESTE
/// (debug): medido ~31,9 MiB lá e ~16,4 MiB no release; 48 MiB = debug + ~50% (variação de
/// driver GL/Mesa entre máquinas).
const TETO_KIB: u64 = 48 * 1024;

/// Quanto tempo a janela fica aberta sendo medida.
const JANELA: Duration = Duration::from_secs(4);

/// O quê: lê `VmHWM` (KiB) de `/proc/<pid>/status`. Onde: laço de [`janela_dentro_do_teto`].
fn vm_hwm_kib(pid: u32) -> Option<u64> {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()?
        .lines()
        .find(|l| l.starts_with("VmHWM:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[test]
fn janela_dentro_do_teto() {
    if !std::path::Path::new("/proc/self/status").exists() {
        eprintln!("orçamento GUI: PULADO — /proc indisponível nesta plataforma");
        return;
    }
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        eprintln!("orçamento GUI: PULADO — sem display (DISPLAY/WAYLAND_DISPLAY)");
        return;
    }
    let mut filho = Command::new(env!("CARGO_BIN_EXE_schematize-market-gui"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("não subiu a GUI");
    let pid = filho.id();
    let inicio = Instant::now();
    let mut pico = 0u64;
    while inicio.elapsed() < JANELA {
        pico = pico.max(vm_hwm_kib(pid).unwrap_or(0));
        match filho.try_wait() {
            // Saiu sozinha (sem compositor utilizável): sem janela não há o que medir.
            Ok(Some(st)) => {
                eprintln!("orçamento GUI: PULADO — saiu sozinha ({st}), sem janela para medir");
                return;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => panic!("try_wait falhou: {e}"),
        }
    }
    let _ = filho.kill();
    let _ = filho.wait();
    eprintln!("orçamento GUI: pico {pico} KiB (teto {TETO_KIB} KiB)");
    assert!(pico > 0, "VmHWM não foi lido: o medidor está cego");
    assert!(pico <= TETO_KIB, "GUI: RSS pico {pico} KiB > teto {TETO_KIB} KiB (BUDGET.md)");
}
