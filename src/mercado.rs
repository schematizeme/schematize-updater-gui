//! A TELA DO MERCADO — as linhas vêm do `list --json`, e as ações abrem terminal.
//!
//! **O quê:** transforma o `schematize-market list --json` nas linhas que a janela desenha, e
//! monta os comandos de instalar/remover/trocar método.
//!
//! **Onde:** a aba "Mercado" da janela. Esta tela era do hub (`schematize_gui_slint`,
//! `screen_envs.slint`), e vem para cá porque o hub delega ao market o que é do market — a
//! mesma direção do ADR-0012.
//!
//! ## O que mudou na migração, e é a única coisa que mudou
//!
//! No hub, as linhas vinham de `environments::status()` — uma chamada de biblioteca, no mesmo
//! processo. Aqui elas vêm do `list --json`, porque esta janela **não depende do crate do
//! market**: ela é `std`-only, e é a interface que tem de abrir quando o resto está quebrado.
//! Depender do crate a faria compilar junto com o gestor, e voltaríamos ao "abre a versão
//! antiga" que essa independência resolveu.
//!
//! Comportamento visível: idêntico. Três seções (linguagens, ferramentas, apps da casa), chip
//! de método só onde há método, sem botão de remover na seção de apps.
//!
//! ## Por que nenhuma decisão sai de `status_text`
//!
//! O contrato tem campos de DECISÃO (`slug`, `category`, `present`, `installed_via`,
//! `provenance`, `state`) e **um** de prosa, `status_text`, que muda de idioma. Tudo que esta
//! tela ramifica sai dos primeiros; o `status_text` só é MOSTRADO.
//!
//! Não é preciosismo: a versão anterior desta janela casava rótulo humano, lia certo em um
//! idioma e devolvia vazio nos outros dezenove — **sem erro nenhum** — e então afirmava "app
//! não instalado" a quem tinha o app.

use crate::json::Json;

/// Uma linha da tela, já pronta para desenhar.
///
/// **Onde:** [`ler`] produz; a janela consome.
#[derive(Debug, Clone, PartialEq)]
pub struct Linha {
    /// Identificador para os comandos (`go`, `rust`, `schematize-deployer`).
    pub slug: String,
    /// Nome como a pessoa o conhece ("Go", "C# / .NET").
    pub display: String,
    /// `"language"` | `"tool"` | `"app"` — é o que decide QUAIS botões a linha tem.
    pub categoria: String,
    /// Métodos disponíveis (vazio em ferramenta e em app da casa).
    pub metodos: Vec<String>,
    /// Método pré-selecionado: o que já está instalado, senão o primeiro da lista.
    pub metodo_sel: String,
    /// Texto auxiliar: as vias possíveis, ou o "sobre" do app.
    pub dica: String,
    /// Está instalado? Campo de DECISÃO — nunca inferido do texto.
    pub instalado: bool,
    /// Prosa traduzida, só para mostrar.
    pub status_texto: String,
    /// Abre uma seção nova? Traz o título quando sim.
    pub titulo_secao: String,
    /// O que aconteceu com o último clique nesta linha ("terminal aberto", ou o comando para
    /// rodar à mão). Vazio na maior parte do tempo; é a linha que dá o retorno, não um diálogo.
    pub op_rotulo: String,
}

/// **O quê:** o título de uma seção. **Onde:** [`ler`].
///
/// Em português e sem catálogo, como o resto desta janela: ela é `std`-only e não carrega os
/// vinte idiomas do market. É uma dívida honesta e está no checklist (o texto de runtime dos
/// apps em inglês + pt), não um esquecimento.
fn titulo_secao(categoria: &str) -> &'static str {
    match categoria {
        "tool" => "Ferramentas de dev",
        "app" => "Apps da casa",
        _ => "Linguagens",
    }
}

