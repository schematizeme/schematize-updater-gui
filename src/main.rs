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

use std::cell::RefCell;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

mod gestor;
mod json;
mod mercado;
mod terminal;

slint::include_modules!();

use gestor::{gestor_bin, read_status, run_streaming, Status};

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

/// **O quê:** a aba em que a janela abre, lida da linha de comando.
///
/// **Onde:** [`main`]. `--mercado` abre no Mercado; sem argumento, em Atualizações.
///
/// **Por que existe:** a aba "Mercado" do hub abre ESTA janela. Sem o argumento, quem clica
/// em "Mercado" lá cai em "Atualizações" aqui e tem de clicar de novo — um clique a mais para
/// chegar onde já tinha pedido para ir. O ícone do desktop, esse, abre sem argumento: quem
/// clica nele não pediu tela nenhuma em especial, e a de atualizações é a que responde à
/// pergunta mais comum.
///
/// **Argumento desconhecido não é erro.** Esta janela é a interface que abre quando o resto
/// está quebrado; sair com erro por causa de uma flag que alguém digitou errado seria trocar
/// uma janela que funciona por nenhuma.
fn aba_inicial(args: impl Iterator<Item = String>) -> i32 {
    for a in args {
        if a == "--mercado" || a == "--market" {
            return 1;
        }
    }
    0
}

