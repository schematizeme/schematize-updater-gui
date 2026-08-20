//! schematize-updater-gui — janela (Slint) do gestor de atualizações.
//!
//! O quê: casca VISUAL fina por cima do binário `schematize-updater` (headless std-only). Lê o
//! `status`, dispara `install`/`update` com progresso AO VIVO no log, e abre o app (`run`). Onde:
//! chamada pelo app/instalador quando o usuário quer uma janela amigável (1ª instalação ou update),
//! em vez de um terminal — o cenário "prever macacos". NÃO depende do crate `schematize`: fala só
//! com o binário do updater, então nunca embute versão via git-dep (o bug que dava "abre a antiga").
#![windows_subsystem = "windows"]

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

slint::include_modules!();

/// Estado lido do `schematize-updater status`.
#[derive(Default, Clone)]
struct Status {
    updater_ver: String,
    app_installed: String,
    app_latest: String,
    platform: String,
    binready: String,
    instdir: String,
    pin: String,
    app_missing: bool,
    has_update: bool,
}

/// Resolve o binário do updater: PATH → ~/.cargo/bin → ao lado deste executável. Assim funciona
/// mesmo lançado pelo menu do desktop (cujo PATH não tem ~/.cargo/bin) — mesma lição do launcher.
fn updater_bin() -> PathBuf {
    // DOIS nomes, novo primeiro: o app virou Overflow e `schematize-updater` segue
    // instalado em máquina que não atualizou. Procurar só um deles deixaria a janela
    // sem backend — e o sintoma seria uma GUI que abre e não faz nada.
    let names: [&str; 2] = if cfg!(windows) {
        ["overflow-updater.exe", "schematize-updater.exe"]
    } else {
        ["overflow-updater", "schematize-updater"]
    };
    let name = names[0];
    // 1) ao lado de mim (instalação canônica em ~/.cargo/bin junto do gui).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for n in names {
                let c = dir.join(n);
                if c.is_file() {
                    return c;
                }
            }
        }
    }
    // 2) ~/.cargo/bin.
    if let Some(home) = home_dir() {
        for n in names {
            let c = home.join(".cargo").join("bin").join(n);
            if c.is_file() {
                return c;
            }
        }
    }
    // 3) confia no PATH.
    PathBuf::from(name)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).or_else(|| {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    })
}

/// Roda `schematize-updater status` e faz o parse por prefixo de rótulo.
fn read_status() -> Status {
    let mut s = Status::default();
    let out = Command::new(updater_bin())
        .arg("status")
        .stdin(Stdio::null())
        .output();
    let text = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => return s,
    };
    for line in text.lines() {
        let Some((label, value)) = line.split_once(':') else { continue };
        let label = label.trim();
        let value = value.trim().to_string();
        match label {
            // Os dois rótulos: GUI e updater se atualizam em momentos diferentes, e
            // uma janela nova falando com um updater antigo (ou o inverso) tem de ler
            // a versão do mesmo jeito.
            "overflow-updater" | "schematize-updater" => s.updater_ver = value,
            "plataforma" => s.platform = value,
            "binário pronto?" => s.binready = value,
            "app instalado" => s.app_installed = value,
            "última publicada" => s.app_latest = value,
            "dir de instalação" => s.instdir = value,
            l if l.starts_with("versão fixada") => s.pin = value,
            _ => {}
        }
    }
    s.app_missing = s.app_installed.is_empty()
        || s.app_installed == "nenhum"
        || s.app_installed == "—";
    s.has_update = !s.app_missing && semver_gt(strip_v(&s.app_latest), strip_v(&s.app_installed));
    s
}

fn strip_v(s: &str) -> &str {
    s.trim().trim_start_matches('v')
}