/// **O quê:** o texto de status de uma linha, com DUAS quedas antes de desistir.
///
/// **Onde:** [`linha_env`].
///
/// ## Por que isto tem fallback, e como o buraco apareceu
///
/// O campo se chamava `status`; ele passou a se chamar `status_text` quando o contrato separou
/// campo de decisão de prosa. A janela nova, rodando contra um market **ainda não atualizado**,
/// lia `status_text`, não achava nada, e desenhava uma pílula **vazia** — um oval colorido com
/// nada dentro, ao lado do nome. Foi visto ao abrir a janela de verdade, não em teste.
///
/// Isso é o §37.48: a máquina de quem usa tem o que tem, e as duas metades de um sistema
/// distribuído **nunca** se atualizam no mesmo instante. Uma janela que só funciona contra a
/// versão do dia em que ela foi escrita está quebrada por desenho.
///
/// As quedas, em ordem:
/// 1. `status_text` — o contrato de hoje.
/// 2. `status` — o nome antigo, para um market que ainda não subiu.
/// 3. **derivado dos campos de DECISÃO** — e este é o que nunca falha, porque `present` e
///    `installed_via` são o que a janela já usa para decidir. Não é traduzido, e não precisa
///    ser: se chegou aqui, o market é velho demais para ter o que traduzir.
fn status_texto(o: &Json) -> String {
    if let Some(s) = o.str("status_text").filter(|s| !s.is_empty()) {
        return s.to_string();
    }
    if let Some(s) = o.str("status").filter(|s| !s.is_empty()) {
        return s.to_string();
    }
    match (
        o.bool("present").unwrap_or(false),
        o.str("installed_via").or_else(|| o.str("provenance")),
    ) {
        (true, Some(via)) if via != "absent" => format!("via {via}"),
        (true, _) => "instalado".to_string(),
        (false, _) => "não instalado".to_string(),
    }
}

/// **O quê:** uma linha de linguagem/ferramenta, a partir de um item do contrato.
/// **Onde:** [`ler`].
fn linha_env(o: &Json) -> Linha {
    let metodos: Vec<String> =
        o.arr("methods").iter().filter_map(|m| m.como_str()).map(str::to_string).collect();
    // O método pré-selecionado é o que JÁ está instalado, quando há um. Selecionar sempre o
    // primeiro da lista faria o botão "trocar" propor trocar para o que já está lá.
    let metodo_sel = o
        .str("installed_via")
        .map(str::to_string)
        .filter(|m| metodos.contains(m))
        .or_else(|| metodos.first().cloned())
        .unwrap_or_default();
    Linha {
        slug: o.str_ou_vazio("slug"),
        display: o.str_ou_vazio("display"),
        categoria: o.str_ou_vazio("category"),
        metodos,
        metodo_sel,
        dica: o.str_ou_vazio("hint"),
        // DECISÃO, do campo booleano — nunca do `status_text`.
        instalado: o.bool("present").unwrap_or(false),
        status_texto: status_texto(o),
        titulo_secao: String::new(),
        op_rotulo: String::new(),
    }
}

/// **O quê:** uma linha de app da casa, a partir de um item de `apps`.
/// **Onde:** [`ler`].
///
/// **O `state` tem TRÊS valores, e achatá-los seria mentir.** Um binário que está lá e não
/// responde (`broken`) é problema diferente de um que não existe (`absent`): dizer "não
/// instalado" sobre o primeiro manda reinstalar o que já se tem e esconde a causa real
/// (permissão, biblioteca faltando, arquitetura errada).
fn linha_app(o: &Json) -> Linha {
    let estado = o.str_ou_vazio("state");
    let versao = o.str("version").unwrap_or_default();
    let status_texto = match estado.as_str() {
        "installed" => format!("instalado ({versao})"),
        "broken" => "quebrado — não responde".to_string(),
        _ => "não instalado".to_string(),
    };
    let bin = o.str_ou_vazio("bin");
    Linha {
        slug: bin.clone(),
        display: bin,
        categoria: "app".into(),
        // App da casa não tem "método": ele compila do fonte, e só. A lista vazia é o que faz a
        // tela não desenhar chip de método nesta seção.
        metodos: Vec::new(),
        metodo_sel: String::new(),
        dica: o.str_ou_vazio("about"),
        instalado: estado == "installed",
        status_texto,
        titulo_secao: String::new(),
        op_rotulo: String::new(),
    }
}

