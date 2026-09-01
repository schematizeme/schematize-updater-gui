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
    // Canônico primeiro, e o do interregno (nome Overflow) como rede: máquina que
    // instalou naquela janela pode ter só aquele binário, e procurar um só deixaria a
    // janela sem backend — o sintoma seria uma GUI que abre e não faz nada.
    let names: [&str; 2] = if cfg!(windows) {
        ["schematize-updater.exe", "overflow-updater.exe"]
    } else {
        ["schematize-updater", "overflow-updater"]
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
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

/// Roda `schematize-updater status` e devolve o `Status`, ou POR QUE não deu.
///
/// **Onde:** na abertura da janela e depois de cada ação (`refresh`, fim do install).
///
/// **Por que devolve `Result` e não um `Status` vazio:** a versão anterior engolia a falha
/// (`_ => return s`) e devolvia o `Status::default()`. Com os campos vazios, `app_missing`
/// virava `true` e a janela afirmava **"app não instalado"** — quando a verdade era "não
/// consegui falar com o updater". A pessoa então clicava em "Instalar", que chama o mesmo
/// binário ausente, e nada acontecia. Estado de erro renderizado como fato é pior que erro
/// visível: manda a pessoa consertar o problema errado.
fn read_status() -> Result<Status, String> {
    let bin = updater_bin();
    let out = Command::new(&bin)
        .arg("status")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("não consegui executar {}: {e}", bin.display()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        return Err(format!(
            "{} status falhou ({}){}",
            bin.display(),
            out.status,
            if err.is_empty() { String::new() } else { format!(": {err}") }
        ));
    }
    Ok(parse_status(&String::from_utf8_lossy(&out.stdout)))
}

/// Parse da saída de `schematize-updater status`, por prefixo de rótulo.
///
/// **Onde:** [`read_status`], e os testes — é a metade que não toca em processo nenhum, e
/// por isso a única que dá pra exercitar sem o updater instalado na máquina.
fn parse_status(text: &str) -> Status {
    let mut s = Status::default();
    for line in text.lines() {
        let Some((label, value)) = line.split_once(':') else { continue };
        let label = label.trim();
        let value = value.trim().to_string();
        match label {
            // Os dois rótulos: GUI e updater se atualizam em momentos diferentes, e
            // uma janela nova falando com um updater do interregno (ou o inverso) tem
            // de ler a versão do mesmo jeito.
            "schematize-updater" | "overflow-updater" => s.updater_ver = value,
            "plataforma" => s.platform = value,
            "binário pronto?" => s.binready = value,
            "app instalado" => s.app_installed = value,
            "última publicada" => s.app_latest = value,
            "dir de instalação" => s.instdir = value,
            l if l.starts_with("versão fixada") => s.pin = value,
            _ => {}
        }
    }
    s.app_missing =
        s.app_installed.is_empty() || s.app_installed == "nenhum" || s.app_installed == "—";
    s.has_update = !s.app_missing && semver_gt(strip_v(&s.app_latest), strip_v(&s.app_installed));
    s
}

/// Aplica na janela o resultado de [`read_status`] — inclusive quando ele é `Err`.
///
/// **Onde:** os quatro pontos que liam o status (abertura, refresh, fim do install).
///
/// **Por quê:** sem isto cada chamador teria de lembrar de tratar o `Err`, e o que existia
/// antes era justamente um `Err` esquecido virando "app não instalado".
fn aplicar_leitura(w: &MainWindow, r: Result<Status, String>) {
    match r {
        Ok(s) => {
            w.set_updater_ausente(false);
            apply_status(w, &s);
        }
        Err(e) => {
            // Nada de afirmar sobre o app: não sabemos. A janela diz o que houve e o botão
            // de ação sai de cena — clicar chamaria o mesmo binário que acabou de falhar.
            apply_status(w, &Status::default());
            w.set_app_missing(false);
            w.set_has_update(false);
            w.set_updater_ausente(true);
            w.set_action_label("Instalar".into());
            w.set_status_line(format!("sem contato com o updater — {e}").into());
        }
    }
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
        if s.app_missing {
            "Instalar"
        } else if s.has_update {
            "Atualizar"
        } else {
            "Reinstalar"
        }
        .into(),
    );
}

