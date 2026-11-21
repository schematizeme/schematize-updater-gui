# BUDGET — orçamento de memória do schematize-market-gui (casca Slint)

Piso de eficiência de runtime da casa (`eficiencia-runtime.md` §1). O teste
`tests/orcamento_rss.rs` abre a janela e falha acima do teto. Subir o teto exige nova medição e
justificativa aqui. Teto de GUI da casa: 400 MiB; este fica muito abaixo.

| Operação | RSS pico alvo (teto) | Medido antes | Medido depois | Como medir |
|---|---|---|---|---|
| Janela aberta, release (idle, 6 s, Wayland) | n/a (informativo) | 16,4 MiB (binário instalado) | 16,4 MiB (caminho de render intocado) | polling de `VmHWM` |
| Janela aberta, build de teste (debug) | 48 MiB | 31,9 MiB | 31,9 MiB | `cargo test --test orcamento_rss -- --nocapture` |
| Streaming do log do `install`/`update` | cauda de 20 000 bytes por construção | n/d | n/d | teste `cauda_do_log_corta_em_fronteira_utf8` |

Notas: o teste roda o binário debug (31,9 MiB medido); o teto de 48 MiB deixa folga para variação de driver GL/Mesa entre máquinas (a medição é
de uma máquina). O RSS do gestor (`schematize-market`) é processo filho e tem orçamento próprio.
Sem `/proc` ou sem display o teste imprime `PULADO` explicitamente.

## Diagnóstico

- `slint` já está com `default-features = false` e só o backend/renderer necessários (winit +
  femtovg, sem skia). Não há dependência a cortar sem perder acessibilidade (`accessibility`).
  Árvore: ~403 pacotes únicos, 71 linhas de duplicata (calloop, hashbrown, bitflags… trazidos
  pelo próprio slint/winit; não removíveis daqui).
- Log da janela já era cauda limitada (20 000 bytes), mas o corte fatiava em offset de byte e
  dava panic em caractere multibyte (`✓`, `✗`, acentos); agora corta em fronteira UTF-8
  (`anexar_cauda`). Foi correção de robustez, não de RSS.
- Saídas de `status --json`/`list --json` são lidas inteiras (`output()`): são JSON de poucos
  KiB do gestor local. Sem canal ilimitado, sem polling.
