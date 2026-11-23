# Fase 2: plano do escritório

> Status: decisões E1–E11 aceitas em 22/11/2026 e registradas em [ARQUITETURA.md](ARQUITETURA.md). F1, A1, D1, A2, A3 e B1 feitas (servidor, redutor e variáveis em `core::events`; binário em `hook/`; `office_subscribe` e lista de depuração no build do e2e). D3 feita (`core::state`: eventos reduzidos → formato próprio e foto do escritório). D2 feita (`termbay hooks install`, `uninstall` e `status`; `termbay-hook` dentro dos pacotes). `core::state` ligado ao `office_subscribe`: a página recebe a foto do escritório e depois as mudanças (E5). Marco E1 completo. B2 feita: painel do escritório em `ui/src/office/panel.ts` (abre sozinho no primeiro agente com `[office] auto_open`, a menos que a pessoa já o tenha fechado antes, mesmo sem agentes; fecha pelo Cmd/Ctrl+Shift+O ou pelo botão; divisor arrastável com largura lembrada no armazenamento local); o conteúdo entra pela interface `OfficeContent` (`ui/src/office/content.ts`), carregado com `import()` só na primeira abertura: a lista de depuração no build do e2e e um painel vazio no release, até a cena (C2). Próximo passo: C1 e C2 (primeira cena).
> Objetivo: quando o `claude` roda dentro do termbay, um painel mostra o escritório em 2D, com o agente principal ("chefe") e cada subagente como funcionário. Eles chegam, trabalham com balões do que estão fazendo, esperam aprovação, trocam mensagens e vão embora. O terminal continua igual para quem não usa agentes.
> Base: o que os hooks expõem está em [FASE0-HOOKS.md](FASE0-HOOKS.md). O terminal (fase 1, marcos 1 e 2) já reserva o painel `aside#office-panel` e o atalho Cmd/Ctrl+Shift+O.

## 1. Escopo

**Entra:**
- `termbay-hook <evento>`: binário pequeno, sem Tauri (E11), chamado pelos hooks do Claude Code; reduz o payload e o envia ao app.
- Servidor de eventos local (`core::events`), só em `127.0.0.1`, com token por sessão do app.
- Tradutor com estado (`core::state`): eventos crus reduzidos → formato próprio (seção 2, E4), com o mapa `agent_id → nome`.
- `termbay hooks install` / `uninstall`: registra os hooks no `settings.json` do Claude Code, com prévia e confirmação.
- Painel do escritório (`ui/src/office/`) em PixiJS, carregado só quando abre, que para de desenhar quando escondido.
- Mensagens entre agentes (subagentes nomeados e times in-process) como balões que vão de um personagem ao outro.
- Arte própria ou CC0, com origem em `assets/CREDITS.md`.

**Fica de fora (e onde vai entrar):**

| Fora da fase 2 | Onde | O que a fase 2 deixa pronto |
|---|---|---|
| Recrutador, `termbay init`, catálogo de cargos | fase 3 | `time.json` é lido se existir (nome e visual), mas nada o gera ainda |
| `termbay doctor` completo | fase 3 | `termbay hooks status` cobre o essencial: hooks instalados? caminho do binário certo? |
| Modo navegador (`termbay serve`) | depois (D7) | o servidor de eventos e o formato próprio já são os que o `serve` vai usar |
| Transcrições (plano B da fase 0) | depois | o tradutor aceita eventos de outra origem sem mudar a interface |
| Teammates em painel dividido (tmux/iTerm2) | depois | eventos com outro `session_id` aparecem como outra sessão, sem quebrar |
| 3D, blocos (OSC 133), paleta de comandos | fase 5 | nada específico |
| Marco 3 da fase 1 (config, temas, busca, revisão de segurança) | em paralelo | a fase 2 só acrescenta a seção `[office]` à config |

## 2. Decisões a fechar antes de começar