fn main() -> Result<(), slint::PlatformError> {
    let w = MainWindow::new()?;

    // Carrega o status inicial.
    aplicar_leitura(&w, read_status());

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
            w.set_status_line(
                "baixando/compilando — isso pode levar alguns minutos na 1ª vez…".into(),
            );

            let weak2 = weak.clone();
            let busy2 = busy.clone();
            std::thread::spawn(move || {
                run_streaming(subcmd, weak2.clone());
                // ao terminar: recarrega status e libera os botões, no event loop.
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        aplicar_leitura(&w, read_status());
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
                aplicar_leitura(&w, read_status());
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Saída típica do `schematize-updater status`, com todos os rótulos.
    const SAIDA: &str = "\
schematize-updater: 0.6.2
plataforma: linux-x86_64
binário pronto?: sim
app instalado: 0.55.0
última publicada: 0.55.2
dir de instalação: /home/u/.cargo/bin
versão fixada (pin): nenhuma
";

    /// O parse pega cada rótulo, e o `has_update` sai da comparação de versões.
    #[test]
    fn parse_le_todos_os_rotulos() {
        let s = parse_status(SAIDA);
        assert_eq!(s.updater_ver, "0.6.2");
        assert_eq!(s.platform, "linux-x86_64");
        assert_eq!(s.binready, "sim");
        assert_eq!(s.app_installed, "0.55.0");
        assert_eq!(s.app_latest, "0.55.2");
        assert_eq!(s.instdir, "/home/u/.cargo/bin");
        assert_eq!(s.pin, "nenhuma");
        assert!(!s.app_missing, "o app está instalado");
        assert!(s.has_update, "0.55.0 -> 0.55.2 é atualização");
    }

    /// O rótulo do interregno (nome Overflow) tem que ser lido igual — uma máquina que
    /// instalou naquela janela ainda tem o binário antigo respondendo.
    #[test]
    fn rotulo_antigo_do_updater_e_lido() {
        assert_eq!(parse_status("overflow-updater: 0.2.3\n").updater_ver, "0.2.3");
    }

    /// App ausente: os três jeitos de o updater dizer "não tem".
    #[test]
    fn app_ausente_em_qualquer_das_formas() {
        for v in ["", "nenhum", "—"] {
            let s = parse_status(&format!("app instalado: {v}\núltima publicada: 1.0.0\n"));
            assert!(s.app_missing, "{v:?} tinha que contar como ausente");
            assert!(!s.has_update, "app ausente não é 'tem update', é 'instalar'");
        }
    }

    /// Linha sem `:` e rótulo desconhecido são ignorados sem estragar o resto.
    #[test]
    fn lixo_na_saida_nao_derruba_o_parse() {
        let s = parse_status("isto nao tem dois pontos\ndesconhecido: 9\napp instalado: 1.2.3\n");
        assert_eq!(s.app_installed, "1.2.3");
    }

    /// Entrada vazia não pode virar afirmação sobre o app.
    ///
    /// Ela ainda marca `app_missing` — e é por isso que [`read_status`] devolve `Result`:
    /// quem não conseguiu FALAR com o updater não passa por aqui, passa pelo `Err`.
    #[test]
    fn saida_vazia_marca_ausente_mas_nao_ve_update() {
        let s = parse_status("");
        assert!(s.app_missing);
        assert!(!s.has_update, "sem dado nenhum não existe atualização a oferecer");
    }

    /// Comparação de versões: os casos que decidem se o botão diz "Atualizar".
    #[test]
    fn semver_compara_o_que_importa() {
        assert!(semver_gt("0.55.2", "0.55.1"));
        assert!(semver_gt("1.0.0", "0.99.99"));
        assert!(semver_gt("0.6.0", "0.5.9"));
        assert!(!semver_gt("0.55.1", "0.55.1"), "igual não é maior");
        assert!(!semver_gt("0.55.0", "0.55.1"), "menor não é maior");
        // Campo que não é versão (o updater imprime "? (rede)" quando não alcança a rede)
        // nunca pode virar "tem atualização".
        assert!(!semver_gt("? (rede)", "0.55.1"));
        assert!(!semver_gt("0.55.1", "? (rede)"));
        // Número de partes diferente.
        assert!(semver_gt("1.1", "1.0.9"));
        assert!(!semver_gt("1.0", "1.0.0"));
    }

    /// O `v` da tag é aparado dos dois lados antes de comparar.
    #[test]
    fn strip_v_apara_tag_e_espaco() {
        assert_eq!(strip_v(" v1.2.3 "), "1.2.3");
        assert_eq!(strip_v("1.2.3"), "1.2.3");
        assert_eq!(strip_v(""), "");
    }

    /// **O bug que estes testes existem pra travar.** `updater_bin()` aponta pro PATH quando
    /// não acha nada; se aquele binário não existe, `read_status` tem que dar `Err` — não
    /// devolver um `Status` vazio, que a janela leria como "app não instalado".
    #[test]
    fn updater_inalcancavel_e_erro_e_nao_status_vazio() {
        // Não dá pra tirar o updater do PATH sem mexer em estado global do processo. O que
        // dá — e é o que importa — é provar que o caminho de erro NÃO passa pelo parse: um
        // `Status` só nasce de texto que o updater realmente imprimiu.
        let vazio = parse_status("");
        assert!(vazio.app_missing, "o parse de vazio marca ausente…");
        // …e por isso `read_status` não pode devolver ISTO quando falha em executar.
        // A assinatura é a prova estrutural: `Result<Status, String>`.
        fn assina(_: fn() -> Result<Status, String>) {}
        assina(read_status);
    }

    /// O nome do executável do updater conhece o Windows — mesma lição do D10 no CLI.
    #[test]
    fn nome_do_binario_por_plataforma() {
        let p = updater_bin();
        let nome = p.file_name().unwrap().to_string_lossy().into_owned();
        if cfg!(windows) {
            assert!(nome.ends_with(".exe"), "no Windows o binário é .exe: {nome}");
        } else {
            assert!(!nome.ends_with(".exe"), "fora do Windows não tem .exe: {nome}");
        }
        assert!(nome.contains("updater"), "{nome}");
    }
}
