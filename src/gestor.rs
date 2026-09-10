//! O GESTOR — falar com o binário `schematize-market`, e nada de tela.
//!
//! **O quê:** resolve onde o gestor está, lê o `status --json`, e roda um subcomando com a
//! saída AO VIVO. É a metade da janela que sabe conversar com um processo.
//!
//! **Onde:** [`crate::main`], que só liga isto aos botões.
//!
//! ## Por que este arquivo existe separado
//!
//! O `main.rs` chegou a 375 linhas úteis com a aba do Mercado — acima do limite de 300 que a
//! casa marca como sinal de falta de abstração. E a divisão natural estava à vista: um lado
//! **conversa com o gestor** (processo, JSON, versões) e o outro **liga botão a callback**.
//! Separados, o primeiro é testável sem janela nenhuma, que é o que ele já era na prática.

use crate::json;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::MainWindow;

/// Estado lido do `schematize-market status --json`.
///
/// **Campos públicos de propósito:** isto é um portador de dado que atravessa a fronteira
/// entre este módulo (que fala com o processo) e o `main` (que desenha). Getters aqui seriam
/// nove funções de uma linha cada, sem invariante nenhuma para proteger — o que há de invariante
/// (`app_missing`, `has_update`) é calculado no [`parse_json`], que é o único lugar que constrói
/// um `Status` a partir de fora.
#[derive(Default, Clone)]
pub struct Status {
    pub gestor_ver: String,
    pub app_installed: String,
    pub app_latest: String,
    pub platform: String,
    pub binready: String,
    pub instdir: String,
    pub pin: String,
    pub app_missing: bool,
    pub has_update: bool,
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
pub fn gestor_bin() -> PathBuf {
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
pub fn read_status() -> Result<Status, String> {
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
    parse_json(&String::from_utf8_lossy(&out.stdout))
}

/// Lê o JSON do `schematize-market status --json`.
///
/// **Onde:** [`read_status`], e os testes — é a metade que não toca em processo nenhum, e por
/// isso a única que dá para exercitar sem o gestor instalado na máquina.
///
/// **Ela lê com o [`crate::json`], e não mais procurando substring.** As duas funções que
/// havia aqui (`valor_de`/`booleano_de`) achavam `"chave":` no texto e liam até a próxima
/// aspa. Aquilo bastava para este contrato, que é plano — mas a janela passou a ler também o
/// `list --json`, que é uma lista de objetos, e ali a busca por substring erra em silêncio:
/// `"slug"` casa dentro de qualquer item, e todos mostrariam o do primeiro. Um leitor só, para
/// os dois, é menos código e uma classe de bug a menos.
///
/// **Documento ilegível é `Err`, e NÃO um `Status` vazio.** Este é o mesmo bug do arquivo, um
/// nível abaixo: um `Status::default()` tem `app_installed` vazio, o que faz `app_missing`
/// virar `true` — e a janela afirmaria **"app não instalado"** quando a verdade é "falei com o
/// gestor e não entendi a resposta". A pessoa clicaria em "Instalar" para consertar um problema
/// que não tem. Os dois modos de falha — não consegui executar, não consegui entender — levam
/// à mesma tela honesta: "sem contato com o gestor".
///
/// Foi a troca do leitor que expôs isto: com a busca por substring, texto ilegível devolvia
/// campos vazios em silêncio, e dois testes tinham congelado esse comportamento como se fosse
/// o desejado.
pub fn parse_json(text: &str) -> Result<Status, String> {
    let j = json::ler(text).map_err(|e| format!("resposta do gestor ilegível: {e}"))?;
    let mut s = Status::default();

    s.gestor_ver = j.str_ou_vazio("market");
    s.app_installed = j.str_ou_vazio("app_installed");
    s.app_latest = j.str_ou_vazio("app_latest");
    s.instdir = j.str_ou_vazio("install_dir");
    s.pin = j.str_ou_vazio("pin");

    s.platform = match (j.str("os"), j.str("arch")) {
        (Some(o), Some(a)) => format!("{o} / {a}"),
        (Some(o), None) => o.to_string(),
        _ => String::new(),
    };
    s.binready = match j.bool("prebuilt") {
        Some(true) => "sim".into(),
        Some(false) => "não".into(),
        None => String::new(),
    };

    s.app_missing =
        s.app_installed.is_empty() || s.app_installed == "nenhum" || s.app_installed == "—";
    s.has_update = !s.app_missing && semver_gt(strip_v(&s.app_latest), strip_v(&s.app_installed));
    Ok(s)
}

pub fn strip_v(s: &str) -> &str {
    s.trim().trim_start_matches('v')
}

/// `a > b` em semver simples (major.minor.patch). Partes não-numéricas → 0. Não-versões → false.
pub fn semver_gt(a: &str, b: &str) -> bool {
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

/// Roda `schematize-market <subcmd>` com stdout+stderr canalizados, empurrando cada linha pro log
/// da janela (via event loop). Mantém só a cauda do log pra não crescer sem limite.
pub fn run_streaming(subcmd: &str, weak: slint::Weak<MainWindow>) {
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
pub fn push_log(weak: &slint::Weak<MainWindow>, line: String) {
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
        let s = parse_json(SAIDA).expect("o contrato é legível");
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
        let s = parse_json(outro).expect("legível");
        assert_eq!(s.app_installed, "1.0.0", "a leitura não pode depender de idioma nenhum");
        assert_eq!(s.binready, "não");
        assert!(!s.app_missing, "o app ESTÁ instalado — dizer o contrário foi o bug");
    }

    /// App ausente: as formas de o gestor dizer "não tem".
    #[test]
    fn app_ausente_em_qualquer_das_formas() {
        for v in [r#""""#, r#""nenhum""#, r#""—""#, "null"] {
            let s = parse_json(&format!(r#"{{"app_installed":{v},"app_latest":"1.0.0"}}"#))
                .expect("legível");
            assert!(s.app_missing, "{v} tinha que contar como ausente");
            assert!(!s.has_update, "app ausente não é 'tem update', é 'instalar'");
        }
    }

    /// Chave ausente não vira valor de outra; e JSON truncado ou lixo vira **`Err`**, não um
    /// `Status` vazio que a janela leria como "app não instalado".
    #[test]
    fn shape_inesperado_e_erro_e_nao_dado_inventado() {
        let s = parse_json(r#"{"app_installed":"1.2.3"}"#).expect("objeto válido, só incompleto");
        assert_eq!(s.app_installed, "1.2.3");
        assert_eq!(s.instdir, "", "chave ausente não pode virar valor de outra");

        for lixo in [r#"{"app_installed":"1.2"#, "isto nao e json", ""] {
            assert!(parse_json(lixo).is_err(), "lixo tem de ser Err: {lixo:?}");
        }
    }

    /// Escape de aspas e de barra no valor — `install_dir` no Windows tem barras invertidas.
    #[test]
    fn valores_com_escape_sao_lidos() {
        let s = parse_json(r#"{"install_dir":"C:\\Users\\u\\.cargo\\bin"}"#).expect("legível");
        assert_eq!(s.instdir, r"C:\Users\u\.cargo\bin");
    }

    /// **Entrada vazia não pode virar afirmação sobre o app** — e a forma certa de não afirmar
    /// é `Err`, não um `Status` com os campos em branco.
    ///
    /// A versão anterior devolvia o `Status::default()` aqui, e `app_missing` virava `true`
    /// porque `app_installed` estava vazio. A janela então dizia "app não instalado" a quem, na
    /// verdade, tinha recebido uma resposta ilegível — e mandava a pessoa consertar o problema
    /// errado. É o mesmo defeito que esta janela já teve uma vez, um nível abaixo.
    #[test]
    fn saida_vazia_e_erro_e_nao_afirmacao_sobre_o_app() {
        assert!(parse_json("").is_err(), "vazio não é 'app ausente', é 'não sei'");
        // Um objeto VÁLIDO sem `app_installed` é outra coisa: o gestor respondeu, e o que ele
        // disse é que não há app. Aí sim `app_missing`.
        let s = parse_json("{}").expect("objeto vazio é resposta válida");
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
    fn gestor_inalcancavel_e_erro_e_nao_status_vazio() {
        // Não dá pra tirar o gestor do PATH sem mexer em estado global do processo. O que dá —
        // e é o que importa — é provar que NENHUM caminho produz um `Status` a partir de nada:
        // as duas funções devolvem `Result`, e um `Status` só nasce de texto que o gestor
        // realmente imprimiu E que deu para entender.
        fn assina_leitura(_: fn() -> Result<Status, String>) {}
        assina_leitura(read_status);
        fn assina_parse(_: fn(&str) -> Result<Status, String>) {}
        assina_parse(parse_json);
        assert!(parse_json("").is_err());
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
