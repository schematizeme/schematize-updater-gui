# schematize-market-gui — a janela do gestor

A janela (Slint) do gestor de atualizações da casa. Mostra versões e plataforma, instala e
atualiza com **progresso ao vivo**, e abre o app — para quem não quer um terminal.

```
schematize-market-gui        # abre a janela
```

## O repositório se chama `updater-gui`; o binário, `market-gui`

Não é descuido, e a distinção importa.

Esta janela nasceu como a janela do `schematize-updater`. Aquele binário foi **absorvido pelo
`schematize-market`** (ADR-0013), que passou a ser o único responsável por instalar e
atualizar. O ADR-0014 (D4) decidiu que a janela **não** é descontinuada: o que a quebrou foi o
dono ter mudado, não ela — o problema que ela resolve (atualizar sem terminal) continua real.

- **O binário virou `schematize-market-gui`.** Nome de binário que não bate com o dono é
  exatamente o que deixou o update do deployer morto por um release inteiro: a função procurava
  `deployer` depois de o binário virar `schematize-deployer`, e ninguém percebeu — porque nada
  dá erro nesse caso, ele só para de funcionar.
- **O repositório continua `schematize-updater-gui`.** Ele é endereço: está em documentação, em
  bookmark e no histórico de quem clonou. Renomear repo é operação de plataforma, com
  redirecionamento, e é chamada humana — não a de um refactor.

Quem tem o binário antigo na máquina não fica com dois: `schematize-updater-gui` está na lista
de nomes aposentados da purga do market, que o remove dizendo o que removeu.

## Ela lê `status --json`, e isso é o ponto

O `status` humano do market passa pelo catálogo i18n: os rótulos são `plataforma` em português,
`platform` em inglês, `プラットフォーム` em japonês. Esta janela casava o rótulo **em
português** — então lia certo num idioma e devolvia **tudo vazio nos outros dezenove, sem erro
nenhum**. Com os campos vazios, ela afirmava *"app não instalado"* a quem tinha o app, e o
botão "Instalar" não resolvia nada.

Agora ela consome `schematize-market status --json`, cujas **chaves nunca são traduzidas** —
é o que as torna contrato. Parsear saída feita para humano é contrato de mentira: passa no
teste de quem escreveu e falha na máquina de quem usa.

## Por que ela é `std`-only

Sem `serde`, sem crate de JSON, sem o crate `schematize`. Esta é a interface que tem de abrir
**quando o resto está quebrado** — primeira instalação, app corrompido, toolchain incompleto.
Cada dependência que ela ganha é uma chance a mais de ela não compilar justamente na máquina
onde ela é a única coisa que funciona.

## Distribuição

Sai como **asset do release do `schematize-market`** (ADR-0014 D5), ao lado do binário do
próprio market — o dono do binário é o dono da janela dele. Até esta decisão ela era o único
binário da casa sem caminho de binário pronto: compilava do fonte em toda máquina, sempre.
