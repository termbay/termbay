# Arquitetura do termbay

> Status: proposta, antes do primeiro código. Atualizado em 16/11/2026.

## Visão

O termbay é um **terminal de verdade**, leve e open source, no estilo do Warp. Nele dá pra rodar qualquer comando: `git`, `npm`, `vim`, `ssh` e o próprio `claude`.

O diferencial é o **escritório**: quando o Claude Code trabalha com agentes, um painel mostra um escritório em 2D. Cada agente é um funcionário com nome, mesa e personalidade, e balões mostram o que cada um está fazendo e com quem está conversando.

Ao iniciar um projeto novo, o termbay lê o escopo e **monta o time de agentes automaticamente**.

```
┌──────────────────────────────┬─────────────────────┐
│ ~/projeto $ git status       │   Escritório        │
│ ~/projeto $ npm test         │  Ana   "lendo api"  │
│ ~/projeto $ claude           │  Bruno "testes"     │
│ > crie a tela de login...    │  Carla (livre)      │
└──────────────────────────────┴─────────────────────┘
       terminal (sempre)          aparece com agentes
```

## Princípios

1. **Terminal primeiro.** Ele precisa ser bom mesmo sem agentes. O escritório é um painel extra, que aparece sozinho quando há agentes e pode ser fixado ou escondido com um atalho.
2. **Cada usuário usa a própria conta do Claude.** O app nunca faz login, nunca guarda token, senha ou chave e não tem servidor próprio no meio.
3. **Leve, roda em qualquer máquina.** macOS, Linux e Windows, inclusive em máquinas modestas.
4. **Local e privado.** Tudo roda na máquina do usuário. Sem telemetria por padrão.
5. **Totalmente open source.** Código, catálogo de cargos e arte com licenças livres.

## Conta do Claude

O termbay usa o **Claude Code oficial** (`claude`) que o usuário instala e no qual faz login com a própria conta, seja assinatura Pro/Max ou chave de API.

- **Na primeira execução:** o app verifica se o `claude` está instalado (`claude --version`) e logado. Se não estiver, mostra como instalar e fazer login, e para por aí.
- **Recrutador:** também usa a conta do usuário, chamando `claude -p` (modo sem interface).
- **Por que não usar o Claude Agent SDK direto:** até onde se sabe, a Anthropic não permite que apps de terceiros ofereçam o login do claude.ai pelo SDK, só por chave de API. Pelo CLI oficial, os dois tipos de conta funcionam. **Confirmar nos termos atuais antes do lançamento.**

## Visão geral dos componentes

```
termbay/
├── core/         Rust, binário único `termbay`
│   ├── pty         abre o shell do usuário (portable-pty)
│   ├── eventos     servidor local que recebe eventos dos hooks
│   ├── estado      agentes, mesas, o que cada um está fazendo
│   └── cli         subcomandos: hook, init, serve, doctor
├── ui/           TypeScript, sem framework pesado
│   ├── terminal    xterm.js (abas, divisão de tela, busca)
│   └── escritorio  PixiJS, 2D pixel art
├── app/          Tauri: janela desktop que usa a ui/
├── recruiter/    catálogo de cargos e geração do time
├── assets/       sprites e mapas do escritório (CC0 ou próprios)
└── docs/
```

```
          ┌─────────────── app (Tauri) ───────────────┐
          │  ui/terminal (xterm.js)   ui/escritorio   │
          └──────────▲───────────────────────▲────────┘
                     │ bytes do pty          │ eventos normalizados
          ┌──────────┴───────────────────────┴────────┐
          │                 core (Rust)                │
          │   pty  ◄─── shell do usuário ───► claude   │
          │   eventos ◄── `termbay hook` ◄── hooks│
          └────────────────────────────────────────────┘
```

### core (Rust)

