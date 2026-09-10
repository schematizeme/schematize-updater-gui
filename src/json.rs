//! Um leitor de JSON pequeno, `std`-only — a metade da janela que não toca em processo nenhum.
//!
//! **O quê:** transforma o texto de um `--json` do market numa árvore que dá para consultar.
//!
//! **Onde:** [`crate::gestor`] (o `status --json`) e [`crate::mercado`] (o `list --json`).
//!
//! ## Por que um leitor à mão, e não `serde_json`
//!
//! Esta janela é `std`-only **de propósito**. Ela é a interface que tem de abrir **quando o
//! resto está quebrado** — primeira instalação, app corrompido, toolchain incompleto. Cada
//! dependência que ela ganha é uma chance a mais de ela não compilar justamente na máquina
//! onde ela é a única coisa que funciona.
//!
//! ## O que mudou, e por quê
//!
//! A versão anterior não era um leitor: eram duas funções que procuravam `"chave":` no texto
//! e liam até a próxima aspa. Aquilo bastava para o `status --json`, cujo contrato tem nove
//! chaves de topo, todas planas.
//!
//! O `list --json` não é plano — são três listas de objetos. Continuar procurando substring ali
//! daria errado de um jeito **silencioso**: `"slug"` casa dentro de qualquer um dos objetos, e
//! a busca devolveria o primeiro de todos como se fosse o do item que se está lendo. Uma lista
//! de vinte linguagens em que todas mostram o nome da primeira é o tipo de bug que passa em
//! revisão e aparece em uso.
//!
//! ## O contrato deste módulo com quem o chama
//!
//! **Nunca entra em pânico**, e é a razão de tudo aqui devolver `Option`/`Result`. Uma janela
//! que morre ao abrir é pior que uma lista vazia: a pessoa não vê nem a mensagem de erro. Todo
//! caminho de falha vira "não consegui ler", que a tela sabe mostrar.
//!
//! Ele também **não tenta ser um parser completo de JSON**: `\uXXXX` não é decodificado (o
//! contrato não emite), e números viram `f64`. Ler o shape que o market emite custa este
//! arquivo; ler JSON arbitrário custaria uma dependência.

/// Um valor JSON, no mínimo que os contratos do market usam.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Obj(Vec<(String, Json)>),
    Arr(Vec<Json>),
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
}

impl Json {
    /// **O quê:** o valor de uma chave, se isto for objeto. `None` para qualquer outra coisa.
    /// **Onde:** toda leitura de campo nos dois consumidores.
    ///
    /// Busca linear numa lista de pares, e não um mapa: os objetos do contrato têm menos de
    /// quinze chaves, e um `HashMap` por linha custaria mais alocação do que a busca economiza.
    pub fn get(&self, chave: &str) -> Option<&Json> {
        match self {
            Json::Obj(pares) => pares.iter().find(|(k, _)| k == chave).map(|(_, v)| v),
            _ => None,
        }
    }