| # | Decisão | Recomendação | Motivo |
|---|---|---|---|
| E1 | Como o hook acha o app | O app põe no ambiente de cada shell que abre: `TERMBAY_EVENTS` (endereço `127.0.0.1:<porta>`), `TERMBAY_TOKEN` e `TERMBAY_PANE` (id do painel). O `claude` e os hooks herdam. **Sem essas variáveis, `termbay-hook` sai na hora com código 0**, sem fazer nada | Nenhum arquivo de descoberta para vazar ou ficar velho. O `claude` fora do termbay não é afetado. O `TERMBAY_PANE` liga cada sessão ao seu painel (clicar no personagem foca o terminal dele) |
| E2 | Onde reduzir o payload | **No processo do hook, antes de enviar.** O `termbay-hook` mantém só os campos da lista da seção 3.2, troca caminhos pelo nome do arquivo, corta textos e descarta conteúdo (`Write.content`, `tool_response` de leitura, `prompt`, `transcript_path`). O tradutor com estado roda no app | O conteúdo de arquivos, comandos e caminhos com o nome do usuário nunca cruza o socket nem entra na memória do app (lacuna 8 da fase 0) |
| E3 | Contrato do hook com o Claude Code | **Sempre código 0 e nada no stdout**, mesmo com erro. Conexão e envio com prazo total de 200 ms; se passar, o evento é descartado. Entrada lida até 8 MiB, o resto ignorado | No Claude Code, código 2 bloqueia a ferramenta e o stdout de alguns hooks vira decisão. O escritório nunca pode atrapalhar nem atrasar o `claude` |
| E4 | Formato próprio | Chaves em **inglês** (convenção do código): `{ "v": 1, "session", "pane", "agent", "kind", ... }` com `kind` em `start`, `tool`, `message`, `wait`, `end`, `error`, `task`. Versionado em `v`; campos desconhecidos são ignorados | A arquitetura mostra o formato com chaves em português (`sessao`, `agente`, `tipo`), o que contraria a regra de código em inglês. O formato é a porta de entrada para tradutores de outras ferramentas |
| E5 | Onde fica o estado | Em Rust (`core::state`): quem está no escritório, o que cada um faz, os últimos balões. A UI recebe uma **foto inicial e depois as mudanças** por um `Channel` do Tauri (`office_subscribe`) | Testável sem DOM, igual para o app e para o futuro `serve`, e as heurísticas da fase 0 (ligação `Agent → agent_id`, retomadas) ficam num lugar só |
| E6 | Servidor | `std::net::TcpListener` em `127.0.0.1:0`, uma thread, HTTP/1.1 mínimo escrito à mão (só `POST /v1/events`), token de 32 bytes do sistema (`getrandom`, já no lockfile) comparado em tempo constante, corpo até 64 KiB, conexões curtas, `Host` conferido. Sem `tokio`, sem framework | É um endpoint só, local, com payloads pequenos. Menos dependências e menos superfície. O `serve` pode trocar por algo maior quando precisar de WebSocket |
| E7 | Onde instalar os hooks | `termbay hooks install` grava no **`~/.claude/settings.json` do usuário** por padrão (`--project` para `.claude/settings.json`), mostrando a mudança e pedindo confirmação (`--yes` para pular). Mescla sem apagar hooks do usuário, é idempotente e guarda o caminho absoluto do `termbay-hook` | Com E1, o hook não faz nada fora do termbay, então instalar uma vez serve para todos os projetos. Gravar no projeto sujaria o git de cada repositório |
| E8 | Biblioteca do escritório | **PixiJS 8** (MIT), carregado com `import()` só quando o painel abre pela primeira vez, ticker parado com o painel escondido ou a janela minimizada | Mantém a decisão "PixiJS 2D" da arquitetura. O carregamento tardio faz quem só usa o terminal não pagar nada na abertura. Alternativa mais leve: Canvas 2D puro, se o PixiJS pesar demais na medição (F1) |
| E9 | Quando o painel aparece | Abre sozinho no **primeiro evento de agente** da janela (`[office] auto_open = true`), fecha só pelo atalho ou pelo botão. Um escritório por janela, com todas as sessões; cada sessão tem seu chefe, identificado pelo nome da aba | Segue o princípio "aparece sozinho quando há agentes" sem esconder nada do terminal. Várias sessões do `claude` em abas diferentes aparecem no mesmo escritório |
| E10 | O que um balão pode mostrar | Nome do arquivo (nunca o caminho), `description` do `Bash` (sem ela, só o nome do programa, nunca o comando inteiro), `summary` do `SendMessage`, padrão do `Grep`/`Glob`, assunto da tarefa. Tudo cortado em 80 caracteres e limpo de sequências de controle. **Nunca** o texto do prompt do usuário nem o conteúdo de arquivos | Comandos podem ter tokens (`curl -H "Authorization: ..."`). O escritório pode aparecer num print ou numa transmissão |
| E11 | Binário do hook | **`termbay-hook`, um binário separado e pequeno**: crate próprio no workspace, sem Tauri, que depende só do redutor e do cliente do `core`. Vai no mesmo pacote, ao lado do `termbay`. `termbay hooks install` continua no binário do app | Medido na F1: o binário do app abre em 31,9 ms (p95) no Linux, acima do limite de 30 ms, porque carrega 131 bibliotecas, WebKitGTK entre elas, antes do `main`. O `PreToolUse` segura cada ferramenta do `claude` até o hook terminar |