- **pty:** abre o shell padrão do usuário (zsh, fish, bash, PowerShell) com `portable-pty`, que também funciona no Windows. Cada aba ou divisão de tela tem seu próprio pty.
- **eventos:** servidor em `127.0.0.1`, numa porta aleatória, com um token por sessão. Recebe os eventos crus dos hooks e os traduz para o formato próprio (ver abaixo).
- **estado:** mantém quem está no escritório, o que cada um está fazendo e o histórico recente de balões.
- **cli:**
  - `termbay`: abre o app.
  - `termbay hook`: chamado pelos hooks do Claude Code. Lê o JSON da entrada padrão e envia ao servidor local. Por ser o próprio binário, o usuário não precisa de Node nem Python.
  - `termbay init`: roda o recrutador no projeto atual.
  - `termbay serve`: modo navegador. Abre a mesma interface em `localhost`, pra máquinas sem interface gráfica, SSH ou máquinas fracas.
  - `termbay doctor`: verifica `claude`, login, hooks e shell.

### ui (TypeScript)

- **terminal:** xterm.js, o mesmo componente do terminal integrado do VS Code. Programas interativos, cores e atalhos funcionam.
- **escritorio:** PixiJS, 2D isométrico ou top-down em pixel art. Quando a janela está minimizada ou o painel escondido, ele para de desenhar.
- **Sem framework pesado:** TypeScript puro ou algo mínimo, pra manter o app leve.

### app (Tauri)

- **Por que Tauri:** ele usa o navegador que já vem no sistema (WebView2, WKWebView ou WebKitGTK), então o instalador fica em cerca de 10–15 MB, contra mais de 150 MB do Electron, e usa bem menos memória.
- **Mesma interface no navegador:** a pasta `ui/` é servida igual no `termbay serve`.

## Formato de eventos

O escritório não conhece o formato do Claude Code, só este formato próprio. Os hooks são traduzidos pelo core.

```json
{ "v": 1, "sessao": "s1", "agente": "Ana", "tipo": "inicio",     "cargo": "backend" }
{ "v": 1, "sessao": "s1", "agente": "Ana", "tipo": "ferramenta", "acao": "lendo", "alvo": "src/api.ts" }
{ "v": 1, "sessao": "s1", "agente": "Ana", "tipo": "mensagem",   "para": "Bruno", "texto": "terminei a API" }
{ "v": 1, "sessao": "s1", "agente": "Ana", "tipo": "fim",        "resultado": "ok" }
```

| tipo         | no escritório                                            |
|--------------|----------------------------------------------------------|
| `inicio`     | o funcionário chega e senta na mesa                      |
| `ferramenta` | animação e balão ("lendo api.ts", "rodando testes")      |
| `mensagem`   | balão sai de um personagem e vai até o outro             |
| `espera`     | personagem pensativo ou aguardando aprovação do usuário  |
| `fim`        | volta ao estado livre (café, conversa no corredor)       |

**Vantagens:**
- **Mudanças no Claude Code:** só o tradutor precisa mudar.
- **Outras ferramentas:** Codex, Gemini CLI e outras podem ganhar seu próprio tradutor no futuro, o que é um bom ponto de entrada pra quem quiser contribuir.

## Integração com o Claude Code (hooks)

- **Instalação:** o `termbay init` (ou `doctor --fix`) adiciona ao `.claude/settings.json` do projeto hooks que chamam `termbay hook <evento>`.
- **Eventos de interesse:** início e fim de subagente, uso de ferramenta (antes e depois), pedido de permissão, erros de ferramenta, tarefas e fim da sessão. Lista final e motivos em [FASE0-HOOKS.md](FASE0-HOOKS.md). Não registrar `MessageDisplay` (bloqueia a exibição do texto) nem `WorktreeCreate` (substitui o comportamento padrão).
- **Conversa entre agentes:** toda mensagem passa pela ferramenta `SendMessage`, e o hook `PreToolUse` traz remetente, destinatário e texto completo. Funciona também entre subagentes nomeados, sem times (validado na fase 0, Claude Code 2.1.311). Em times de agentes (só em sessão interativa) é ainda melhor: o nome do teammate vem em todos os eventos, a criação devolve uma cor para cada um e o `SendMessage` já traz remetente, destinatário, cores e resumo.
- **Fallback:** as transcrições da sessão em `~/.claude/projects/` (somente leitura) trazem o que os hooks não trazem: falas intermediárias dos subagentes e a mensagem recebida. Formato não documentado, então fica fora do MVP, como enriquecimento opcional.