/// **O quê:** todas as linhas da tela, na ordem em que ela as desenha.
///
/// **Onde:** a janela, depois de rodar `schematize-market list --json`.
///
/// **É PURA:** entra texto, sai lista. Nada aqui abre processo, e é isso que permite testar a
/// tela inteira sem ter o market instalado na máquina de quem roda a suíte.
///
/// **Documento ilegível é `Err`**, e a tela mostra o motivo. Devolver lista vazia diria "o
/// mercado não tem nada", que é uma afirmação — e falsa.
pub fn ler(texto: &str) -> Result<Vec<Linha>, String> {
    let j = crate::json::ler(texto).map_err(|e| format!("resposta do market ilegível: {e}"))?;
    let mut linhas = Vec::new();
    for (chave, monta) in
        [("languages", linha_env as fn(&Json) -> Linha), ("tools", linha_env), ("apps", linha_app)]
    {
        for (i, o) in j.arr(chave).iter().enumerate() {
            let mut l = monta(o);
            // A PRIMEIRA linha de cada seção abre o título; as outras não o repetem.
            if i == 0 {
                l.titulo_secao = titulo_secao(&l.categoria).to_string();
            }
            linhas.push(l);
        }
    }
    Ok(linhas)
}

/// **O quê:** o comando de shell que instala/remove/troca uma linguagem ou ferramenta.
///
/// **Onde:** [`crate::mercado`] → a janela, que o passa ao terminal.
///
/// **`gestor` tem de ser o caminho ABSOLUTO, e este é o bug que já apareceu em uso.** A janela
/// aberta pelo lançador do desktop tem **PATH mínimo** — sem `~/.cargo/bin` — e o terminal que
/// ela abre herda esse PATH. Com o nome puro, a pessoa clicava em instalar e recebia
/// `schematize-market: comando não encontrado` sobre um gestor que **estava** instalado.
/// Reproduzível com `env -i PATH=/usr/bin:/bin bash -c 'schematize-market --version'`.
///
/// **Sem `-y`:** o market mostra o que vai fazer e pede confirmação ali dentro. O que se
/// consente são minutos de CPU e, às vezes, a senha do sudo — vale mais que um clique a menos.
///
/// **O `read` no fim** segura o terminal aberto depois que o comando termina. Sem ele a janela
/// fecharia junto com o processo, e o erro que a pessoa precisa ler sumiria com ela.
pub fn comando(gestor: &str, acao: &str, alvo: &str, metodo: &str) -> String {
    let (rotulo, arg) = if metodo.is_empty() {
        (String::new(), String::new())
    } else {
        (format!(" ({metodo})"), format!(" --method {metodo}"))
    };
    format!(
        "echo '── {gestor} {acao} {alvo}{rotulo} ──'; echo; \
         {gestor} {acao} {alvo}{arg}; \
         echo; read -n1 -s -r -p '…'"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O contrato do `list --json`, como o market o emite hoje — copiado da saída real.
    const SAIDA: &str = r#"{
  "market": "0.2.1",
  "languages": [
    {"slug": "go", "display": "Go", "category": "language", "methods": ["docker", "mise", "distro", "official"], "installed_via": "mise", "present": true, "provenance": "mise", "hint": "docker, mise, distro, official", "status_text": "via mise"},
    {"slug": "csharp", "display": "C# / .NET", "category": "language", "methods": ["docker", "mise"], "installed_via": null, "present": false, "provenance": "absent", "hint": "docker, mise", "status_text": "não instalado"}
  ],
  "tools": [
    {"slug": "gh", "display": "GitHub CLI", "category": "tool", "methods": [], "installed_via": null, "present": true, "provenance": "distro", "hint": "distro", "status_text": "via distro (gh)"}
  ],
  "apps": [
    {"bin": "schematize-deployer", "about": "deploy e VPS", "state": "installed", "version": "0.7.2"},
    {"bin": "schematize-optimizer", "about": "limites da máquina", "state": "broken", "version": null},
    {"bin": "schematize-market", "about": "instalar e atualizar", "state": "absent", "version": null}
  ]
}"#;

    /// As três seções saem na ordem, e só a PRIMEIRA linha de cada uma abre título.
    #[test]
    fn tres_secoes_com_um_titulo_cada() {
        let l = ler(SAIDA).unwrap();
        assert_eq!(l.len(), 6);
        let titulos: Vec<&str> = l.iter().map(|x| x.titulo_secao.as_str()).collect();
        assert_eq!(
            titulos,
            ["Linguagens", "", "Ferramentas de dev", "Apps da casa", "", ""],
            "título repetido desenharia um cabeçalho por linha"
        );
    }

    /// **Cada linha traz o SEU dado.** É o bug que a busca por substring teria: `"slug"` casa
    /// dentro de qualquer item, e todas as linguagens mostrariam a primeira.
    #[test]
    fn cada_linha_traz_o_proprio_dado() {
        let l = ler(SAIDA).unwrap();
        assert_eq!(l[0].slug, "go");
        assert_eq!(l[0].display, "Go");
        assert_eq!(l[1].slug, "csharp");
        assert_eq!(l[1].display, "C# / .NET");
        assert_eq!(l[2].slug, "gh");
    }

    /// **A decisão sai do campo BOOLEANO, nunca da prosa.** Este é o teste que trava o bug
    /// original: o `status_text` muda de idioma, e nenhuma ramificação pode depender dele.
    #[test]
    fn o_instalado_vem_do_campo_e_nao_do_texto() {
        let l = ler(SAIDA).unwrap();
        assert!(l[0].instalado, "`present: true`");
        assert!(!l[1].instalado, "`present: false`");

        // O MESMO documento, com a prosa em outro idioma: as decisões não se movem.
        let outro = SAIDA
            .replace("via mise", "mise 経由")
            .replace("não instalado", "インストールされていません");
        let m = ler(&outro).unwrap();
        assert_eq!(m[0].instalado, l[0].instalado, "a decisão não pode depender do idioma");
        assert_eq!(m[1].instalado, l[1].instalado);
        assert_eq!(m[0].slug, l[0].slug);
        // E a prosa, essa sim, muda — senão o teste passaria por não haver nada traduzido.
        assert_ne!(m[0].status_texto, l[0].status_texto);
    }

    /// O método pré-selecionado é o que JÁ está instalado. Selecionar sempre o primeiro faria
    /// o botão propor trocar para o método que já está em uso.
    #[test]
    fn o_metodo_selecionado_e_o_instalado_quando_ha_um() {
        let l = ler(SAIDA).unwrap();
        assert_eq!(l[0].metodo_sel, "mise", "o `go` está instalado via mise");
        assert_eq!(l[1].metodo_sel, "docker", "sem instalado, o primeiro da lista");
        assert_eq!(l[2].metodo_sel, "", "ferramenta não tem método");
    }

    /// **O `state` dos apps tem três valores, e o do meio é o que importa.** Um binário que
    /// está lá e não responde é problema diferente de um que não existe.
    #[test]
    fn app_quebrado_nao_e_o_mesmo_que_app_ausente() {
        let l = ler(SAIDA).unwrap();
        let (instalado, quebrado, ausente) = (&l[3], &l[4], &l[5]);
        assert!(instalado.instalado);
        assert!(instalado.status_texto.contains("0.7.2"), "{:?}", instalado.status_texto);

        assert!(!quebrado.instalado);
        assert!(quebrado.status_texto.contains("quebrado"), "{:?}", quebrado.status_texto);
        assert_ne!(
            quebrado.status_texto, ausente.status_texto,
            "achatar os dois manda reinstalar o que já está lá"
        );

        assert!(!ausente.instalado);
        assert!(ausente.status_texto.contains("não instalado"));
    }

    /// App da casa não tem método — e a lista vazia é o que faz a tela não desenhar chips ali.
    #[test]
    fn app_da_casa_nao_tem_metodo() {
        let l = ler(SAIDA).unwrap();
        for a in &l[3..] {
            assert_eq!(a.categoria, "app");
            assert!(a.metodos.is_empty(), "app compila do fonte, e só: {a:?}");
        }
    }

    /// **O DEFEITO QUE SÓ APARECEU AO ABRIR A JANELA DE VERDADE.**
    ///
    /// O campo se chamava `status` e passou a `status_text`. Contra um market ainda não
    /// atualizado, a janela lia `status_text`, não achava nada, e desenhava uma pílula VAZIA —
    /// um oval colorido com nada dentro. Nenhum teste pegava, porque todos usavam o contrato
    /// novo.
    ///
    /// As duas metades de um sistema distribuído nunca se atualizam no mesmo instante. Uma
    /// janela que só funciona contra a versão do dia em que foi escrita está quebrada por
    /// desenho (§37.48).
    #[test]
    fn o_status_tem_texto_mesmo_contra_um_market_antigo() {
        // 1) o contrato de hoje.
        let novo = r#"{"languages":[{"slug":"go","present":true,"status_text":"via mise"}]}"#;
        assert_eq!(ler(novo).unwrap()[0].status_texto, "via mise");

        // 2) o nome ANTIGO — market ainda não atualizado. Era este o caso que dava a pílula
        //    vazia, e é o que se vê hoje numa máquina com o market publicado.
        let antigo = r#"{"languages":[{"slug":"go","present":true,"status":"via mise"}]}"#;
        assert_eq!(ler(antigo).unwrap()[0].status_texto, "via mise");

        // 3) NENHUM dos dois: deriva dos campos de decisão, que a janela já usa mesmo.
        let so_decisao = r#"{"languages":[
            {"slug":"go","present":true,"installed_via":"mise"},
            {"slug":"rust","present":true,"installed_via":null,"provenance":"distro"},
            {"slug":"zig","present":true},
            {"slug":"csharp","present":false}]}"#;
        let l = ler(so_decisao).unwrap();
        assert_eq!(l[0].status_texto, "via mise");
        assert_eq!(l[1].status_texto, "via distro", "cai no `provenance` quando não há método");
        assert_eq!(l[2].status_texto, "instalado", "sem via conhecida, ainda assim não é vazio");
        assert_eq!(l[3].status_texto, "não instalado");

        // A invariante que a pílula depende: NUNCA vazio. Uma pílula sem texto é um oval
        // colorido que não diz nada, e é pior que não desenhar pílula.
        for linha in &l {
            assert!(!linha.status_texto.is_empty(), "pílula vazia: {linha:?}");
        }
    }

    /// **JSON vazio, `null` e lixo não podem derrubar a janela.** Uma janela que morre ao abrir
    /// é pior que uma lista vazia: a pessoa não vê nem a mensagem de erro.
    #[test]
    fn entrada_hostil_nao_panica() {
        // Documento válido e vazio: lista vazia, e isso é um fato — o gestor respondeu.
        assert_eq!(ler("{}").unwrap().len(), 0);
        assert_eq!(ler(r#"{"languages":[],"tools":[],"apps":[]}"#).unwrap().len(), 0);
        // Itens sem os campos esperados: linhas com campos vazios, nunca pânico.
        let l = ler(r#"{"languages":[{},{"slug":null},{"methods":"nao e lista"}]}"#).unwrap();
        assert_eq!(l.len(), 3);
        assert!(l.iter().all(|x| x.slug.is_empty() || x.slug == "null"));
        assert!(l.iter().all(|x| !x.instalado), "sem `present` não se afirma instalado");
        // Ilegível é Err — dizer "o mercado não tem nada" seria uma afirmação, e falsa.
        for lixo in ["", "isto nao e json", r#"{"languages":["#] {
            assert!(ler(lixo).is_err(), "{lixo:?}");
        }
    }

    /// **O COMANDO USA O CAMINHO ABSOLUTO.** A janela aberta pelo lançador do desktop tem PATH
    /// mínimo, e o terminal herda esse PATH: com o nome puro, quem clicava em instalar recebia
    /// `schematize-market: comando não encontrado` sobre um gestor instalado.
    #[test]
    fn o_comando_usa_o_caminho_absoluto_do_gestor() {
        let c = comando("/home/u/.cargo/bin/schematize-market", "install", "go", "mise");
        assert!(c.contains("/home/u/.cargo/bin/schematize-market install go --method mise"), "{c}");
        // Self-check: com o nome puro a asserção acima falharia. Um teste que não sabe
        // reprovar não prova nada.
        let ruim = comando("schematize-market", "install", "go", "mise");
        assert!(!ruim.contains("/home/u/.cargo/bin/"), "o self-check parou de valer");
    }

    /// O eco que a pessoa lê no topo do terminal mostra o MESMO comando que roda. Se ele
    /// dissesse o nome puro e executasse o caminho, o erro que ela copiasse para pedir ajuda
    /// seria sobre um comando que ninguém rodou.
    #[test]
    fn o_eco_e_o_comando_executado_sao_o_mesmo() {
        let g = "/opt/x/schematize-market";
        let c = comando(g, "remove", "go", "");
        assert_eq!(c.matches(&format!("{g} remove go")).count(), 2, "{c}");
    }

    /// Sem método, nenhum `--method` vazio é passado — o gestor receberia uma flag sem valor.
    #[test]
    fn sem_metodo_nao_ha_flag_pendurada() {
        let c = comando("/b/m", "install", "gh", "");
        assert!(!c.contains("--method"), "{c}");
        assert!(c.contains("/b/m install gh;"), "{c}");
    }

    /// O terminal não fecha na cara de quem clicou — o erro tem de sobreviver ao comando.
    #[test]
    fn o_terminal_espera_uma_tecla_no_fim() {
        assert!(comando("/b/m", "install", "go", "mise").contains("read -n1"));
    }
}