/// `a > b` em semver simples (major.minor.patch). Partes não-numéricas → 0. Não-versões → false.
fn semver_gt(a: &str, b: &str) -> bool {
    let pa: Vec<u64> = a.split('.').map(|x| x.parse().unwrap_or(0)).collect();
    let pb: Vec<u64> = b.split('.').map(|x| x.parse().unwrap_or(0)).collect();
    if pa.iter().all(|&n| n == 0) || pb.iter().all(|&n| n == 0) {
        return false; // uma delas não é versão (ex.: "? (rede)")
    }
    for i in 0..pa.len().max(pb.len()) {
        let (x, y) = (pa.get(i).copied().unwrap_or(0), pb.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

/// Aplica um Status na janela.
fn apply_status(w: &MainWindow, s: &Status) {
    let dash = |v: &str| if v.is_empty() { "—".to_string() } else { v.to_string() };
    w.set_updater_ver(dash(&s.updater_ver).into());
    w.set_app_installed(dash(&s.app_installed).into());
    w.set_app_latest(dash(&s.app_latest).into());
    w.set_platform(dash(&s.platform).into());
    w.set_binready(dash(&s.binready).into());
    w.set_instdir(dash(&s.instdir).into());
    w.set_pin(s.pin.clone().into());
    w.set_app_missing(s.app_missing);
    w.set_has_update(s.has_update);
    w.set_action_label(
        if s.app_missing { "Instalar" } else if s.has_update { "Atualizar" } else { "Reinstalar" }
            .into(),
    );
}

fn main() -> Result<(), slint::PlatformError> {
    let w = MainWindow::new()?;

    // Carrega o status inicial.
    apply_status(&w, &read_status());

    let busy = Arc::new(AtomicBool::new(false));

    // ---- Ação primária: install/update com progresso ao vivo ----
    {
        let weak = w.as_weak();
        let busy = busy.clone();
        w.on_do_primary(move || {
            if busy.swap(true, Ordering::SeqCst) {
                return; // já rodando
            }
            let Some(w) = weak.upgrade() else { return };
            let subcmd = if w.get_app_missing() { "install" } else { "update" };
            w.set_busy(true);
            w.set_log(format!("$ schematize-updater {subcmd}\n").into());
            w.set_status_line("baixando/compilando — isso pode levar alguns minutos na 1ª vez…".into());

            let weak2 = weak.clone();
            let busy2 = busy.clone();
            std::thread::spawn(move || {
                run_streaming(subcmd, weak2.clone());
                // ao terminar: recarrega status e libera os botões, no event loop.
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        apply_status(&w, &read_status());
                        w.set_busy(false);
                        w.set_status_line("concluído. reabra o app se estava aberto.".into());
                    }
                    busy2.store(false, Ordering::SeqCst);
                });
            });
        });
    }

    // ---- Abrir app ----
    {
        w.on_do_launch(move || {
            let _ = Command::new(updater_bin())
                .arg("run")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        });
    }

    // ---- Rechecar status ----
    {
        let weak = w.as_weak();
        w.on_refresh(move || {
            if let Some(w) = weak.upgrade() {
                apply_status(&w, &read_status());
                w.set_status_line("status atualizado.".into());
            }
        });
    }

    // ---- Alternar tema ----
    {
        let weak = w.as_weak();
        w.on_toggle_theme(move || {
            if let Some(w) = weak.upgrade() {
                w.set_dark(!w.get_dark());
            }
        });
    }

    w.run()
}

/// Roda `schematize-updater <subcmd>` com stdout+stderr canalizados, empurrando cada linha pro log
/// da janela (via event loop). Mantém só a cauda do log pra não crescer sem limite.
fn run_streaming(subcmd: &str, weak: slint::Weak<MainWindow>) {
    let mut child = match Command::new(updater_bin())
        .arg(subcmd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            push_log(&weak, format!("erro: não consegui executar o updater: {e}\n"));
            return;
        }
    };

    // stderr numa thread; stdout na atual — as duas empurram pro mesmo log.
    let stderr = child.stderr.take();
    let weak_err = weak.clone();
    let err_handle = std::thread::spawn(move || {
        if let Some(err) = stderr {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                push_log(&weak_err, format!("{line}\n"));
            }
        }
    });

    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            push_log(&weak, format!("{line}\n"));
        }
    }
    let _ = err_handle.join();
    match child.wait() {
        Ok(st) if st.success() => push_log(&weak, "\n✓ concluído.\n".into()),
        Ok(st) => push_log(&weak, format!("\n✗ falhou ({st}).\n")),
        Err(e) => push_log(&weak, format!("\n✗ erro ao esperar o processo: {e}\n")),
    }
}

/// Anexa `line` ao log da janela pelo event loop, limitando ao tail (~20k chars).
fn push_log(weak: &slint::Weak<MainWindow>, line: String) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(w) = weak.upgrade() {
            let mut s = w.get_log().to_string();
            s.push_str(&line);
            if s.len() > 20_000 {
                s = s[s.len() - 20_000..].to_string();
            }
            w.set_log(s.into());
        }
    });
}