## 3. Contratos

### 3.1 Fluxo

```
claude (dentro de um painel do termbay)
  └─ hook do Claude Code ──► termbay-hook PreToolUse      (processo curto, sem Tauri)
                               │ lê stdin, reduz (E2), corta (E10)
                               ▼  POST /v1/events, token, 200 ms (E3)
app: core::events ──► core::state (tradutor, E5) ──► Channel ──► ui/office (PixiJS)
```

### 3.2 O que o hook mantém (lista fechada)

Comuns: `hook_event_name`, `session_id`, `agent_id`, `agent_type`, `tool_use_id`, mais `TERMBAY_PANE` do ambiente.

| Evento | Campos mantidos |
|---|---|
| `PreToolUse` / `PermissionRequest` | `tool_name`; de `tool_input`: nome do arquivo de `file_path`, `description`, primeira palavra de `command`, `pattern`, e no `Agent`: `name`, `subagent_type`, `description`, `run_in_background`; no `SendMessage`: `to`, `summary`, `type` |
| `PostToolUse` | `tool_name`; no `Agent`: `agentId`, `status`, `name`, `color`; no `SendMessage`: `success`, `resumedAgentId`, `routing.sender`, `routing.target`, `routing.summary`, cores |
| `PostToolUseFailure` | `tool_name`, `is_interrupt` (o texto do erro fica de fora) |
| `SubagentStart` / `SubagentStop` / `TeammateIdle` | ids, `teammate_name`, e só no `SubagentStop` os primeiros 80 caracteres de `last_assistant_message` |
| `TaskCreated` / `TaskCompleted` | `task_id`, `task_subject` |
| `SessionStart` / `SessionEnd` / `Stop` / `StopFailure` / `UserPromptSubmit` / `Notification` | só o evento, `source`/`reason`/`notification_type`, e se o prompt é um `<task-notification>` (sim/não) |

Qualquer evento fora da lista é descartado no hook.

### 3.3 Interface com a UI

- `office_subscribe(channel)`: devolve a foto atual (`OfficeSnapshot`) e depois manda `OfficeEvent` no formato E4.
- Clicar num personagem foca o painel de `TERMBAY_PANE` pelo controlador de layout da UI (o mesmo caminho do foco por atalho).

## 4. Tarefas

Siglas: **A** = ana-core, **B** = bruno-terminal, **C** = carla-escritorio, **D** = davi-integracao, **F** = felipe-qa, **G** = gabi-opensource.

### Marco E1: "o evento chega" (sem arte)

| # | Resp. | Tarefa | Depende de | Pronto quando | Teste que comprova |
|---|---|---|---|---|---|
| F1 | F | **Medição antes de tudo:** tempo de abertura do binário do app nos três sistemas (no Linux ele carrega o WebKitGTK ao abrir) e peso do PixiJS no bundle. **Feita em 22/11/2026**, resultado na seção 6 | — | números na CI e decisão: binário único ou um `termbay-hook` separado (decidido: E11) | `scripts/measure-startup.mjs` no job de bundle: 50 execuções, primeira abertura, p50, p95 e máximo |
| A1 | A | `core::events`: servidor de E6 numa thread, iniciado com o app | E6 | aceita evento com token certo e recusa o resto | unitários: token errado, sem token, corpo grande demais, `Host` diferente, método ou caminho errado, conexão lenta (prazo) e porta só em `127.0.0.1` |
| A2 | A | Variáveis de E1 no ambiente de cada pty | E1 | o shell enxerga as três variáveis | teste do `core::pty` com `/bin/sh` e `cmd` imprimindo as variáveis |
| D1 | D | Redutor do hook (seção 3.2 e E10), puro, em `core` | E2, E10 | só os campos da lista saem | tabela com os payloads da fase 0; teste de "canário": um segredo posto em `content`, `command`, `prompt`, `transcript_path` e no erro nunca aparece na saída |
| A3 | A | Binário `termbay-hook <evento>` (E3, E11): crate `hook/` no workspace, sem Tauri. O app sobe o servidor ao abrir e passa as variáveis a cada shell | A1, D1 | o evento chega ao servidor; sem variáveis, sai na hora | integração: com servidor de teste recebe o evento; sem servidor, sem variáveis, com stdin gigante ou JSON inválido sai com 0, sem stdout e dentro do prazo. Medição da F1 com limite de p95: 10 ms no Linux e no Windows, 20 ms no macOS, cujos runners variam muito |
| D2 | D | `termbay hooks install` / `uninstall` / `status` (E7), e o `termbay-hook` empacotado ao lado do `termbay` nos três sistemas (`.app`, `.deb`/`.rpm`, NSIS), achado pelo `install` ao lado do próprio executável | A3 | instala, desinstala e mostra o estado sem perder nada do usuário; o pacote continua abaixo de 20 MB | fixtures de `settings.json`: vazio, com hooks do usuário, com hooks antigos do termbay, inválido (não grava e explica); rodar duas vezes não duplica |
| B1 | B | Ponte: `office_subscribe` e uma lista de eventos para depuração (atrás de um sinalizador de build) | A1 | os eventos aparecem na tela | e2e: um painel roda `termbay hook PreToolUse < fixture.json` e o evento aparece |

