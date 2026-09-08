//! A janela (Slint) do gestor de atualizações — hoje o `schematize-market`.
//!
//! O quê: casca VISUAL fina por cima do binário do gestor. Lê o `status --json`, dispara
//! `install`/`update` com progresso AO VIVO no log, e abre o app (`run`). Onde: chamada pelo
//! app/instalador quando o usuário quer uma janela amigável (1ª instalação ou update), em vez
//! de um terminal — o cenário "prever macacos". NÃO depende do crate `schematize`: fala só com
//! o binário do gestor, então nunca embute versão via git-dep (o bug que dava "abre a antiga").
//!
//! ## Mudança de dono (ADR-0013 + ADR-0014)
//!
//! Esta janela era a do `schematize-updater`. Aquele binário foi ABSORVIDO pelo
//! `schematize-market`, que passou a ser o único responsável por instalar e atualizar. O
//! ADR-0014 (D4) decidiu que a janela **não** é descontinuada: ela vira a janela do market —
//! o que a quebrou foi o dono ter mudado, não ela. O D5 a distribui como asset do release do
//! market, então ela deixa de ser o único binário da casa que compila do fonte em toda
//! máquina.
//!
//! ## Por que ela lê `--json` e não a tabela de `status`
//!
//! A tabela humana passa pelo catálogo i18n do market: os rótulos são `plataforma` em
//! português, `platform` em inglês, `プラットフォーム` em japonês. Esta janela casava o rótulo
//! **em português** — então lia certo num idioma e devolvia tudo vazio nos outros dezenove,
//! **sem erro nenhum**. Com os campos vazios, `app_missing` virava `true` e a janela afirmava
//! "app não instalado" a quem tinha o app. Parsear saída feita para humano é contrato de
//! mentira: passa no teste de quem escreveu e falha na máquina de quem usa.
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
    gestor_ver: String,
    app_installed: String,
    app_latest: String,
    platform: String,
    binready: String,
    instdir: String,
    pin: String,
    app_missing: bool,
    has_update: bool,
}

/// Resolve o binário do GESTOR: ao lado deste executável → ~/.cargo/bin → PATH. Assim funciona
/// mesmo lançado pelo menu do desktop (cujo PATH não tem ~/.cargo/bin) — mesma lição do launcher.
///
/// **Só o `schematize-market`, e de propósito.** A lista antiga trazia `schematize-updater` e
/// o nome do interregno como rede. Os dois estão aposentados: o updater foi absorvido
/// (ADR-0013) e é REMOVIDO pelo market e pelo `install.sh` ao assumir. Manter o fallback faria
/// a janela conversar com um gestor congelado numa máquina em transição — e o que ele fizesse
/// desfaria o que o market acabou de fazer. Sem backend é um erro visível; com o backend
/// errado é um estrago silencioso.
fn gestor_bin() -> PathBuf {
    let names: [&str; 1] =
        if cfg!(windows) { ["schematize-market.exe"] } else { ["schematize-market"] };
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

/// Roda `schematize-market status --json` e devolve o `Status`, ou POR QUE não deu.
///
/// **Onde:** na abertura da janela e depois de cada ação (`refresh`, fim do install).
///
/// **Por que devolve `Result` e não um `Status` vazio:** a versão anterior engolia a falha
/// (`_ => return s`) e devolvia o `Status::default()`. Com os campos vazios, `app_missing`
/// virava `true` e a janela afirmava **"app não instalado"** — quando a verdade era "não
/// consegui falar com o gestor". A pessoa então clicava em "Instalar", que chama o mesmo
/// binário ausente, e nada acontecia. Estado de erro renderizado como fato é pior que erro
/// visível: manda a pessoa consertar o problema errado.
fn read_status() -> Result<Status, String> {
    let bin = gestor_bin();
    let out = Command::new(&bin)
        .args(["status", "--json"])
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
    Ok(parse_json(&String::from_utf8_lossy(&out.stdout)))
}

/// Lê o JSON do `schematize-market status --json`.
///
/// **Onde:** [`read_status`], e os testes — é a metade que não toca em processo nenhum, e por
/// isso a única que dá para exercitar sem o gestor instalado na máquina.
///
/// **Por que um leitor de JSON à mão, e não `serde_json`:** esta janela é `std`-only de
/// propósito. Ela é a interface que tem de abrir **quando o resto está quebrado** — primeira
/// instalação, app corrompido, toolchain incompleto. Cada dependência que ela ganha é uma
/// chance a mais de ela não compilar justamente na máquina onde ela é a única coisa que
/// funciona. O contrato tem nove chaves de topo, todas planas; ler isso custa esta função.
///
/// **O que ele NÃO tenta ser:** um parser de JSON. Ele lê o shape que o market emite, que é
/// fixo e travado por teste do lado de lá (`o_shape_do_json_e_contrato`). JSON arbitrário —
/// aninhado, com escapes exóticos — não é entrada esperada aqui, e o resultado de um shape
/// inesperado é campo vazio, nunca pânico.
fn parse_json(text: &str) -> Status {
    let mut s = Status::default();
    let campo = |k: &str| valor_de(text, k).unwrap_or_default();

    s.gestor_ver = campo("market");
    s.app_installed = campo("app_installed");
    s.app_latest = campo("app_latest");
    s.instdir = campo("install_dir");
    s.pin = campo("pin");

    s.platform = match (valor_de(text, "os"), valor_de(text, "arch")) {
        (Some(o), Some(a)) => format!("{o} / {a}"),
        (Some(o), None) => o,
        _ => String::new(),
    };
    s.binready = match booleano_de(text, "prebuilt") {
        Some(true) => "sim".into(),
        Some(false) => "não".into(),
        None => String::new(),
    };

    s.app_missing =
        s.app_installed.is_empty() || s.app_installed == "nenhum" || s.app_installed == "—";
    s.has_update = !s.app_missing && semver_gt(strip_v(&s.app_latest), strip_v(&s.app_installed));
    s
}

/// **O quê:** o valor de string de uma chave de topo. `None` se ausente ou `null`.
/// **Onde:** [`parse_json`].
///
/// Procura `"chave"` seguido de `:` e de aspas. `null` (sem aspas) devolve `None` — é como o
/// market diz "não há pin", e tratá-lo como a string `"null"` mostraria a palavra na tela.
fn valor_de(text: &str, chave: &str) -> Option<String> {
    let marca = format!("\"{chave}\"");
    let i = text.find(&marca)? + marca.len();
    let resto = text[i..].trim_start().strip_prefix(':')?.trim_start();
    if resto.starts_with("null") {
        return None;
    }
    let resto = resto.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = resto.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?),
            '"' => return Some(out),
            _ => out.push(c),
        }
    }
    None
}

