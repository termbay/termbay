# Projetos parecidos

> Pesquisa feita em 16/11/2026 no GitHub (`gh search repos`, API do GitHub) e na web. Estrelas são aproximadas. "Último push" é a data do último push no repositório, consultada nesse dia. Onde não deu pra confirmar, está escrito "não confirmado".

## Resumo

Já existem várias ferramentas que mostram os agentes do Claude Code como personagens num escritório em pixel art. A maior delas, o **Pixel Agents** (cerca de 10 mil estrelas, MIT), já faz boa parte do que o termbay promete no escritório: um personagem por agente, subagentes e colegas de time como personagens, balões e um terminal embutido no modo navegador.

O que ainda **não encontramos** em nenhum projeto é a combinação de:

1. um **terminal de uso geral** de verdade (abas, divisão de tela, blocos), bom mesmo sem agentes;
2. um **binário único em Rust**, sem Node nem Python em tempo de execução, nos três sistemas;
3. um **recrutador** que lê o escopo do projeto e gera o time (`.claude/agents/`) com nomes, cargos e visual;
4. **conversa entre agentes** (times do Claude Code) desenhada como balões de um personagem para outro.

## Escritórios e visualizadores de agentes

| Projeto | Licença | Atividade | O que faz | Sobreposição | O que nos diferencia |
|---|---|---|---|---|---|
| [Pixel Agents](https://github.com/pixel-agents-hq/pixel-agents) | MIT (código). Arte baseada no pacote Metro City de JIK-A-4: licença da arte não confirmada | ~10,3 mil estrelas, último push 10/11/2026 | Extensão do VS Code e CLI `npx pixel-agents`. Escritório pixel art com um personagem por agente, subagentes e "teammates", balões de notificação, editor de layout e de personagens, terminal embutido no navegador, barras de uso e contexto. Roteiro cita "formar time" e passar trabalho entre agentes | **Alta.** Escritório, personagens, balões, servidor local com token, suporte a times | Precisa de VS Code ou Node 20. Não é um terminal de uso geral nem app desktop leve. Não monta o time a partir do escopo |
| [Claude Office Visualizer](https://github.com/paulrobello/claude-office) (paulrobello) | MIT | ~620 estrelas, último push 11/11/2026 (v0.25.1) | Escritório pixel art em PixiJS: o agente principal é o "chefe" e os subagentes são "funcionários". Recebe eventos por hooks. Next.js + FastAPI, também via Docker. Suporta OpenCode | **Alta** no escritório e no uso de hooks | Precisa de Node + Python rodando. Não é terminal. Sem recrutador |
| [pixtuoid](https://github.com/pixtuoid/pixtuoid) | MIT | ~570 estrelas, último push 16/11/2026 | App de terminal (TUI) em Rust que mostra os agentes como colegas num escritório pixel art. Suporta cerca de 15 CLIs de agentes, tem `pixtuoid doctor` e instala a integração sozinho | **Média.** Rust, `doctor`, escritório, multiplataforma | É um painel que roda dentro de outro terminal, não um terminal com abas. Sem recrutador nem conversa entre agentes |
| [Claude Code Park](https://github.com/t-soda/claude-code-park) | MIT | ~25 estrelas, último push 26/10/2026 | App desktop **Tauri + Rust + PixiJS**: escritório top-down com agentes como funcionários, gerenciador de hooks, métricas por agente. Lê os `.jsonl` de `~/.claude/projects/` | **Alta na pilha técnica** (é quase a nossa arquitetura) | Só macOS. Não é terminal. Sem recrutador |
| [Claude Code Office](https://github.com/azorkai/claude-code-office) (azorkai) | MIT | ~0 estrelas, último push 27/09/2026 | Painel em `127.0.0.1` com escritório isométrico 3D, linha do tempo e custo estimado. Lê `~/.claude` somente leitura, sem dependências npm | Média: escritório, subagentes, local e privado | Precisa de Node. Não é terminal. Nome parecido com o nosso |
| [Pixel Agent Desk](https://github.com/Mgpixelart/pixel-agent-desk) | Código MIT, **arte proprietária** (`LICENSE-ASSETS` proíbe modificar e redistribuir) | ~140 estrelas, último push 10/03/2026 | App Electron que escuta hooks do Claude Code e mostra cada sessão como avatar, com suporte a subagentes e times | Alta: hooks, escritório, times | Electron (pesado). Arte não pode ser reaproveitada |
| [Agent Office](https://github.com/NosytLabs/agent-office) (NosytLabs) | MIT (código do projeto) | ~23 estrelas, último push 16/11/2026 | Painel local em pixel art para Claude Code, Codex, Gemini CLI, OpenCode e Hermes, com instalador de integrações | Média: escritório, vários tradutores | Python. Não é terminal. Nome parecido com o nosso |
| [ccworkspace](https://github.com/pknull/ccworkspace) | MIT | ~22 estrelas, último push 16/09/2026 | Companheiro desktop que mostra sessões do Claude Code e do Codex como escritório virtual (alpha, "puramente cosmético") | Baixa a média | Não é terminal |
| [NotchCrew](https://github.com/DevZair/notchcrew) | MIT | ~3 estrelas, último push 27/10/2026 | App do notch do macOS com escritório pixel art, atividade de agentes e respostas a permissões via hooks | Baixa | Só macOS, interface em russo, não é terminal |
| [AI Town](https://github.com/a16z-infra/ai-town) | MIT | ~10,8 mil estrelas, último push 05/11/2026 | Cidade virtual onde personagens de IA vivem e conversam. Simulação, não visualiza agentes de código | Baixa: só a estética | Não tem relação com Claude Code |
| [My Virtual Office](https://github.com/eliautobot/my-virtual-office) | **AGPL-3.0** | ~390 estrelas, último push 14/11/2026 | Espaço de trabalho 2D auto-hospedado para agentes de IA | Média | Licença incompatível: não reaproveitar código |

Outros menores, todos com poucas estrelas: [PixelOffice](https://github.com/KalebKE/PixelOffice) (Apache-2.0), [pixel-office](https://github.com/Neolio42/pixel-office) (MIT), [agents-office](https://github.com/cernadasjuan/agents-office) (3D, licença não confirmada), [clawffice](https://github.com/ractive/clawffice) (MIT, mas a arte vem de pacotes pagos), [claude_code_office](https://github.com/Open-Shadow/claude_code_office) (MIT).

## Terminais com IA e orquestradores de sessões

| Projeto | Licença | Atividade | O que faz | Sobreposição | O que nos diferencia |
|---|---|---|---|---|---|
| [Warp](https://github.com/warpdotdev/warp) | **AGPL-3.0** (segundo a API do GitHub) | ~66 mil estrelas, último push 16/11/2026 | O terminal que inspira o projeto: blocos, IA própria, "ambiente de desenvolvimento agêntico" | Terminal moderno com blocos | Nosso foco é o Claude Code do usuário e o escritório. Licença incompatível: só inspiração, nada de código |
| [Wave Terminal](https://github.com/wavetermdev/waveterm) | Apache-2.0 | ~22,7 mil estrelas, último push 14/11/2026 | Terminal multiplataforma com IA integrada, blocos e painéis | Terminal com IA | Electron. Sem escritório nem visualização de agentes |
| [Tabby](https://github.com/Eugeny/tabby) | MIT | ~75 mil estrelas, último push 16/11/2026 | Terminal moderno, multiplataforma, com plugins | Terminal com abas e temas | Electron. Sem agentes. Possível referência de recursos |
| [cmux](https://github.com/manaflow-ai/cmux) | Não confirmado (a API mostra `NOASSERTION`, arquivo próprio da Manaflow) | ~28,8 mil estrelas, último push 16/11/2026 | Terminal macOS baseado no Ghostty com abas verticais e notificações para agentes de código | Terminal pensado para agentes | Só macOS. Sem escritório |
| [Panes](https://github.com/wygoralves/panes) | MIT | ~130 estrelas, último push 07/09/2026 | Cockpit local em **Tauri v2** com agentes, terminal e git, nos três sistemas. README também em pt-BR | Média: Tauri, terminal + agentes | Sem escritório nem recrutador |
| [Paradev](https://github.com/ParadevAI/Paradev) (antigo Superset) | MIT | ~16 mil estrelas, último push 16/11/2026 | App desktop para rodar vários agentes em paralelo, um worktree por tarefa, revisão e status dos agentes | Média: várias sessões e status | Foco em worktrees e PRs. macOS primeiro |
| [Vibe Kanban](https://github.com/BloopAI/vibe-kanban) | Apache-2.0 | ~28,7 mil estrelas, último push 15/11/2026 | Quadro kanban que orquestra agentes de código | Baixa | Outro foco: gestão de tarefas |
| [Claude Squad](https://github.com/smtg-ai/claude-squad) | **AGPL-3.0** | ~8,8 mil estrelas, último push 13/11/2026 | TUI para gerenciar várias sessões de agentes no terminal | Média | Licença incompatível |
| [opcode](https://github.com/winfunc/opcode) (antigo Claudia) | **AGPL-3.0** | ~22 mil estrelas, último push 06/11/2026 | GUI para Claude Code: sessões, agentes personalizados | Média | Licença incompatível |
| [CloudCLI / Claude Code UI](https://github.com/siteboon/claudecodeui) | **AGPL-3.0** | ~14,6 mil estrelas, último push 16/11/2026 | Interface web/mobile para Claude Code e outros CLIs | Baixa | Licença incompatível |
| [Crystal / Nimbalyst](https://github.com/stravu/crystal) | MIT | ~3,1 mil estrelas, último push 26/02/2026 (virou Nimbalyst) | Várias sessões em worktrees paralelos | Baixa | Parece parado neste repositório |

## Monitoramento por hooks

| Projeto | Licença | Atividade | O que faz | Observação |
|---|---|---|---|---|
| [claude-code-hooks-multi-agent-observability](https://github.com/disler/claude-code-hooks-multi-agent-observability) | **Sem licença** (nenhum arquivo detectado) | ~1,6 mil estrelas, último push 08/02/2026 | Painel em tempo real com eventos de hooks de vários agentes | Boa referência de quais eventos os hooks expõem. Sem licença = **não reaproveitar código** |
| [claude-watch](https://github.com/NirDiamant/claude-watch) | Sem licença detectada | ~70 estrelas, último push 31/10/2026 | Painel de observabilidade do Claude Code | Só como referência |
| [cc-hooks](https://github.com/huypl53/cc-hooks) | MIT | ~0 estrelas, último push 14/06/2026 | Monitor que captura os tipos de evento dos hooks | Útil para a fase 0 (lista de eventos) |

## Geração de time de agentes

Não encontramos nenhuma ferramenta que gere **um time inteiro** de agentes a partir do escopo do projeto com nomes, personalidade e visual. O mais próximo:

- [claude-agent-builder](https://github.com/keysersoose/claude-agent-builder) (MIT, ~50 estrelas, último push 06/03/2026): skill que gera subagentes, skills e hooks a partir de uma descrição.
- [agent-factory](https://github.com/tobrun/agent-factory) (Apache-2.0, último push 28/08/2025): subagente que cria outros subagentes. Parece parado.
- Várias skills avulsas que criam **um** agente por vez.

## Colaborar ou reaproveitar

- **Pixel Agents (MIT):** melhor candidato para colaborar. Dá pra conversar sobre um formato de eventos comum e trocar tradutores. Reaproveitar código é permitido (com aviso de copyright em `CREDITS.md`). **Não** copiar a arte enquanto a licença do pacote Metro City não estiver confirmada.
- **Claude Code Park (MIT):** mesma pilha (Tauri + Rust + PixiJS). Bom para estudar a leitura dos `.jsonl` e a renderização. Pode ser reaproveitado com crédito.
- **pixtuoid (MIT):** referência de `doctor`, instalação de integração e suporte a vários CLIs em Rust.
- **Claude Office Visualizer (MIT):** referência de mapeamento de hooks para animações.
- **Panes e Tabby (MIT):** referência de recursos de terminal.
- **Não reaproveitar código:** Warp, Claude Squad, opcode, CloudCLI e My Virtual Office (AGPL-3.0); disler e claude-watch (sem licença); a arte do Pixel Agent Desk (proprietária) e do clawffice (pacotes pagos). Para essas, a alternativa é escrever do zero ou usar só como inspiração de ideia, sem copiar código nem assets.

## Risco de confusão com o nome

> **Decisão (16/11/2026):** o projeto passou a se chamar **termbay**. Na checagem, o nome estava livre como organização no GitHub, no crates.io e no npm, e não apareceu nenhum produto com ele na web. A marca não foi pesquisada no INPI nem no USPTO. O texto abaixo registra a análise do nome antigo.

O nome **officeAgents** tem risco de confusão **médio a alto**:

- [office-agents](https://github.com/hewliyang/office-agents) (~600 estrelas, licença não confirmada): suplementos de IA para Word, Excel e PowerPoint, e já ocupa o escopo npm `@office-agents`.
- A Microsoft anunciou o **"Office Agent"** no Microsoft 365 Copilot ([Computerworld](https://www.computerworld.com/article/4064781/microsoft-upgrades-m365-copilot-with-agent-mode.html)), que inclusive usa modelos da Anthropic. "Office" + "agent" lembra o Microsoft Office antes do escritório.
- Também há empresas e produtos chamados "OfficeAgent" (por exemplo, vagas da roboco-io no GitHub) e os projetos parecidos "Agent Office" (NosytLabs) e "Claude Code Office" (azorkai, paulrobello).
- Os nomes `officeagents` no npm e no crates.io estavam livres em 16/11/2026.

## Recomendação

**Seguir com o projeto, mas ajustar o foco e o diferencial antes de divulgar.**

- O escritório sozinho **não é mais novidade**: o Pixel Agents já entrega isso com uma comunidade grande. Divulgar o termbay como "escritório pixel art para o Claude Code" nos coloca como mais um clone.
- O diferencial deve ser: **um terminal leve, de uso geral, em binário único**, com o escritório como painel extra, mais o **recrutador** e a **conversa entre agentes**. O README e o pitch devem começar por aí.
- Conversar com o Pixel Agents sobre um formato de eventos compartilhado, sem fundir os projetos: as propostas (extensão/servidor Node e terminal desktop em Rust) são diferentes.
- Avaliar **um nome mais distinto** antes da primeira divulgação, para fugir da associação com o Microsoft Office e do `office-agents` existente. Se o nome ficar, deixar claro no README que não há relação com Microsoft nem Anthropic.