### Marco E2: "o escritório ganha vida"

| # | Resp. | Tarefa | Depende de | Pronto quando | Teste que comprova |
|---|---|---|---|---|---|
| D3 | D | `core::state`: tradutor com estado (E4, E5) com as regras da fase 0: chefe sem `agent_id`, mapa `agent_id → nome`, ligação por `tool_use_id` com a heurística por ordem, segundo plano, retomada, `agent_type` vazio ignorado, `SubagentHandback` | D1 | sequências reais viram a foto certa | testes com sequências gravadas e anonimizadas das sessões A–G da fase 0 (em `recordings/`, sem os brutos), incluindo dois subagentes em paralelo e retomada por mensagem |
| C1 | C | Mapeamento evento → animação, puro, em TS | E4 | cada `kind` tem estado e balão definidos | Vitest da tabela inteira, inclusive corte em 80 caracteres e textos com sequências de controle |
| C2 | C | Cena PixiJS com personagens provisórios (formas e nomes): mesas distribuídas pelo número de agentes, chegar, sentar, trabalhar, esperar, sair | C1, E8 | uma sessão com 2 subagentes anima do início ao fim | e2e: o canvas existe e a foto da cena bate com a do estado; teste de que o ticker para com o painel escondido |
| B2 | B | Painel: abrir sozinho (E9), atalho Cmd/Ctrl+Shift+O (já reservado), divisor arrastável, largura lembrada, `[office]` na config | B1 | o painel aparece com agentes e some no atalho | Vitest do modelo do painel; e2e: evento de agente abre o painel e o atalho fecha; fechado, nenhum `requestAnimationFrame` |
| F2 | F | Revisão de segurança do marco: servidor, token no ambiente (o `ssh` não repassa variáveis por padrão; conferir `sudo`, `tmux`, `docker run -e`), redutor, balões | A1–A3, D1 | parecer em [SEGURANCA.md](SEGURANCA.md) sem pendência alta | itens do parecer viram testes |

### Marco E3: "conversa e acabamento"

| # | Resp. | Tarefa | Depende de | Pronto quando | Teste que comprova |
|---|---|---|---|---|---|
| D4 | D | Mensagens: `SendMessage` vira `message` com remetente e destinatário resolvidos; times in-process com nome e cor do `teammate_spawned`; desligamento do time | D3 | conversa Ana → Bruno → Ana aparece certa | sequências da sessão C (subagentes) e da sessão G (time) |
| C3 | C | Balão que viaja de um personagem ao outro; quadro de tarefas simples (`task`) | D4, C2 | mensagens e tarefas visíveis | Vitest do caminho e da fila de balões; e2e com a sequência da sessão G |
| C4 | C | Arte: personagens, mesas e chão próprios ou CC0, com animações de andar, sentar e digitar; registro em `assets/CREDITS.md` | C2 | arte final no lugar dos provisórios | checagem da Gabi; teste de que todo arquivo em `assets/` aparece no `CREDITS.md` |
| B3 | B | Clicar num personagem foca o painel dele (`TERMBAY_PANE`); passar o mouse mostra a última ação | C2 | o clique leva ao terminal certo, inclusive em outra aba | e2e com duas abas |
| F3 | F | Linha de base: binário ainda abaixo de 20 MB, memória e CPU com o escritório aberto, fechado e minimizado, latência do hook | C4 | números no README e limites na CI | jobs de tamanho e latência; CPU parada com o painel fechado |
| G1 | G | Documentação: README (como ligar o escritório, o que é enviado e o que nunca é), ARQUITETURA atualizada, CHANGELOG, licenças do PixiJS e da arte | todas | revisão da Gabi aprovada | checagem de licenças verde |

## 5. Ordem e paralelismo