## Recrutador automático

`termbay init` em um projeto:

1. **Escopo:** lê o README, uma descrição passada pelo usuário ou faz algumas perguntas curtas.
2. **Proposta:** chama `claude -p` pedindo um `time.json` com cargos, nomes, personalidade e visual de cada funcionário, escolhidos do catálogo.
3. **Aprovação:** mostra o time pro usuário, que pode trocar, remover ou adicionar funcionários.
4. **Geração:** cria os arquivos `.claude/agents/<nome>.md` e salva o `time.json` em `.termbay/`.

```json
{
  "projeto": "loja-online",
  "time": [
    { "nome": "Ana",   "cargo": "backend",  "personalidade": "metódica", "visual": "ana-01" },
    { "nome": "Bruno", "cargo": "qa",       "personalidade": "curioso",  "visual": "bruno-02" },
    { "nome": "Carla", "cargo": "revisora", "personalidade": "direta",   "visual": "carla-01" }
  ]
}
```

- **Catálogo próprio de cargos:** fica em `recruiter/catalogo/`, como arquitetura, frontend, backend, QA, revisão, segurança e documentação. Pode adaptar modelos do [ECC](https://github.com/affaan-m/ECC) (MIT), mantendo os créditos. O ECC não é instalado inteiro, porque traz hooks próprios que entram em conflito com os nossos e carrega contexto demais.
- **Tamanho do time:** por padrão de 3 a 5 agentes, configurável. O consumo sai da conta do usuário.
- **Sem dependência de busca externa:** o recrutador não depende de skills de busca em tempo de execução. O catálogo versionado garante resultados previsíveis.

## Terminal: recursos por fase

| Fase | Recursos |
|------|----------|
| 1    | abas, divisão de tela, temas, histórico, busca, configuração em arquivo |
| 2    | blocos (comando e saída agrupados, como no Warp) via integração com o shell (OSC 133), paleta de comandos |
| 3    | autocompletar visual (desenho e proposta de antecipar para a fase 1b em [AUTOCOMPLETE.md](AUTOCOMPLETE.md)), atalho "mandar este erro pro Claude" |

## Metas de leveza

- **Binário/instalador:** menos de 20 MB.
- **Memória:** baixa quando parado. O escritório não desenha quando está escondido.
- **3D:** desligado por padrão (Three.js como opção futura).
- **Dependências do usuário:** zero, além do `claude`. Sem Node ou Python pra rodar.

## Roteiro

0. **Teste dos hooks (1–2 dias):** registrar todos os eventos de uma sessão real com agentes e times e decidir o que dá pra animar. Feito em 16/11/2026 com subagentes e com um time in-process em sessão interativa ([FASE0-HOOKS.md](FASE0-HOOKS.md)).
1. **Terminal base:** Tauri + core + xterm.js, com abas e divisão de tela. Plano, tarefas e decisões em [FASE1-PLANO.md](FASE1-PLANO.md).
   - **Autocompletar (marco A em andamento, logo depois do marco 1):** menu de sugestões do próprio shell, via integração com o shell (OSC 133 + OSC privado), base também dos blocos da fase 5. Desenho, tarefas e decisões em [AUTOCOMPLETE.md](AUTOCOMPLETE.md).
2. **Escritório:** painel PixiJS lendo eventos dos hooks, com personagens e balões.
3. **Recrutador:** `termbay init` com catálogo de cargos.
4. **Conversa entre agentes:** times do Claude Code nos balões.
5. **Extras:** blocos estilo Warp, modo navegador polido, tradutores para outras ferramentas, 3D opcional.

## Riscos e pontos em aberto

- **Eventos dos hooks:** a fase 0 mostrou que os hooks cobrem ferramentas, subagentes e mensagens enviadas ([FASE0-HOOKS.md](FASE0-HOOKS.md)). Times de agentes in-process também foram validados, com `TeammateIdle`. Falta só o modo de painel dividido (tmux/iTerm2), em que cada teammate é um processo separado.
- **Termos da Anthropic:** confirmar que usar o `claude` oficial a partir de um app de terceiros é permitido com assinatura.
- **Projetos parecidos:** pesquisa feita em 16/11/2026, ver [PROJETOS-PARECIDOS.md](PROJETOS-PARECIDOS.md). O escritório sozinho já existe (Pixel Agents e outros); o diferencial é o terminal, o recrutador e a conversa entre agentes, e o nome pode confundir.
- **Arte:** sprites próprios ou CC0. Nada de assets de jogos existentes.
- **WebView no Linux:** o WebKitGTK varia entre distribuições, então é preciso testar cedo.
- **Licenças dentro do Tauri:** alguns crates do Tauri parecem usar MPL-2.0, que não está na lista do `CLAUDE.md`. Confirmar com `cargo deny` no primeiro build (ver D10 em [FASE1-PLANO.md](FASE1-PLANO.md)).

## Decisões

| Decisão           | Escolha                 | Status   |
|-------------------|-------------------------|----------|
| Desktop           | Tauri + modo navegador  | aceito (16/11/2026) |
| Core              | Rust                    | aceito (16/11/2026) |
| Terminal          | xterm.js + portable-pty | aceito (16/11/2026) |
| Escritório        | PixiJS 2D               | proposto |
| Conta do Claude   | CLI oficial do usuário  | proposto |
| Licença           | MIT                     | aceito (16/11/2026): simples, permissiva, compatível com o ECC |
| Nome              | termbay                 | aceito (16/11/2026): substitui "officeAgents", que confundia com o Microsoft Office e com o projeto `office-agents`. Ver [PROJETOS-PARECIDOS.md](PROJETOS-PARECIDOS.md) |
| Versão do Tauri   | 2.x, minor estável fixada | aceito (16/11/2026): `Channel` para streaming e capabilities nos três sistemas (D1) |
| Frontend          | TypeScript puro, sem framework | aceito (16/11/2026): abas e divisão são pouca interface; menos peso e menos dependências (D2) |
| Node              | só no build (Vite, npm com lockfile) | aceito (16/11/2026): xterm.js vem do npm; o binário embute arquivos estáticos e roda sem Node (D3) |
| xterm.js          | `@xterm/*` empacotado pelo Vite, WebGL com volta ao DOM | aceito (16/11/2026): WebGL é o mais rápido; DOM cobre WebViews sem WebGL (D4) |
| Pastas e Cargo    | workspace: `core/` (lib sem Tauri), `app/src-tauri/` (binário `termbay`), `ui/` | aceito (16/11/2026): core testável e reutilizável no `serve` e nos hooks (D5) |
| Pty ↔ UI          | comandos Tauri + um `Channel` binário por pty, com controle de fluxo | aceito (16/11/2026): ordenado e sem JSON/base64 na saída (D6) |
| Modo navegador    | fora da fase 1; entra com o servidor de eventos | aceito (16/11/2026): usa o mesmo servidor local com token; não abrir porta só para isso (D7) |
| Config do usuário | `~/.termbay/config.toml` (`TERMBAY_CONFIG`) | aceito (16/11/2026): mesmo caminho nos três sistemas; inválido nunca impede de abrir (D8) |
| Plataformas mínimas | macOS 10.15+, Windows 10 1809+, Linux com webkit2gtk-4.1 | aceito (16/11/2026): mínimos do Tauri 2 e do ConPTY; AppImage fora por tamanho (D9) |
| Licenças das deps | `cargo-deny` + checagem npm; MPL-2.0 só por exceção por crate | aceito (16/11/2026): MPL vem do Tauri e precisa de aceite explícito (D10) |
| Autocompletar: roteiro | marco A logo depois do marco 1 da fase 1, antes das abas | aceito (16/11/2026): pedido do Guilherme; a config mínima do `[autocomplete]` entra junto e a T14 a absorve depois. Recomendação original era a fase 1b (D11, [AUTOCOMPLETE.md](AUTOCOMPLETE.md)) |
| Integração com o shell | OSC 133 + OSC 7 + OSC privado com nonce, injetado sem editar arquivos do usuário | aceito (16/11/2026): padrões que os blocos vão usar, mais estado confiável que programas e shells remotos não forjam (D12) |
| Fonte das sugestões | o próprio shell da aba (widget em tecla privada) + histórico só leitura; catálogo depois | aceito (16/11/2026): contexto exato da sessão, sem processo extra e custo zero parado (D13) |
| Gatilho do autocompletar | Tab abre o menu; Enter insere e nunca executa; abrir enquanto digita só com fontes inertes | aceito (16/11/2026): completions do shell executam código e podem travar o prompt (D14) |
| Autocompletar por padrão | desligado no marco A, ligado a partir do marco B | aceito (16/11/2026): injetar no shell pode quebrar configurações; ligar só depois dos quatro shells testados (D15) |
| Autocompletar: onde mora | parser OSC e motor no core (só com integração), menu na UI, `pty_complete` + evento `pty-shell` | aceito (16/11/2026): core testável sem Tauri e UI sem lógica de shell (D16) |
| Autocompletar: plataformas | marco A só fish e zsh; bash e PowerShell no marco B | aceito (16/11/2026): exceção temporária ao multiplataforma para entregar cedo, fechada antes de ligar por padrão (D17) |
| CSP dos estilos | `dangerousDisableAssetCspModification: ["style-src"]` | aceito (16/11/2026, em vigor desde o marco 1): o nonce do Tauri anularia o `'unsafe-inline'` de que o xterm.js precisa; os scripts seguem protegidos por hash (P7 em [SEGURANCA.md](SEGURANCA.md)) |
| Limite de painéis | sem limite de abas; teto de 64 ptys no core | aceito (16/11/2026): cada pty custa um processo e quatro threads; o teto só barra acidentes (D18, [FASE1-PLANO.md](FASE1-PLANO.md) seção 6) |
| Última aba | fechar a última aba encerra o app nos três sistemas | aceito (16/11/2026): uma janela só na fase 1; ficar aberto sem janela no macOS não traz ganho (D19) |
| Shell que sai | código 0 fecha o painel; outro código mantém com aviso | aceito (16/11/2026): sair de propósito não deixa lixo, e um erro ao abrir continua legível (D20) |
| Confirmar ao fechar | painel ou aba com programa rodando pede confirmação; janela no marco 3 | aceito (16/11/2026): protege `npm run dev` e `ssh` de um Cmd+W por engano, sem plugin de diálogo (D21) |
| Config no marco 2 | só `get_config` com o `[autocomplete]`; T14 completa no marco 3 | aceito (16/11/2026): corrige o `trigger` ignorado pela UI sem antecipar a config inteira (D22) |
| Diretório de painéis novos | herdado do processo do shell (`pty_info`), nunca do OSC 7 | aceito (16/11/2026): o OSC 7 pode ser forjado pela saída; o sistema operacional não (D23) |
| Atalhos | Cmd no macOS, Ctrl+Shift no Linux e no Windows, handler global antes do xterm.js, menu próprio no macOS | aceito (16/11/2026): coerente com o copiar/colar e sem tomar teclas do shell (D24) |
| Controle de fluxo | 512 KiB para pausar, 128 KiB para retomar, por pty, contados antes do agrupador | aceito (16/11/2026): detalha a D6; Ctrl+C abaixo de 1 s com vários painéis e memória limitada (D25) |
| Divisão de tela | árvore binária meio a meio; escritório fora da árvore | aceito (16/11/2026): o mais simples que cobre o marco 2 (D26) |