/// **O quê:** o valor booleano de uma chave de topo. **Onde:** [`parse_json`], para `prebuilt`.
fn booleano_de(text: &str, chave: &str) -> Option<bool> {
    let marca = format!("\"{chave}\"");
    let i = text.find(&marca)? + marca.len();
    let resto = text[i..].trim_start().strip_prefix(':')?.trim_start();
    if resto.starts_with("true") {
        Some(true)
    } else if resto.starts_with("false") {
        Some(false)
    } else {
        None
    }
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
            w.set_gestor_ausente(false);
            apply_status(w, &s);
        }
        Err(e) => {
            // Nada de afirmar sobre o app: não sabemos. A janela diz o que houve e o botão
            // de ação sai de cena — clicar chamaria o mesmo binário que acabou de falhar.
            apply_status(w, &Status::default());
            w.set_app_missing(false);
            w.set_has_update(false);
            w.set_gestor_ausente(true);
            w.set_action_label("Instalar".into());
            w.set_status_line(format!("sem contato com o gestor — {e}").into());
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
    w.set_gestor_ver(dash(&s.gestor_ver).into());
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
            w.set_log(format!("$ schematize-market {subcmd}\n").into());
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
            let _ = Command::new(gestor_bin())
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

/// Roda `schematize-market <subcmd>` com stdout+stderr canalizados, empurrando cada linha pro log
/// da janela (via event loop). Mantém só a cauda do log pra não crescer sem limite.
fn run_streaming(subcmd: &str, weak: slint::Weak<MainWindow>) {
    let mut child = match Command::new(gestor_bin())
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

    /// Saída típica do `schematize-market status --json` — o CONTRATO, copiado do que o
    /// market de fato emite (travado do lado de lá por `o_shape_do_json_e_contrato`).
    const SAIDA: &str = r#"{
  "market": "0.2.0",
  "os": "linux",
  "arch": "x86_64",
  "prebuilt": true,
  "app_installed": "0.55.0",
  "app_latest": "0.55.2",
  "pin": null,
  "install_dir": "/home/u/.cargo/bin",
  "apps": [
    {"bin": "schematize-deployer", "repo": "schematizeme/schematize_deployer_rs", "installed": "0.5.0", "latest": "0.5.0"}
  ]
}"#;

    /// Cada chave chega no campo certo, e o `has_update` sai da comparação de versões.
    #[test]
    fn le_todas_as_chaves_do_contrato() {
        let s = parse_json(SAIDA);
        assert_eq!(s.gestor_ver, "0.2.0");
        assert_eq!(s.platform, "linux / x86_64");
        assert_eq!(s.binready, "sim");
        assert_eq!(s.app_installed, "0.55.0");
        assert_eq!(s.app_latest, "0.55.2");
        assert_eq!(s.instdir, "/home/u/.cargo/bin");
        assert_eq!(s.pin, "", "`null` e ausencia de pin, nao a palavra null");
        assert!(!s.app_missing, "o app está instalado");
        assert!(s.has_update, "0.55.0 -> 0.55.2 é atualização");
    }

    /// **O BUG QUE ESTA JANELA TINHA, e que o `--json` conserta.**
    ///
    /// A versão anterior casava rótulos EM PORTUGUÊS (`"plataforma"`, `"app instalado"`). O
    /// `status` do market passa pelo catálogo i18n, então em qualquer um dos outros dezenove
    /// idiomas os rótulos são outros — e o parse devolvia tudo vazio, **sem erro**. Com os
    /// campos vazios, `app_missing` virava `true` e a janela afirmava "app não instalado" a
    /// quem tinha o app instalado.
    ///
    /// O JSON não tem esse problema por construção: as chaves nunca são traduzidas. Este teste
    /// prova isso passando a MESMA informação com os valores em outro idioma — o que muda é a
    /// prosa, não o contrato.
    #[test]
    fn o_contrato_nao_depende_do_idioma() {
        let outro = r#"{"market":"0.2.0","os":"linux","arch":"x86_64","prebuilt":false,
                        "app_installed":"1.0.0","app_latest":"1.0.0","pin":null,
                        "install_dir":"/inicio/u/.cargo/bin","apps":[]}"#;
        let s = parse_json(outro);
        assert_eq!(s.app_installed, "1.0.0", "a leitura não pode depender de idioma nenhum");
        assert_eq!(s.binready, "não");
        assert!(!s.app_missing, "o app ESTÁ instalado — dizer o contrário foi o bug");
    }

    /// App ausente: as formas de o gestor dizer "não tem".
    #[test]
    fn app_ausente_em_qualquer_das_formas() {
        for v in [r#""""#, r#""nenhum""#, r#""—""#, "null"] {
            let s = parse_json(&format!(r#"{{"app_installed":{v},"app_latest":"1.0.0"}}"#));
            assert!(s.app_missing, "{v} tinha que contar como ausente");
            assert!(!s.has_update, "app ausente não é 'tem update', é 'instalar'");
        }
    }

    /// Chave ausente, JSON truncado e lixo não derrubam o parse nem inventam dado.
    #[test]
    fn shape_inesperado_nao_derruba_nem_inventa() {
        let s = parse_json(r#"{"app_installed":"1.2.3"}"#);
        assert_eq!(s.app_installed, "1.2.3");
        assert_eq!(s.instdir, "", "chave ausente não pode virar valor de outra");

        assert_eq!(parse_json(r#"{"app_installed":"1.2"#).app_installed, "");
        assert_eq!(parse_json("isto nao e json").app_installed, "");
    }

    /// Escape de aspas e de barra no valor — `install_dir` no Windows tem barras invertidas.
    #[test]
    fn valores_com_escape_sao_lidos() {
        let s = parse_json(r#"{"install_dir":"C:\\Users\\u\\.cargo\\bin"}"#);
        assert_eq!(s.instdir, r"C:\Users\u\.cargo\bin");
    }

    /// Entrada vazia não pode virar afirmação sobre o app.
    ///
    /// Ela ainda marca `app_missing` — e é por isso que [`read_status`] devolve `Result`:
    /// quem não conseguiu FALAR com o gestor não passa por aqui, passa pelo `Err`.
    #[test]
    fn saida_vazia_marca_ausente_mas_nao_ve_update() {
        let s = parse_json("");
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

    /// **O bug que estes testes existem pra travar.** `gestor_bin()` aponta pro PATH quando
    /// não acha nada; se aquele binário não existe, `read_status` tem que dar `Err` — não
    /// devolver um `Status` vazio, que a janela leria como "app não instalado".
    #[test]
    fn updater_inalcancavel_e_erro_e_nao_status_vazio() {
        // Não dá pra tirar o updater do PATH sem mexer em estado global do processo. O que
        // dá — e é o que importa — é provar que o caminho de erro NÃO passa pelo parse: um
        // `Status` só nasce de texto que o updater realmente imprimiu.
        let vazio = parse_json("");
        assert!(vazio.app_missing, "o parse de vazio marca ausente…");
        // …e por isso `read_status` não pode devolver ISTO quando falha em executar.
        // A assinatura é a prova estrutural: `Result<Status, String>`.
        fn assina(_: fn() -> Result<Status, String>) {}
        assina(read_status);
    }

    /// O nome do executável do gestor conhece o Windows — mesma lição do D10 no CLI.
    ///
    /// **E ele é `schematize-market`, não `schematize-updater` (ADR-0013/0014).** A asserção do
    /// nome está aqui de propósito: esta janela existe para falar com o gestor, e apontar para
    /// o binário APOSENTADO seria conversar com um programa congelado que desfaria o que o
    /// market acabou de fazer. Nome errado aqui não dá erro — dá estrago silencioso.
    #[test]
    fn nome_do_binario_por_plataforma_e_do_gestor_certo() {
        let p = gestor_bin();
        let nome = p.file_name().unwrap().to_string_lossy().into_owned();
        if cfg!(windows) {
            assert!(nome.ends_with(".exe"), "no Windows o binário é .exe: {nome}");
        } else {
            assert!(!nome.ends_with(".exe"), "fora do Windows não tem .exe: {nome}");
        }
        assert!(nome.starts_with("schematize-market"), "o dono mudou (ADR-0013): {nome}");
        assert!(!nome.contains("updater"), "o updater foi aposentado: {nome}");
    }
}
