//! Abrir um TERMINAL do sistema — a parte de plataforma, e nada de domínio.
//!
//! **O quê:** roda um comando de shell num emulador de terminal gráfico, desacoplado desta
//! janela.
//!
//! **Onde:** as ações da aba do Mercado (instalar, remover, trocar método).
//!
//! ## Por que terminal, e não uma barra de progresso dentro da janela
//!
//! Instalar uma linguagem **compila do fonte por minutos**, pode pedir sudo para as libs de
//! build, e o gestor fala o tempo todo. Fazer isso no event loop travaria a janela; fazer numa
//! thread com barra desenhada exigiria reimplementar, em Slint, o que um terminal já faz melhor
//! — mostrar a saída ao vivo, aceitar `Ctrl-C`, e deixar o erro na tela para ser lido e
//! copiado. Progresso, cancelamento e mensagem acionável vêm de graça, **e são de verdade**.
//!
//! É a mesma decisão que o hub tomou nesta mesma tela, e o D6 do overdev a manteve.
//!
//! ## Por que esta cópia existe, e o que a torna legítima
//!
//! O hub tem estas ~40 linhas em `sysenv.rs`. Copiá-las é o que o D4 chama de **plataforma**:
//! a cópia não sabe o que é linguagem, método ou app. O que ela sabe do domínio entra por
//! parâmetro, como string já montada por [`crate::mercado::comando`].
//!
//! **Se um dia algo de domínio migrar para cá, o corte foi feito errado.**

/// **O quê:** um binário existe no `$PATH`? **Onde:** [`abrir`].
///
/// Via `command -v` num shell, e não varrendo o `$PATH` à mão: o shell já sabe de alias, de
/// função e do `PATH` efetivo, e esta janela é `std`-only — reimplementar isso seria mais
/// código para acertar menos.
fn existe(cmd: &str) -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {cmd}"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Emuladores tentados, em ordem, com a flag que cada um usa para receber o comando.
///
/// **`x-terminal-emulator` primeiro** porque em Debian/Ubuntu ele é o alternativo escolhido
/// pela pessoa — respeitar essa escolha vale mais que adivinhar. O `kitty` não tem flag: ele
/// recebe o comando direto.
const TERMINAIS: &[(&str, &[&str])] = &[
    ("x-terminal-emulator", &["-e"]),
    ("konsole", &["-e"]),
    ("gnome-terminal", &["--"]),
    ("xfce4-terminal", &["-x"]),
    ("tilix", &["-e"]),
    ("kitty", &[]),
    ("alacritty", &["-e"]),
    ("xterm", &["-e"]),
];

/// **O quê:** abre `comando` no primeiro terminal disponível. `false` se não houver nenhum.
///
/// **Onde:** as ações da aba do Mercado.
///
/// **`false` NÃO é o fim do caminho**, e é por isso que ele é `bool` e não `panic`: quando não
/// há terminal, a janela entrega o comando para a pessoa rodar onde quiser. Sumir com o
/// problema é o que o §37.48 chama de bug do software — a janela se adapta ao que a máquina
/// tem, em vez de exigir que a pessoa saiba por que nada aconteceu.
pub fn abrir(comando: &str) -> bool {
    for (term, pre) in TERMINAIS {
        if existe(term)
            && std::process::Command::new(term)
                .args(*pre)
                .arg("bash")
                .arg("-c")
                .arg(comando)
                .spawn()
                .is_ok()
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lista de terminais está sã: nenhum nome vazio, nenhum duplicado, e o `kitty` — o
    /// único sem flag — continua sem flag. Um nome duplicado faria o segundo nunca ser
    /// tentado, e um vazio tentaria executar o programa "".
    #[test]
    fn a_lista_de_terminais_esta_sa() {
        let nomes: Vec<&str> = TERMINAIS.iter().map(|(n, _)| *n).collect();
        assert!(nomes.iter().all(|n| !n.is_empty()));
        let mut ordenados = nomes.clone();
        ordenados.sort_unstable();
        ordenados.dedup();
        assert_eq!(ordenados.len(), nomes.len(), "terminal duplicado nunca é tentado duas vezes");
        assert_eq!(nomes[0], "x-terminal-emulator", "a escolha da pessoa vem primeiro");
        let kitty = TERMINAIS.iter().find(|(n, _)| *n == "kitty").unwrap();
        assert!(kitty.1.is_empty(), "o kitty recebe o comando direto, sem flag");
    }

    /// `existe` responde `false` para o que não existe, sem entrar em pânico e sem imprimir
    /// nada na saída desta janela.
    #[test]
    fn binario_inexistente_e_false_e_nao_panico() {
        assert!(!existe("terminal-que-nao-existe-em-lugar-nenhum-42"));
        assert!(existe("sh"), "`sh` existe em qualquer unix");
    }
}