fn main() -> Result<(), slint::PlatformError> {
    let w = MainWindow::new()?;
    let aba = aba_inicial(std::env::args().skip(1));
    w.set_aba(aba);

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

    // ---- ABA MERCADO ----
    //
    // O estado das linhas vive num `Rc<RefCell<…>>` compartilhado entre os quatro callbacks:
    // escolher método muda uma linha, instalar/remover leem a linha escolhida, e recarregar
    // troca a lista inteira. Sem um dono único, cada callback teria a sua cópia e o método
    // escolhido num não existiria no outro.
    let linhas: Rc<RefCell<Vec<mercado::Linha>>> = Rc::new(RefCell::new(Vec::new()));

    {
        let weak = w.as_weak();
        let linhas = linhas.clone();
        w.on_mercado_recarregar(move || {
            let Some(w) = weak.upgrade() else { return };
            w.set_mercado_carregando(true);
            let r = ler_mercado();
            match r {
                Ok(v) => {
                    w.set_mercado_erro(slint::SharedString::new());
                    *linhas.borrow_mut() = v;
                }
                Err(e) => {
                    // O erro vira TELA, e a lista fica vazia — mas quem desenha olha o erro
                    // primeiro. Lista vazia sem erro diria "o mercado não tem nada", que é uma
                    // afirmação, e falsa.
                    w.set_mercado_erro(e.into());
                    linhas.borrow_mut().clear();
                }
            }
            aplicar_linhas(&w, &linhas.borrow());
            w.set_mercado_carregando(false);
        });
    }

    {
        let weak = w.as_weak();
        let linhas = linhas.clone();
        w.on_mercado_escolher_metodo(move |i, m| {
            let Some(w) = weak.upgrade() else { return };
            {
                let mut v = linhas.borrow_mut();
                // Índice fora da lista é possível: a tela pode ter sido redesenhada entre o
                // clique e este callback. `get_mut` devolve `None` em vez de panicar — e uma
                // janela que morre no clique é pior que um clique que não faz nada.
                let Some(l) = v.get_mut(i as usize) else { return };
                l.metodo_sel = m.to_string();
            }
            aplicar_linhas(&w, &linhas.borrow());
        });
    }

    {
        let weak = w.as_weak();
        let linhas = linhas.clone();
        w.on_mercado_instalar(move |i| {
            let Some(w) = weak.upgrade() else { return };
            acao_de_mercado(&w, &linhas, i, "install");
        });
    }

    {
        let weak = w.as_weak();
        let linhas = linhas.clone();
        w.on_mercado_remover(move |i| {
            let Some(w) = weak.upgrade() else { return };
            acao_de_mercado(&w, &linhas, i, "remove");
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

    // Aberta direto no Mercado (o hub passa `--mercado`): a leitura é disparada aqui, porque
    // quem a dispararia é o CLIQUE na aba — e esse clique não vai acontecer. Sem isto a tela
    // nasceria vazia, e vazia sem erro é a janela dizendo "o mercado não tem nada".
    if aba == 1 {
        w.invoke_mercado_recarregar();
    }

    w.run()
}

/// **O quê:** roda `schematize-market list --json` e devolve as linhas da aba do Mercado.
///
/// **Onde:** o callback de recarregar.
///
/// **Os dois modos de falha viram a MESMA tela, e ambos dizem o que houve:** não consegui
/// executar o gestor, e executei mas não entendi a resposta. Nenhum deles pode virar "lista
/// vazia" — a pessoa leria isso como "o mercado não tem nada", que é uma afirmação, e falsa.
/// É a mesma distinção entre "não sei" e "não tem" que já quebrou esta janela uma vez.
fn ler_mercado() -> Result<Vec<mercado::Linha>, String> {
    let bin = gestor_bin();
    let out = Command::new(&bin)
        .args(["list", "--json"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("não consegui executar {}: {e}", bin.display()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        return Err(format!(
            "{} list falhou ({}){}",
            bin.display(),
            out.status,
            if err.is_empty() { String::new() } else { format!(": {err}") }
        ));
    }
    mercado::ler(&String::from_utf8_lossy(&out.stdout))
}

/// **O quê:** joga as linhas do Rust para o modelo que a tela desenha.
///
/// **Onde:** depois de recarregar, de escolher método e de disparar uma ação.
///
/// Reconstrói o modelo inteiro em vez de mexer numa linha só: são algumas dezenas de itens, e
/// um modelo que se atualiza por partes é onde nasce a divergência entre o que o Rust acha que
/// está na tela e o que está.
fn aplicar_linhas(w: &MainWindow, linhas: &[mercado::Linha]) {
    let modelo: Vec<LinhaMercado> = linhas
        .iter()
        .map(|l| LinhaMercado {
            slug: l.slug.clone().into(),
            display: l.display.clone().into(),
            categoria: l.categoria.clone().into(),
            metodos: slint::ModelRc::new(slint::VecModel::from(
                l.metodos.iter().map(slint::SharedString::from).collect::<Vec<_>>(),
            )),
            metodo_sel: l.metodo_sel.clone().into(),
            dica: l.dica.clone().into(),
            instalado: l.instalado,
            status_texto: l.status_texto.clone().into(),
            titulo_secao: l.titulo_secao.clone().into(),
            op_rotulo: l.op_rotulo.clone().into(),
        })
        .collect();
    w.set_mercado_linhas(slint::ModelRc::new(slint::VecModel::from(modelo)));
}

/// **O quê:** dispara `install`/`remove` de uma linha num TERMINAL, e marca a linha com o que
/// aconteceu.
///
/// **Onde:** os dois botões da aba do Mercado.
///
/// **Sem terminal a janela NÃO some com o problema:** ela põe o comando na própria linha, para
/// a pessoa rodar onde quiser. Um botão que não faz nada e não diz nada é o que o §37.48 chama
/// de bug do software.
fn acao_de_mercado(w: &MainWindow, linhas: &Rc<RefCell<Vec<mercado::Linha>>>, i: i32, acao: &str) {
    let gestor = gestor_bin().display().to_string();
    let rotulo = {
        let mut v = linhas.borrow_mut();
        let Some(l) = v.get_mut(i as usize) else { return };
        let cmd = mercado::comando(&gestor, acao, &l.slug, &l.metodo_sel);
        if terminal::abrir(&cmd) {
            "terminal aberto".to_string()
        } else {
            let metodo = if l.metodo_sel.is_empty() {
                String::new()
            } else {
                format!(" --method {}", l.metodo_sel)
            };
            format!("rode: {gestor} {acao} {}{metodo}", l.slug)
        }
    };
    {
        let mut v = linhas.borrow_mut();
        if let Some(l) = v.get_mut(i as usize) {
            l.op_rotulo = rotulo;
        }
    }
    aplicar_linhas(w, &linhas.borrow());
}

#[cfg(test)]
mod tests_aba {
    use super::aba_inicial;

    /// Sem argumento, a janela abre em Atualizações — é a tela que responde à pergunta mais
    /// comum de quem clicou no ícone sem pedir nada em especial.
    #[test]
    fn sem_argumento_abre_em_atualizacoes() {
        assert_eq!(aba_inicial(std::iter::empty()), 0);
    }

    /// `--mercado` abre no Mercado. É o que o hub passa quando alguém clica na aba de lá: sem
    /// isto, a pessoa pediria "Mercado" e cairia em "Atualizações".
    #[test]
    fn mercado_abre_no_mercado() {
        for flag in ["--mercado", "--market"] {
            assert_eq!(aba_inicial([flag.to_string()].into_iter()), 1, "{flag}");
        }
        // Entre outros argumentos, também.
        assert_eq!(aba_inicial(["-x".to_string(), "--mercado".to_string()].into_iter()), 1);
    }

    /// **Argumento desconhecido NÃO derruba a janela.** Ela é a interface que abre quando o
    /// resto está quebrado; sair com erro por uma flag digitada errado seria trocar uma janela
    /// que funciona por nenhuma.
    #[test]
    fn argumento_desconhecido_nao_derruba_nada() {
        assert_eq!(aba_inicial(["--nao-existe".to_string()].into_iter()), 0);
        assert_eq!(aba_inicial(["".to_string(), "-".to_string()].into_iter()), 0);
    }
}