```
Aceite de E1–E10
   │
   ├─ F: F1 (medição) ───────────────────────► F2 ─────────────► F3
   ├─ A: A1 ──► A2 ──► A3 ──┐
   ├─ D: D1 ────────────────┴─► D2 ──► D3 ──────► D4
   ├─ B:              B1 ──────► B2 ──────────────► B3
   └─ C: C1 ──────────────────► C2 ───────────────► C3, C4      G1 no fim
                  ▲ marco E1           ▲ marco E2          ▲ marco E3
```

- **F1 veio primeiro** porque podia mudar a forma do hook, e mudou (E11).
- **A e D começam juntos:** o redutor (D1) é puro e não precisa do servidor. A Carla começa pelo mapeamento (C1) e pela cena com eventos falsos, sem esperar o core.
- **Marco E1:** o evento sai do `claude` e aparece numa lista na tela, nos três sistemas, com o hook instalado por comando.
- **Marco E2:** uma sessão real com subagentes anima do começo ao fim com personagens provisórios, e o painel abre e fecha sozinho.
- **Marco E3:** conversas, times, arte final e números de desempenho. Fim da fase 2.

## 6. Riscos e mitigação

| Risco | Mitigação |
|---|---|
| **Abertura lenta do hook:** cada evento abre um processo (20 a 30 por sessão trivial), e o binário do app carrega o WebKitGTK no Linux | **Resolvido pela F1 e pela E11** (tabela abaixo): o hook é um binário próprio, sem Tauri, com limite de p95 na CI |
| **Hooks no Windows:** o Claude Code roda o comando do hook por um shell que pode variar (Git Bash ou outro) e o caminho tem espaços | caminho absoluto entre aspas no `settings.json`; e2e no Windows com o comando gravado pelo `hooks install` |
| **Mudanças no Claude Code:** campos e eventos mudam sem aviso | redutor e tradutor tolerantes (campo desconhecido é ignorado, campo faltando não derruba), testes com as sequências gravadas, versão do Claude Code anotada em cada gravação |
| **Mexer no `settings.json` do usuário** | prévia, confirmação, cópia `settings.json.bak` antes de gravar, mesclagem que preserva o resto, `uninstall` que remove só o que é nosso |
| **Token no ambiente:** qualquer processo do usuário dentro do painel vê o token | o modelo de ameaça é o mesmo usuário na mesma máquina; o token só permite mandar eventos de escritório, nunca ler nada nem controlar o pty. Registrado em SEGURANCA.md (F2) |
| **Escritório gastando CPU/bateria** | ticker parado quando escondido, minimizado ou sem mudanças por alguns segundos; F3 mede |
| **Ligação `Agent → agent_id` em paralelo** (lacuna 3 da fase 0) | personagem chega "genérico" e ganha o nome quando o `PostToolUse` confirma; teste com a sessão B |
| **Termos da Anthropic** (ponto em aberto da arquitetura) | a fase 2 só lê eventos do `claude` oficial do usuário e não muda o login; confirmar os termos antes do lançamento continua pendente |

### Resultado da F1 (22/11/2026)

Binário do app (`termbay --version`, release), 50 execuções depois de 3 de aquecimento, nos runners da CI:

| Sistema | Primeira abertura | p50 | p95 | Limite do plano (p95) |
|---|---|---|---|---|
| macOS | 23,8 ms | 7,3 ms | 11,5 ms | 30 ms |
| Windows | 38,5 ms | 10,5 ms | 17,3 ms | 60 ms |
| Linux | 58,8 ms | 31,0 ms | **31,9 ms** | 30 ms (acima) |

No Linux, o binário liga 131 bibliotecas, WebKitGTK entre elas. Daí a E11.

PixiJS 8.23.0 numa cena mínima carregada com `import()`: o pacote inicial fica com 2 KB; o escritório soma 575 KB de JS (170 KB comprimidos), em arquivos carregados só quando o painel abre. As 12 dependências são MIT, ISC ou BSD-3-Clause.

## 7. Decisões aceitas

As onze da seção 2, aceitas pelo Guilherme em 22/11/2026 (E11 depois da F1). As que mais mudam o produto:

- **E1 + E7:** hooks instalados uma vez no `~/.claude/settings.json` do usuário, que não fazem nada fora do termbay.
- **E4:** chaves do formato próprio em inglês, corrigindo o exemplo em português da arquitetura.
- **E8:** PixiJS carregado só quando o painel abre, com Canvas 2D como plano B se pesar.
- **E10:** o que pode e o que nunca pode aparecer num balão.