    /// **O quê:** a string de uma chave. `None` se ausente, `null`, ou de outro tipo.
    ///
    /// **Onde:** os campos de texto dos dois contratos.
    ///
    /// **`null` devolve `None`, e isso é o ponto.** É como o market diz "não há pin" e "não sei
    /// a versão"; tratá-lo como a string `"null"` poria a palavra na tela. Confundir "não sei"
    /// com "vazio" foi exatamente o bug que fez esta janela afirmar "app não instalado" a quem
    /// tinha o app.
    pub fn str(&self, chave: &str) -> Option<&str> {
        match self.get(chave)? {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    /// **O quê:** a string de uma chave, ou `""` quando ausente/`null`.
    /// **Onde:** os campos que a tela mostra direto, onde vazio já é o que se quer desenhar.
    pub fn str_ou_vazio(&self, chave: &str) -> String {
        self.str(chave).unwrap_or_default().to_string()
    }

    /// **O quê:** o booleano de uma chave. `None` se ausente ou de outro tipo.
    /// **Onde:** `prebuilt`, `present`, e os do deployer.
    pub fn bool(&self, chave: &str) -> Option<bool> {
        match self.get(chave)? {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// **O quê:** os itens de uma lista. Lista vazia se a chave não existe ou não é lista.
    ///
    /// **Onde:** `languages`, `tools`, `apps`, `methods`.
    ///
    /// **Vazio em vez de `Option`** porque, para quem desenha, "não veio" e "veio vazia" dão a
    /// mesma tela — e obrigar cada chamador a tratar os dois casos multiplicaria caminhos que
    /// ninguém testa.
    pub fn arr(&self, chave: &str) -> &[Json] {
        match self.get(chave) {
            Some(Json::Arr(v)) => v,
            _ => &[],
        }
    }

    /// **O quê:** a própria string, se isto for uma. **Onde:** os itens de `methods`, que são
    /// strings soltas dentro da lista.
    pub fn como_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// **O quê:** lê um documento inteiro. `Err` com o motivo se não for JSON válido.
///
/// **Onde:** [`crate::gestor::read_status`] e [`crate::mercado::ler`].
///
/// **Exige que o documento ACABE onde diz acabar.** Sobra depois do `}` significa que alguma
/// coisa foi impressa junto — um cabeçalho humano, um aviso — e ler só a primeira metade
/// silenciosamente é como a janela passaria a mostrar um estado que não existe.
pub fn ler(txt: &str) -> Result<Json, String> {
    let b = txt.as_bytes();
    let mut i = 0;
    let v = valor(b, &mut i)?;
    pular_espaco(b, &mut i);
    if i < b.len() {
        return Err(format!("sobrou texto depois do JSON (byte {i})"));
    }
    Ok(v)
}

/// **O quê:** anda enquanto houver espaço em branco. **Onde:** cada passo de [`valor`].
fn pular_espaco(b: &[u8], i: &mut usize) {
    while *i < b.len() && b[*i].is_ascii_whitespace() {
        *i += 1;
    }
}

/// **O quê:** lê UM valor a partir de `i`, deixando `i` logo depois dele.
/// **Onde:** [`ler`], e ela mesma, recursivamente.
///
/// **A profundidade é limitada** ([`PROFUNDIDADE_MAX`]): sem isso, um documento com dez mil
/// colchetes abertos estouraria a pilha, e estouro de pilha não é `Err` — é a janela sumindo
/// da tela sem mensagem nenhuma. O contrato do market tem profundidade três.
fn valor(b: &[u8], i: &mut usize) -> Result<Json, String> {
    valor_em(b, i, 0)
}

/// Profundidade máxima de aninhamento aceita. O contrato mais fundo do market tem três níveis
/// (documento → lista → objeto); trinta e dois é folga larga e ainda longe de estourar a pilha.
const PROFUNDIDADE_MAX: u32 = 32;

fn valor_em(b: &[u8], i: &mut usize, prof: u32) -> Result<Json, String> {
    if prof > PROFUNDIDADE_MAX {
        return Err("JSON aninhado demais".into());
    }
    pular_espaco(b, i);
    let Some(&c) = b.get(*i) else {
        return Err("documento termina onde devia haver valor".into());
    };
    match c {
        b'{' => objeto(b, i, prof),
        b'[' => lista(b, i, prof),
        b'"' => texto(b, i).map(Json::Str),
        _ => literal(b, i),
    }
}

/// **O quê:** lê `{ "k": v, ... }`. **Onde:** [`valor_em`].
fn objeto(b: &[u8], i: &mut usize, prof: u32) -> Result<Json, String> {
    *i += 1; // o `{`
    let mut pares = Vec::new();
    loop {
        pular_espaco(b, i);
        match b.get(*i) {
            None => return Err("objeto sem `}`".into()),
            Some(b'}') => {
                *i += 1;
                return Ok(Json::Obj(pares));
            }
            Some(b',') => {
                *i += 1;
                continue;
            }
            Some(b'"') => {}
            Some(outro) => return Err(format!("esperava chave, veio `{}`", *outro as char)),
        }
        let k = texto(b, i)?;
        pular_espaco(b, i);
        if b.get(*i) != Some(&b':') {
            return Err(format!("faltou `:` depois de `{k}`"));
        }
        *i += 1;
        let v = valor_em(b, i, prof + 1)?;
        pares.push((k, v));
    }
}

/// **O quê:** lê `[ v, ... ]`. **Onde:** [`valor_em`].
fn lista(b: &[u8], i: &mut usize, prof: u32) -> Result<Json, String> {
    *i += 1; // o `[`
    let mut itens = Vec::new();
    loop {
        pular_espaco(b, i);
        match b.get(*i) {
            None => return Err("lista sem `]`".into()),
            Some(b']') => {
                *i += 1;
                return Ok(Json::Arr(itens));
            }
            Some(b',') => {
                *i += 1;
                continue;
            }
            _ => itens.push(valor_em(b, i, prof + 1)?),
        }
    }
}

/// **O quê:** lê uma string entre aspas, resolvendo os escapes do contrato.
/// **Onde:** [`valor_em`] e [`objeto`] (as chaves).
///
/// **`\uXXXX` não é decodificado**, e sim mantido literal: o contrato não o emite, e
/// implementar pares substitutos de UTF-16 aqui seria escrever a parte mais errável de um
/// parser para um caso que não existe. Se um dia existir, é o teste que avisa.
fn texto(b: &[u8], i: &mut usize) -> Result<String, String> {
    *i += 1; // a aspa de abertura
    let ini = *i;
    let mut escapou = false;
    while *i < b.len() {
        match b[*i] {
            b'\\' => {
                escapou = true;
                *i += 2;
            }
            b'"' => {
                let bruto = &b[ini..*i];
                *i += 1;
                let s = std::str::from_utf8(bruto)
                    .map_err(|_| "string com bytes que não são UTF-8".to_string())?;
                return Ok(if escapou { desescapar(s) } else { s.to_string() });
            }
            _ => *i += 1,
        }
    }
    Err("string sem aspa de fechamento".into())
}

/// **O quê:** resolve os escapes de uma string já delimitada. **Onde:** [`texto`].
fn desescapar(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            // `\\`, `\"`, `\/` e o `\u` não decodificado caem aqui: o caractere vai como está.
            Some(outro) => out.push(outro),
            None => out.push('\\'),
        }
    }
    out
}

/// **O quê:** lê `true`, `false`, `null` ou um número. **Onde:** [`valor_em`].
fn literal(b: &[u8], i: &mut usize) -> Result<Json, String> {
    let ini = *i;
    while *i < b.len() && !b",}] \n\t\r".contains(&b[*i]) {
        *i += 1;
    }
    let lit = std::str::from_utf8(&b[ini..*i]).map_err(|_| "literal ilegível".to_string())?;
    match lit {
        "true" => Ok(Json::Bool(true)),
        "false" => Ok(Json::Bool(false)),
        "null" => Ok(Json::Null),
        "" => Err("valor vazio".into()),
        n => n.parse::<f64>().map(Json::Num).map_err(|_| format!("`{n}` não é valor JSON")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O shape que o `list --json` emite — objeto com listas de objetos — é lido campo a
    /// campo, e **cada item traz o SEU valor**. É o bug que a busca por substring teria:
    /// `"slug"` casa dentro de qualquer objeto da lista, e todos mostrariam o primeiro.
    #[test]
    fn cada_item_da_lista_traz_o_proprio_valor() {
        let j = ler(r#"{"market":"0.2.1","languages":[
                 {"slug":"go","present":true},
                 {"slug":"rust","present":false}]}"#)
        .unwrap();
        assert_eq!(j.str("market"), Some("0.2.1"));
        let langs = j.arr("languages");
        assert_eq!(langs.len(), 2);
        assert_eq!(langs[0].str("slug"), Some("go"));
        assert_eq!(langs[1].str("slug"), Some("rust"), "o segundo item não pode ler o primeiro");
        assert_eq!(langs[0].bool("present"), Some(true));
        assert_eq!(langs[1].bool("present"), Some(false));
    }

    /// `null` é `None`, e **diferente** de `""`. É como o market diz "não há pin"; tratá-lo
    /// como a string `"null"` poria a palavra na tela.
    #[test]
    fn null_e_ausencia_nao_sao_string() {
        let j = ler(r#"{"pin":null,"vazio":"","n":1}"#).unwrap();
        assert_eq!(j.str("pin"), None, "`null` não é string");
        assert_eq!(j.str("vazio"), Some(""), "string vazia É string");
        assert_eq!(j.str("ausente"), None);
        assert_eq!(j.str("n"), None, "número não é string");
        // E o atalho que a tela usa achata os dois primeiros, de propósito.
        assert_eq!(j.str_ou_vazio("pin"), "");
        assert_eq!(j.str_ou_vazio("vazio"), "");
    }

    /// Lista ausente vira lista vazia — quem desenha trata os dois casos igual.
    #[test]
    fn lista_ausente_e_lista_vazia_dao_a_mesma_tela() {
        let j = ler(r#"{"a":[]}"#).unwrap();
        assert!(j.arr("a").is_empty());
        assert!(j.arr("nao_existe").is_empty());
        // E um campo que não é lista também: pedir `arr` de um número é erro de quem chama,
        // e devolver vazio evita que ele vire pânico na máquina do usuário.
        let j = ler(r#"{"a":1}"#).unwrap();
        assert!(j.arr("a").is_empty());
    }

    /// Escapes do contrato: aspas e barras invertidas (o `install_dir` do Windows).
    #[test]
    fn escapes_do_contrato_sao_resolvidos() {
        let j = ler(r#"{"p":"C:\\Users\\u\\.cargo\\bin","q":"diz \"oi\"","r":"a\nb"}"#).unwrap();
        assert_eq!(j.str("p"), Some(r"C:\Users\u\.cargo\bin"));
        assert_eq!(j.str("q"), Some(r#"diz "oi""#));
        assert_eq!(j.str("r"), Some("a\nb"));
    }

    /// **Nada aqui pode entrar em pânico.** Uma janela que morre ao abrir é pior que uma lista
    /// vazia: a pessoa não vê nem a mensagem de erro. Todo lixo vira `Err`.
    #[test]
    fn lixo_vira_erro_e_nunca_panico() {
        for ruim in [
            "",
            "   ",
            "isto nao e json",
            r#"{"a":"#,
            r#"{"a""#,
            r#"{"a" 1}"#,
            r#"{"a":1"#,
            r#"["#,
            r#"{"a":"sem aspa}"#,
            r#"{"a":vazio}"#,
            // Sobra depois do documento: alguém imprimiu junto.
            r#"{"a":1} e mais um aviso"#,
        ] {
            assert!(ler(ruim).is_err(), "devia ser Err: {ruim:?}");
        }
    }

    /// Aninhamento absurdo vira `Err`, não estouro de pilha — que não é `Err`, é a janela
    /// sumindo da tela sem mensagem nenhuma.
    #[test]
    fn aninhamento_absurdo_e_erro_e_nao_estouro_de_pilha() {
        let fundo = "[".repeat(5000);
        assert!(ler(&fundo).is_err());
        let fundo = format!("{}{}", "[".repeat(5000), "]".repeat(5000));
        assert!(ler(&fundo).is_err());
    }

    /// O documento inteiro do `status --json`, como o market o emite hoje.
    #[test]
    fn le_o_contrato_de_status_inteiro() {
        let j = ler(r#"{
  "market": "0.2.0",
  "os": "linux",
  "arch": "x86_64",
  "prebuilt": true,
  "app_installed": "0.55.0",
  "app_latest": "0.55.2",
  "pin": null,
  "install_dir": "/home/u/.cargo/bin",
  "apps": [
    {"bin": "schematize-deployer", "installed": "0.5.0", "latest": "0.5.0"}
  ]
}"#)
        .unwrap();
        assert_eq!(j.str("market"), Some("0.2.0"));
        assert_eq!(j.bool("prebuilt"), Some(true));
        assert_eq!(j.str("pin"), None);
        assert_eq!(j.arr("apps").len(), 1);
        assert_eq!(j.arr("apps")[0].str("bin"), Some("schematize-deployer"));
    }
}
