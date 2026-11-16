# Fase 0: o que os hooks do Claude Code expõem

> Testado em 16/11/2026 com **Claude Code 2.1.311** no macOS, em modo não interativo (`claude -p`, modelo `haiku`).
> Referência: [Hooks reference](https://code.claude.com/docs/en/hooks), [Agent teams](https://code.claude.com/docs/en/agent-teams), [Subagents](https://code.claude.com/docs/en/sub-agents), [Tools reference](https://code.claude.com/docs/en/tools-reference).

## Resumo

- **Dá pra animar quase tudo com hooks:** chegada e saída de cada subagente, cada ferramenta usada (com argumentos), espera por permissão, erros e fim da sessão.
- **Quem agiu:** eventos do agente principal chegam **sem** `agent_id`; eventos de subagente chegam com `agent_id` e `agent_type`. O **nome** do funcionário (`name`, ex.: `bruno`) não vem nos eventos do próprio subagente: vem no `PreToolUse` do `Agent` que o criou.
- **Mensagens entre agentes aparecem com texto completo:** toda mensagem passa pela ferramenta `SendMessage`, e o `PreToolUse`/`PostToolUse` dela traz remetente (`agent_id` ou principal), destinatário (`to`), resumo (`summary`) e texto (`message`). Testado com subagentes nomeados trocando mensagens de verdade (Ana → Bruno → Ana).
- **O que falta:** as "falas" intermediárias dos subagentes (texto que eles escrevem entre ferramentas) não chegam por hook; só a resposta final (`last_assistant_message` no `SubagentStop`).
- **Times de agentes (agent teams) não foram testados de ponta a ponta:** não é possível criar teammates com `claude -p` (documentado e confirmado). A tentativa de sessão interativa automatizada parou na tela de confiança da pasta. O que está abaixo sobre times vem só da documentação.
- **Recomendação:** hooks como fonte principal. Plano B (transcrições) só como enriquecimento opcional e somente leitura, fora do MVP.

## Como foi testado

1. Projeto descartável fora do repositório, com `git init`, um `README.md`, dois agentes de teste em `.claude/agents/` (`ana-teste`, `bruno-teste`) e um `.claude/settings.json` registrando **todos** os eventos documentados com `matcher: ""`, apontando para um script de shell que faz `cat` da entrada padrão e acrescenta uma linha num JSONL.
   - Exceções: `WorktreeCreate`/`WorktreeRemove` ficaram de fora, porque registrar `WorktreeCreate` **substitui** a criação de worktree do Claude Code. `FileChanged` precisa de nomes de arquivo explícitos no `matcher` (usei `notes.txt`).
2. Seis sessões `claude -p` curtas (custo total abaixo de US$ 0,07):

| Sessão | O que forçou |
|---|---|
| A | `Write`, `Bash`, um subagente `general-purpose` em primeiro plano, um comando negado (`curl`) |
| B | dois subagentes customizados em paralelo, `SendMessage` (sem `name`, falhou por nome e funcionou por id) |
| C | `TaskCreate`/`TaskUpdate`, subagentes com `name` (`ana`, `bruno`) trocando mensagens, `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` ligado |
| D | sessão **interativa** com time (via `expect`): não chegou a rodar, ver "O que ficou sem teste" |
| E | ferramenta bloqueada fora da pasta, subagente em segundo plano (`run_in_background: true`) |
| F | `Read` de arquivo inexistente, `CLAUDE.md` carregado |

Os JSONL crus ficaram fora do repositório. Os exemplos abaixo foram reduzidos e anonimizados.

## Campos comuns (observados)

| Campo | Observação |
|---|---|
| `session_id` | sempre presente; agrupa tudo da sessão, inclusive subagentes |
| `transcript_path` | transcrição do agente **principal** (contém o caminho com o nome do usuário: não exibir nem guardar) |
| `cwd` | pasta atual |
| `prompt_id` | presente depois do primeiro prompt; muda a cada "turno", inclusive quando um subagente em segundo plano termina |
| `permission_mode` | presente na maioria dos eventos (não em `SessionStart`, `SubagentStart`, `FileChanged`, `MessageDisplay`, `SessionEnd`) |
| `effort` | `{ "level": "medium" }` em eventos de ferramenta, `Stop` e `SubagentStop` |
| `agent_id`, `agent_type` | **só** quando o evento acontece dentro de um subagente. Ausentes = agente principal |
| `hook_event_name` | nome do evento |

## Eventos: o que chega e o que animar

Coluna "Visto": sim = chegou nos testes; doc = só na documentação; não = não disparou no cenário testado.

| Evento | Visto | Campos úteis | No escritório |
|---|---|---|---|
| `SessionStart` | sim | `source` (`startup`, `resume`, `clear`, `compact`, `fork`), `model` (não veio) | escritório "abre", chefe (agente principal) senta |
| `UserPromptSubmit` | sim | `prompt` (texto do usuário, ou `<task-notification>` quando um subagente em segundo plano termina) | balão "nova tarefa" no chefe; se for `<task-notification>`, "recebeu relatório" |
| `InstructionsLoaded` | sim | `file_path`, `memory_type`, `load_reason` | opcional: chefe lendo o manual |
| `PreToolUse` | sim | `tool_name`, `tool_input`, `tool_use_id`, `agent_id`/`agent_type` | **principal fonte de animação**: balão "lendo api.ts", "rodando npm test" |
| `PostToolUse` | sim | `tool_response`, `duration_ms` | encerra a animação da ferramenta; no `Agent` traz `agentId`, `agentType`, uso de tokens e duração |
| `PostToolUseFailure` | sim | `error`, `is_interrupt`, `duration_ms` | personagem frustrado, balão vermelho curto |
| `PostToolBatch` | sim | `tool_calls[]` (resumo do lote) | redundante com `PostToolUse`; não registrar |
| `PermissionRequest` | sim | `tool_name`, `tool_input`, `permission_suggestions` | personagem esperando aprovação ("posso rodar curl?") |
| `PermissionDenied` | não | (só no modo `auto`, segundo a doc) | balão "negado" |
| `Notification` | não | `message`, `title`, `notification_type` (`permission_prompt`, `idle_prompt`...) | espera / chefe tomando café; não dispara em `-p` |
| `MessageDisplay` | sim | `delta`, `final`, `message_id` (só do agente principal) | **não registrar**: o Claude Code espera o hook terminar antes de mostrar o texto, o que deixaria o terminal mais lento |
| `SubagentStart` | sim | `agent_id`, `agent_type` | funcionário chega e senta. Dispara de novo quando o subagente é **retomado** por uma mensagem |
| `SubagentStop` | sim | `agent_id`, `agent_type`, `last_assistant_message`, `agent_transcript_path`, `background_tasks` | funcionário entrega o relatório e fica livre; balão com o início da resposta |
| `TaskCreated` | sim | `task_id`, `task_subject`, `task_description`, `teammate_name` (só em time) | post-it novo no quadro |
| `TaskCompleted` | sim | igual ao anterior | post-it vai para "feito" |
| `Stop` | sim | `last_assistant_message`, `background_tasks` | chefe terminou o turno. Se `background_tasks` não estiver vazio, o escritório continua trabalhando |
| `StopFailure` | doc | erro de API no fim do turno | alerta |
| `TeammateIdle` | doc | `teammate_name`, `agent_id` | teammate vai para o café |
| `FileChanged` | sim | `file_path`, `event` (`add`...) | só para arquivos listados no `matcher`; pouco útil |
| `SessionEnd` | sim | `reason` | escritório fecha, todos vão embora |
| `Setup`, `UserPromptExpansion`, `ConfigChange`, `CwdChanged`, `DirectoryAdded`, `PreCompact`, `PostCompact`, `PreModelSwitch`, `PostModelSwitch`, `Elicitation`, `ElicitationResult`, `WorktreeCreate`, `WorktreeRemove` | não/doc | | sem uso no MVP |

### Ferramentas e argumentos (via `PreToolUse.tool_input`)

| `tool_name` | Argumentos | Balão |
|---|---|---|
| `Read` | `file_path` | "lendo `<nome do arquivo>`" |
| `Write` / `Edit` | `file_path`, `content` / `old_string`, `new_string` | "escrevendo `<arquivo>`" (nunca mostrar o conteúdo) |
| `Bash` | `command`, `description` | "`<description>`" (mais legível que o comando) |
| `Glob` / `Grep` | `pattern`, `path` | "procurando `<padrão>`" |
| `Agent` | `description`, `prompt`, `subagent_type`, `name`, `run_in_background` | chefe chama um funcionário: balão com `description` |
| `SendMessage` | `to`, `summary`, `message` | balão que sai do remetente e vai até o destinatário |
| `TaskCreate` / `TaskUpdate` | assunto, status | quadro de tarefas |
| `ToolSearch` | `query` | ignorar (ferramenta interna de carregar outras ferramentas) |

## Identificar quem agiu

- **Principal x subagente:** sem `agent_id` = agente principal (o "chefe"). Com `agent_id` = subagente.
- **Tipo:** `agent_type` é o `name` do frontmatter do agente (`ana-teste`) ou o tipo embutido (`general-purpose`, `Explore`, `Plan`). É o **cargo**, não a pessoa: dois subagentes do mesmo tipo têm o mesmo `agent_type` e `agent_id` diferentes.
- **Nome do funcionário:** só aparece em `PreToolUse` do `Agent` (`tool_input.name`, quando o modelo o preenche; o Haiku não preencheu na sessão B até ser pedido explicitamente) e na resposta do `SendMessage` (`tool_response.pin.name`). O tradutor precisa guardar o mapa `agent_id → nome`.
- **Ligação `Agent` → `agent_id`:**
  - Primeiro plano: `PreToolUse Agent` (tem `tool_use_id` e `name`) → `SubagentStart` (tem `agent_id`) → ... → `SubagentStop` → `PostToolUse Agent` (tem `tool_use_id` **e** `tool_response.agentId`). A ligação definitiva só chega no fim. Com um único subagente, a ordem resolve; com vários em paralelo (sessão B), a ordem dos `SubagentStart` seguiu a dos `PreToolUse`, mas isso não é garantido pela doc. Heurística: casar por `agent_type` em ordem de chegada e corrigir quando o `PostToolUse` chegar.
  - Segundo plano: `PostToolUse Agent` chega **antes** do `SubagentStart`, já com `agentId` e `status: "async_launched"`. Ligação imediata.
- **Retomada:** quando alguém manda `SendMessage` para um subagente que já terminou, chega um novo `SubagentStart` com o **mesmo** `agent_id`. O funcionário "volta para a mesa".

## Mensagens entre agentes

Testado na sessão C (subagentes nomeados, sem time):

1. O chefe cria `bruno` e depois `ana` (`Agent` com `name`).
2. `ana` chama `SendMessage { to: "bruno", message: "oi bruno, qual a primeira linha do README?" }`. O `PostToolUse` responde `success: true`, `resumedAgentId` (id do Bruno) e `pin.name: "bruno"`.
3. Chega `SubagentStart` do Bruno (retomado).
4. `bruno` chama `SendMessage { to: "ana", message: "A primeira linha do README.md é: # fase0" }`, e a Ana é retomada.
5. O chefe recebe o fim do trabalho como `UserPromptSubmit` com `<task-notification>`.

O que dá pra saber só com hooks:

| Pergunta | Resposta |
|---|---|
| Quem mandou? | `agent_id` do evento (ou principal, se ausente) |
| Para quem? | `tool_input.to` (nome ou id); o id real vem em `tool_response.resumedAgentId` |
| O que disse? | `tool_input.message` completo e `tool_input.summary` (5 a 10 palavras, bom para balão) |
| Chegou? | `tool_response.success` (`false` com `display: "Not sent — ..."` quando o nome não existe) |
| O destinatário leu? | indireto: `SubagentStart` do destinatário logo depois. O texto recebido **não** vem em nenhum evento do destinatário |
| Resposta do subagente ao chefe | `PostToolUse Agent.tool_response.content` (primeiro plano), `SubagentStop.last_assistant_message`, ou `<task-notification>` no `UserPromptSubmit` (segundo plano) |

Observação: no modo `auto`, subagentes entregam o relatório pela ferramenta `SubagentHandback` (`tool_input.message`), e `last_assistant_message` deixa de ser o relatório (doc, v2.1.271+). O tradutor deve tratar os dois casos.

## Lacunas

1. **Falas intermediárias dos subagentes:** o texto que o subagente escreve entre ferramentas não chega por hook. `MessageDisplay` só disparou para o agente principal. Para balões "pensando", usar o `description` das ferramentas, que já é bem expressivo.
2. **Nome do subagente nos próprios eventos:** só `agent_type` e `agent_id`. Resolvido guardando o mapa no tradutor (ver acima).
3. **Ligação `Agent` → `agent_id` em paralelo:** só é garantida no `PostToolUse` (primeiro plano). Animação pode chegar com o funcionário "genérico" e ganhar o nome alguns segundos depois, ou usar a heurística por ordem.
4. **Mensagem recebida:** o hook mostra o envio; o recebimento só aparece na transcrição do destinatário (`origin.kind: "peer"`).
5. **Times de agentes:** sem teste real (ver abaixo).
6. **`Notification` e `PermissionDenied`:** não disparam em `-p` / fora do modo `auto`. Para "aguardando o usuário", `PermissionRequest` é mais confiável e imediato.
7. **Volume:** uma sessão trivial gerou de 20 a 30 eventos, e cada hook abre um processo. `termbay hook` precisa ser rápido, e o `init` deve registrar só os eventos necessários.
8. **Privacidade:** os payloads trazem conteúdo de arquivos (`Write.content`, `Read.tool_response`), comandos e caminhos com o nome do usuário. O tradutor deve descartar isso logo na entrada e mandar à interface só o resumo (nome do arquivo, `description`, `summary`).

## Times de agentes (agent teams)

- **Pela documentação:** experimental, ligado por `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`. Teammates in-process disparam hooks com `agent_id`; `SubagentStart` dispara "cada vez que um teammate in-process trata uma nova mensagem"; `TeammateIdle` traz `teammate_name`; `TaskCreated`/`TaskCompleted` trazem `teammate_name`. A troca de mensagens usa a mesma `SendMessage`, então deve ser capturada do mesmo jeito. As caixas de entrada ficam em `~/.claude/teams/{time}/inboxes/{agente}.json`.
- **Pelo teste:** com a variável ligada em `-p` (sessão C), agentes com `name` rodaram como subagentes comuns: `agent_type` do arquivo de agente, sem `teammate_name`, sem `TeammateIdle`. Isso bate com a doc: "em modo não interativo com `-p`, o Claude não cria teammates".

### O que ficou sem teste

- **Sessão interativa com time:** automatizei o `claude` interativo com `expect`, mas a tela "Is this a project you trust?" sempre fechou o programa (a opção padrão é "No, exit" e as teclas de seta não foram aceitas). Não marquei a pasta como confiável editando a configuração global do usuário. Ficou sem teste:
  - payloads reais de `TeammateIdle` e `teammate_name` em `TaskCreated`/`TaskCompleted`;
  - se teammates in-process trazem `agent_type`, e com qual valor;
  - teammates em painel dividido (tmux/iTerm2): são processos separados, e não se sabe se os hooks deles chegam com outro `session_id` ou não chegam;
  - se `SendMessage` entre teammates tem os mesmos campos que entre subagentes.
- **Como fechar a lacuna:** uma sessão interativa manual, de poucos minutos, no projeto de teste (já confiável), com `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` e o mesmo `settings.json`. Dá pra fazer junto com o início da fase 1.
- Também não testados: `Notification`, `PermissionDenied`, `StopFailure`, `PreCompact`/`PostCompact`, `Elicitation*`, `WorktreeCreate`/`WorktreeRemove`, `ConfigChange`, `CwdChanged`, `DirectoryAdded`, `Setup`, `UserPromptExpansion`, troca de modelo. Nenhum deles é essencial para o MVP.

## Plano B: transcrições

- **O que existe** (formato não documentado, observado na 2.1.311): `~/.claude/projects/<projeto>/<session_id>.jsonl` (principal) e `<session_id>/subagents/agent-<agent_id>.jsonl` mais `agent-<agent_id>.meta.json`.
- O `meta.json` traz `agentType`, `name`, `description`, `toolUseId` e `finished`: resolve o nome e a ligação `Agent` → `agent_id` sem heurística.
- A transcrição do subagente traz as falas intermediárias e a mensagem recebida (`origin: { kind: "peer", from: "ana", body: "..." }`).
- **Recomendação:** **não** depender do plano B no MVP. Os hooks cobrem `inicio`, `ferramenta`, `mensagem` (envio), `espera` e `fim`. O plano B entra depois, como enriquecimento opcional, somente leitura, desligável e com leitor tolerante (o formato muda sem aviso). O caminho vem pronto no hook (`agent_transcript_path`), então não é preciso varrer `~/.claude/projects/`.

## Tradução proposta para o formato próprio

| Formato próprio | Origem |
|---|---|
| `inicio` | `SubagentStart` (primeira vez por `agent_id`); `SessionStart` para o chefe |
| `ferramenta` | `PreToolUse` (exceto `Agent`, `SendMessage`, `ToolSearch`); termina no `PostToolUse` |
| `mensagem` | `PreToolUse SendMessage` (`para` = `to`, `texto` = `summary` ou início de `message`); do chefe para um funcionário também em `PreToolUse Agent` (`description`) |
| `espera` | `PermissionRequest`, `Notification` (`permission_prompt`, `idle_prompt`), `TeammateIdle` |
| `fim` | `SubagentStop` (`resultado` = início de `last_assistant_message`), `Stop` e `SessionEnd` para o chefe |
| (novo) `erro` | `PostToolUseFailure`, `StopFailure` |
| (novo) `tarefa` | `TaskCreated`, `TaskCompleted` |

Eventos para o `init` registrar: `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PermissionRequest`, `Notification`, `SubagentStart`, `SubagentStop`, `Stop`, `StopFailure`, `TaskCreated`, `TaskCompleted`, `TeammateIdle`. **Não** registrar `MessageDisplay` (bloqueia a exibição), `PostToolBatch` (redundante) nem `WorktreeCreate` (substitui o comportamento padrão).

## Exemplos de payload (reduzidos e anonimizados)

`/home/user/projeto` substitui o caminho real; ids foram trocados.

Ferramenta no agente principal:

```json
{"session_id":"sess-0001","cwd":"/home/user/projeto","permission_mode":"acceptEdits","effort":{"level":"medium"},
 "hook_event_name":"PreToolUse","tool_name":"Bash",
 "tool_input":{"command":"ls","description":"List files in current directory"},"tool_use_id":"toolu_A"}
```

Chefe chama um funcionário com nome:

```json
{"session_id":"sess-0001","hook_event_name":"PreToolUse","tool_name":"Agent",
 "tool_input":{"description":"Ler primeira linha do README","prompt":"...","subagent_type":"bruno-teste","run_in_background":false,"name":"bruno"},
 "tool_use_id":"toolu_B"}
```

Funcionário chega (sem nome, só tipo e id):

```json
{"session_id":"sess-0001","hook_event_name":"SubagentStart","agent_id":"agent-bruno-01","agent_type":"bruno-teste"}
```

Funcionária manda mensagem para outro:

```json
{"session_id":"sess-0001","agent_id":"agent-ana-01","agent_type":"ana-teste","hook_event_name":"PreToolUse","tool_name":"SendMessage",
 "tool_input":{"to":"bruno","summary":"Pergunta ao Bruno sobre a primeira linha do README","message":"oi bruno, qual a primeira linha do README?",
               "type":"message","recipient":"bruno","recipient_kind":"name","content":"oi bruno, qual a primeira linha do README?"},
 "tool_use_id":"toolu_C"}
```

Resposta do envio:

```json
{"hook_event_name":"PostToolUse","tool_name":"SendMessage","agent_id":"agent-ana-01","agent_type":"ana-teste",
 "tool_response":{"success":true,"message":"Resuming agent bruno","resumedAgentId":"agent-bruno-01","pin":{"id":"agent-bruno-01","name":"bruno"}}}
```

Funcionário termina:

```json
{"session_id":"sess-0001","hook_event_name":"SubagentStop","agent_id":"agent-bruno-01","agent_type":"bruno-teste","stop_hook_active":false,
 "agent_transcript_path":"/home/user/.claude/projects/<projeto>/sess-0001/subagents/agent-<agent_id>.jsonl",
 "last_assistant_message":"# fase0","background_tasks":[],"session_crons":[]}
```

Fim da chamada `Agent` (ligação `tool_use_id` → `agentId`):

```json
{"hook_event_name":"PostToolUse","tool_name":"Agent","tool_use_id":"toolu_B",
 "tool_response":{"status":"completed","agentId":"agent-bruno-01","agentType":"bruno-teste","toolsUsed":["Read"],
                  "content":[{"type":"text","text":"# fase0"}],"totalDurationMs":2925,"totalToolUseCount":1}}
```

Subagente em segundo plano (ligação imediata):

```json
{"hook_event_name":"PostToolUse","tool_name":"Agent",
 "tool_response":{"isAsync":true,"status":"async_launched","agentId":"agent-bg-01","description":"Count entries in current directory"}}
```

Espera por permissão:

```json
{"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"curl -s https://example.com","description":"Fetch example.com homepage with curl"},
 "permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"curl -s https://example.com"}],"behavior":"allow","destination":"localSettings"}]}
```

Erro de ferramenta:

```json
{"hook_event_name":"PostToolUseFailure","tool_name":"Read","tool_input":{"file_path":"/home/user/projeto/missing.txt"},
 "error":"File does not exist. ...","is_interrupt":false,"duration_ms":2}
```

Tarefa:

```json
{"hook_event_name":"TaskCreated","task_id":"1","task_subject":"cumprimentar","task_description":"..."}
```
