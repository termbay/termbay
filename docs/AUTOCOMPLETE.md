# Autocompletar estilo Warp

> Status: marco A implementado (fish e zsh, experimental, desligado por padrão), 16/11/2026. As decisões D11–D17 foram aceitas e estão em [ARQUITETURA.md](ARQUITETURA.md). O protocolo implementado, os desvios do desenho e as medições estão na seção 10; os riscos, em [SEGURANCA.md](SEGURANCA.md).
> Base: o que já existe da fase 1 (`core::pty` com portable-pty, ponte Tauri da T7 em `app/src-tauri/src/{bridge,commands}.rs`, `TerminalView` da T8 em `ui/src/terminal/`).

## 1. Objetivo e experiência

Um menu de sugestões sobreposto ao terminal, ancorado no cursor, com as completions **do próprio shell do usuário** e linhas do histórico. Ele deixa o terminal mais confortável sem mudar o comportamento de quem não quer o menu.

```
~/projeto $ git ch█
            ┌──────────────────────────────┐
            │ checkout     trocar de branch│  <- selecionado
            │ cherry-pick  aplicar commit  │
            │ cherry                       │
            │ ── histórico ──              │
            │ git checkout main            │
            └──────────────────────────────┘
```

**Quando aparece:** no primeiro marco, ao apertar **Tab** com o shell parado no prompt. Com um único candidato, o Tab insere o texto direto, sem abrir o menu, como o shell faria. Abrir o menu sozinho enquanto se digita fica para o marco C (seção 7), com fontes que não executam nada (seção 3).

**Teclas com o menu aberto:**

| Tecla | Ação |
|---|---|
| letras, números, `-`, `.` | vão para o shell normalmente e filtram a lista na UI, sem nova consulta |
| `/` | vai para o shell e refaz a consulta (próximo nível do caminho), com debounce de 50 ms |
| ↑ / ↓, Shift+Tab | movem a seleção (não chegam ao shell, então não navegam no histórico dele) |
| Tab, Enter | **inserem** a seleção e fecham o menu. Enter nunca executa o comando: para isso, é preciso apertar Enter de novo |
| Esc | fecha o menu sem mudar nada |
| espaço, ←, →, Backspace antes do início da palavra, Ctrl/Alt/Cmd+qualquer tecla | fecham o menu e seguem para o shell |

O menu também fecha quando a janela perde o foco, quando o terminal rola ou é redimensionado, quando sai um novo prompt e quando o pty termina.

**Nunca aparece:**
- **Em programas de tela inteira** (vim, top, less, htop): o xterm.js está no buffer alternativo (`term.buffer.active.type === "alternate"`).
- **Em prompts que não são do shell** (ssh remoto, REPL do Python, `sudo` pedindo senha, um `zsh` aberto dentro da sessão): depois da marca de "comando executando", o core só volta a "no prompt" quando recebe a marca de fim do **nosso** script, com o nonce da sessão (seção 2). Sem essa marca, o Tab vai direto para o programa.
- **Sem integração ativa** (cmd.exe, nushell, shell com argumentos personalizados que o termbay não reconhece, ou falha no script): o Tab é do shell, como hoje.
- **Durante composição de IME** (acentos, teclas mortas): o atalho só é interceptado fora da composição.

**Desligável:** em `~/.termbay/config.toml` (D8), a mudança vale para as abas abertas depois dela (a recarga ao vivo fica para depois):

```toml
[autocomplete]
enabled = false      # padrão no marco A (D15)
trigger = "tab"      # "tab" ou "ctrl+space" (para quem quer manter o Tab nativo do shell)
history = true       # incluir o histórico do shell nas sugestões
```

## 2. Integração com o shell

O termbay precisa saber três coisas: se o shell está **no prompt**, **o que já foi digitado** e onde fica o cursor. Os terminais modernos resolvem isso com scripts de integração que emitem sequências OSC.

### Protocolos avaliados

| Protocolo | O que reporta | Usado por | Avaliação |
|---|---|---|---|
| **OSC 133** (FinalTerm): `A` início do prompt, `B` início da digitação, `C` comando executando, `D;<código>` fim | regiões do prompt, da entrada e da saída, e código de saída | VS Code, WezTerm, iTerm2, kitty, Ghostty, Windows Terminal; o fish 4 já emite sozinho | padrão de fato. **Emitir**, porque os blocos da fase 5 vão usar |
| **OSC 633** (VS Code): `A`–`D` como no 133, mais `E;<linha>;<nonce>` (linha de comando), `P;Cwd=...` e outras propriedades | o mesmo do 133, mais a linha de comando exata e o diretório | só o VS Code | boa ideia (linha explícita, nonce), mas é um protocolo de um produto. **Não emitir**: reaproveitamos a ideia no OSC privado |
| **OSC 7** `file://host/caminho` | diretório atual | vários terminais e o próprio fish | **emitir** (padrão e simples) |
| **OSC privado do termbay** (número a confirmar, por exemplo `OSC 6973`, sempre com nonce) | handshake, estado confiável (prompt, executando, fim), resposta das completions | só o termbay | **necessário**: o core só confia nestas marcas. As do 133 podem vir de qualquer programa, inclusive de um shell remoto com integração própria |

**Nonce:** o core gera um valor aleatório por pty e o passa na variável `TERMBAY_NONCE`. O script copia o valor para uma variável local do shell e remove a variável de ambiente (`unset`/`set -e`), para que programas filhos não a herdem. Assim, marcas forjadas por `cat arquivo-malicioso` ou por um shell remoto são ignoradas. Um processo do mesmo usuário ainda pode ler o ambiente inicial do shell (`/proc/<pid>/environ` no Linux), mas esse processo já pode fazer coisas piores. O máximo que uma marca forjada consegue é mostrar sugestões erradas (seção 5).

**Handshake:** no primeiro prompt, o script envia `ready` com a versão do protocolo, o shell, a versão do shell e o caminho do arquivo de histórico. Sem `ready`, a integração fica inativa naquela aba e o menu não aparece.

### Injeção sem editar arquivos do usuário

Os scripts ficam versionados no core (`include_str!`) e são gravados na inicialização em uma pasta do termbay por usuário e por versão (diretório de dados do sistema, permissão `0700`, regravados se o hash mudar), nunca nos arquivos de configuração do usuário. A injeção só acontece quando `autocomplete.enabled = true` e o shell é reconhecido. Caso contrário, os argumentos e o ambiente ficam idênticos aos de hoje.

| Shell | Como injetar | Cuidados | Referência |
|---|---|---|---|
| **zsh** | `ZDOTDIR` aponta para uma pasta do termbay com `.zshenv`, `.zprofile`, `.zshrc` e `.zlogin`. Cada arquivo carrega o equivalente do usuário (`$TERMBAY_USER_ZDOTDIR`, que é o `ZDOTDIR` original ou `$HOME`) e devolve o `ZDOTDIR` original antes do `.zshrc` dele | `HISTFILE`, `.zcompdump` e plugins que leem `$ZDOTDIR` precisam ver o valor original; ganchos com `add-zsh-hook precmd/preexec`; widget registrado em todos os keymaps (emacs, viins, vicmd) | VS Code (`shellIntegration-rc.zsh`, MIT), Ghostty (MIT) |
| **fish** | `fish --init-command 'source <script>'`, que roda depois do `config.fish` do usuário e não vaza nada para processos filhos. Alternativa: `XDG_DATA_DIRS` com `fish/vendor_conf.d/`, como fazem o VS Code e o Ghostty | usar os eventos `fish_prompt`, `fish_preexec` e `fish_postexec` em vez de embrulhar o `fish_prompt` do usuário; o fish 4 já emite OSC 133, então só acrescentamos as marcas privadas | VS Code (`shellIntegration.fish`, MIT) |
| **bash** (marco B) | `bash --init-file <script>`. O script carrega o `~/.bashrc` (e o bashrc do sistema quando houver). Como o `--init-file` é ignorado em shell de login (o macOS usa `-l`), o script emula o login carregando `/etc/profile` e o primeiro entre `~/.bash_profile`, `~/.bash_login` e `~/.profile` | o bash 3.2 do macOS não tem `PS0`, então o preexec usa `trap DEBUG`; o Ghostty usa `--posix` + `ENV` como alternativa | VS Code (`shellIntegration-bash.sh`, MIT), bash-preexec (MIT) |
| **PowerShell** (marco B) | `pwsh -NoLogo -NoExit -EncodedCommand <script>`: os perfis carregam normalmente antes. O `-EncodedCommand` evita gravar um `.ps1` e não esbarra na `ExecutionPolicy Restricted`, que bloqueia o `. script.ps1` usado pelo VS Code no Windows PowerShell 5.1 | depende do PSReadLine (padrão no 5.1 do Windows 10 e no pwsh 7); limite de cerca de 32 mil caracteres na linha de comando; validar no ConPTY a passagem dos OSC | VS Code (`shellIntegration.ps1`, MIT) |
| cmd.exe, sh, nushell, outros | sem integração: o menu não aparece | | |

**Código de terceiros:** VS Code, WezTerm, bash-preexec e fzf-tab são MIT. O Ghostty é MIT, **exceto** a integração com o zsh (`src/shell-integration/zsh/`), que começou como cópia da do kitty e é GPL-3.0: ela não pode ser copiada nem adaptada (conferido pela Gabi na AC9). Os MIT podem ser adaptados com o aviso de licença no cabeçalho do script e uma entrada em `CREDITS.md`. iTerm2 (GPL-2.0), kitty (GPL-3.0) e o cliente do Warp (`warpdotdev/Warp`, AGPL-3.0) **não podem** ser copiados nem adaptados: deles, só usamos o protocolo descrito em documentação pública. O WezTerm aparece como `NOASSERTION` na API do GitHub; a Gabi deve conferir o `LICENSE.md` antes de adaptar qualquer trecho.

## 3. Fonte das sugestões

### Perguntar ao shell

| Shell | API | No próprio shell | Em processo separado |
|---|---|---|---|
| fish | `complete -C "<linha até o cursor>"` devolve `candidato<TAB>descrição` | simples: `commandline -cp` dá a linha até o cursor | `fish -c 'complete -C ...'` funciona, mas recarrega a configuração a cada chamada (ou exige um processo fixo) e não vê funções, variáveis nem virtualenv da sessão |
| zsh | não há API direta. Um widget ZLE chama o sistema de completion capturando `compadd` (técnica do fzf-tab, MIT) | viável, com `$BUFFER` e `$CURSOR` | só com `zpty` e um zle falso: frágil e pesado |
| bash | `complete -p <cmd>` + chamar a função registrada com `COMP_WORDS`/`COMP_CWORD`; `compgen` para comandos e arquivos | via `bind -x`, com `READLINE_LINE` e `READLINE_POINT` | `compgen` básico funciona; o bash-completion exige preparar `COMP_*` |
| PowerShell | `TabExpansion2 -inputScript $linha -cursorColumn $pos` devolve `CompletionMatches` (texto, tipo, dica) e o intervalo a substituir | via `Set-PSReadLineKeyHandler` + `GetBufferState` | cada `pwsh` leva de 300 a 800 ms para abrir e ocupa de 60 a 100 MB: fere o "leve" |

**Recomendação (D13): no próprio shell da aba.** O script registra um widget ligado a uma **sequência de tecla privada** que um teclado real nunca gera (por exemplo `CSI 2001 ~`). No PowerShell, o PSReadLine só aceita acordes, então será preciso um acorde raro, a validar no ConPTY. Fluxo:

1. A UI pede `pty_complete`. O core confere que o shell está no prompt, pelas marcas com nonce, e escreve a sequência privada no pty.
2. O widget lê a linha e o cursor, pede as completions ao próprio shell e responde com o OSC privado. A resposta leva nonce, id do pedido, linha, cursor e candidatos codificados sem bytes de controle (escape no estilo do OSC 633: `\\`, `\x3b`, `\xHH` para bytes menores que 0x20), feita em shell puro, sem programas externos. Limite de 500 candidatos ou 64 KB.
3. O widget não altera a linha nem desenha nada.

**Por quê:** usa o contexto exato da sessão (diretório, funções, aliases, virtualenv, completions instaladas pelo usuário), não abre processos extras e não custa nada parado. Custos e riscos:
- **Latência:** é a mesma do Tab nativo. Completions lentas (`kubectl` falando com o cluster, por exemplo) seguram o prompt enquanto rodam, como já acontece hoje. O core desiste em 1,5 s e descarta respostas atrasadas pelo id.
- **Efeitos colaterais:** completions executam código: `make` lê e expande o Makefile, `npm run` lê o `package.json`, `kubectl`, `aws` e `gcloud` vão à rede, e alguns scripts rodam `<cmd> --help`. Por isso, **completions do shell só rodam por ação explícita** (Tab ou o atalho configurado), igual ao Tab nativo, nunca a cada tecla (D14).
- **Teclas digitadas durante a consulta:** o shell processa em ordem. A resposta traz a linha que ele viu, e a UI reaplica o filtro com o que foi digitado depois.
- **Atalhos do usuário:** a sequência privada não colide com nenhuma tecla real; o widget é registrado em todos os keymaps (incluindo o modo vi).

### Histórico (complementar)

- **Origem:** o arquivo de histórico do próprio shell, no caminho reportado pelo handshake (o usuário pode ter mudado `HISTFILE`). Leitura somente, das últimas 10 mil entradas, em memória, compartilhada entre abas do mesmo shell e relida no prompt se o `mtime` mudou. Formatos: fish (`fish_history`), zsh (com `EXTENDED_HISTORY` e metafy), bash (`~/.bash_history`) e PSReadLine (`ConsoleHost_history.txt`).
- **Uso:** linhas inteiras que começam com o que já foi digitado, numa seção separada do menu, ordenadas por frequência e recência.
- **O termbay nunca grava histórico próprio.** `history = false` desliga a fonte.

### Catálogo próprio de comandos: fica para depois

- **Specs existentes:** `withfig/autocomplete` (MIT) e `warpdotdev/command-signatures` (MIT, specs em JSON) têm licença compatível; as specs que o VS Code usa no `terminal-suggest` também são MIT.
- **Problemas:** as specs do Fig são TypeScript com geradores que executam comandos, o que traz de volta o risco de execução. O conjunto completo tem vários MB, o que pesa contra a meta de 20 MB, e duplica o que o shell do usuário já sabe.
- **Recomendação:** reavaliar depois do marco B, medindo a qualidade das sugestões do shell. Se entrar, deve ser um subconjunto em JSON sem geradores, opcional e com créditos.

## 4. Onde cada parte mora

```
 pty (portable-pty) ──bytes──► OscScanner ──bytes (sem o OSC privado)──► batcher ──Channel──► xterm.js
                                  │ marcas com nonce                                         │
                                  ▼                                                          ▼
                       ShellState por pty ◄── pty_complete(id) ◄──────────────── Autocomplete (UI)
                       (prompt|executando)    ──► escreve CSI 2001 ~ no pty                  │
                                  │                                                          ▼
                       resposta do widget ──► CompletionEngine (+ histórico) ──► resultado ► menu no cursor
```

**core (`termbay-core`, sem Tauri):**
- **`shell_integration`:** scripts embutidos, gravação na pasta por usuário, montagem do `SpawnOptions` (args e env) por shell e geração do nonce.
- **`OscScanner`:** máquina de estados pura, como o `OutputBatcher`, sem relógio. Procura `ESC ]` com `memchr`, trata sequências partidas entre leituras e repassa todos os bytes **na mesma ordem e sem alteração**, exceto o OSC privado, que é removido. Os OSC 133 e 7 seguem para o xterm.js. Limita o tamanho de uma sequência aberta (1 MB) e atualiza o `ShellState`. **Só existe nos ptys com integração.**
- **`CompletionEngine`:** junta candidatos do shell e do histórico, remove duplicados, ordena (prefixo antes de aproximado, depois frequência), limita a 50 itens, **remove bytes de controle** e calcula o plano de inserção: só o sufixo quando o candidato começa com a palavra digitada, ou N × DEL (0x7f) seguido do texto quando não começa.
- **Leitores de histórico** por shell.

**app (ponte Tauri):** o scanner fica entre o `PtyEvent::Output` e o batcher, no `PtyBridge::open`, só quando a integração está ligada.

**ui:**
- **`ui/src/autocomplete/controller.ts`:** TypeScript puro, sem DOM. Tem a máquina de estados das teclas (seção 1), o filtro local e a geração dos bytes de inserção. A inserção vai **entre colchetes de colagem** (`ESC[200~ … ESC[201~`) quando o shell ativou o modo, para nenhum caractere disparar atalhos.
- **`ui/src/autocomplete/menu.ts`:** um `listbox` em DOM, posicionado sobre o container do painel (fora do canvas WebGL). A célula do cursor vem de `buffer.active.cursorX/cursorY`. O tamanho da célula vem da largura do `.xterm-screen` dividida por `cols`, porque a API pública do xterm.js não expõe a célula. O menu abre para cima perto da última linha, fica dentro da área do terminal à direita e usa as cores do tema. O foco continua no textarea do xterm.js.
- **Atalho:** vai pelo `attachCustomKeyEventHandler`, encadeado com o handler da área de transferência que já existe. Com o recurso desligado, o handler não é registrado.

**Mudanças no contrato IPC (aditivas, nada muda nos comandos atuais):**

| Item | Tipo | Conteúdo |
|---|---|---|
| `pty-shell` | evento global (raro, como o `pty-exit` de D6) | `{ id, shell: "fish" \| "zsh" \| ..., protocol: 1 }` quando chega o `ready` |
| `pty_complete` | comando assíncrono | entrada `{ id }`. Saída: `{ status: "ok", line, cursor, replaceFrom, items: [{ text, insert, description?, kind: "shell" \| "history" }] }`, ou `{ status: "unavailable" }` quando o shell não está no prompt ou não há integração (a UI então repassa o Tab ao shell), ou `{ status: "timeout" }`. Um pedido novo cancela o anterior |
| `get_config` | já existe (T14) | ganha a seção `[autocomplete]` |
| `PtySession` (`ui/src/transport/types.ts`) | interface | ganha `complete(): Promise<CompletionResult>` e `onShell(listener)`. O transporte de eco devolve `unavailable`, e o WebSocket do `serve` implementa depois o mesmo contrato |
| capabilities | Tauri | o `pty_complete` entra só na janela principal, como os outros |

**Blocos da fase 5:** usam a mesma base: scripts, injeção, nonce, marcas 133 e o `ShellState` com código de saída. Os blocos só acrescentam, na UI, `registerOscHandler(133)` com marcadores do xterm.js (`registerMarker`) para agrupar comando e saída. O `ShellState` também pode alimentar o escritório depois ("rodando `npm test`"), sem mudar o formato de eventos agora.

## 5. Segurança e privacidade

- **Nada sai da máquina:** sem rede, sem servidor e sem porta nova. Tudo passa pelo pty e pelo IPC do Tauri.
- **Sugestão nunca executa:** aceitar só **insere** texto. O core e a UI removem `\r`, `\n` e qualquer byte de controle do texto inserido, e a inserção usa colagem entre colchetes quando disponível. Mesmo uma resposta forjada só consegue mostrar ou inserir texto, e o usuário vê o texto antes de apertar Enter.
- **Nada roda sem ação do usuário:** completions do shell só com Tab ou o atalho, como o Tab nativo. O modo automático (marco C) usa apenas fontes inertes, sem processo: histórico, executáveis do `PATH` e o conteúdo do diretório atual.
- **Marcas confiáveis:** só as que trazem o nonce da sessão, que sai do ambiente logo no início do script.
- **Trigger seguro:** o core só escreve a sequência privada no pty quando o `ShellState` diz "no prompt". Assim ela nunca cai no vim, num REPL ou num ssh.
- **Histórico:** fica só em memória, é lido em modo somente leitura, nunca vai para logs nem para disco do termbay, e nunca é enviado ao Claude, aos agentes ou ao escritório.
- **Scripts:** gravados em pasta do usuário com `0700`. O core confere o dono da pasta antes de usá-la, para outro usuário não plantar código. No Windows, vão inline pelo `-EncodedCommand`.
- **Arquivos do usuário:** nenhum `.zshrc`, `config.fish`, `.bashrc` ou perfil do PowerShell é editado.

## 6. Desempenho

| Situação | Meta |
|---|---|
| **Desligado** | **custo zero:** argumentos e ambiente do shell idênticos aos de hoje, nenhum scanner, nenhum handler de tecla, nenhum DOM extra. Comprovado por teste e pelo benchmark da T16 |
| Ligado, sem usar | scanner com perda de vazão abaixo de 1% no `cat` de 50 MB (T16); script acrescenta menos de 10 ms até o primeiro prompt |
| Tab até o menu | p95 abaixo de 100 ms para `git`, `cd` e `ls` no fish e no zsh numa máquina modesta. O tempo do shell domina; core + IPC + UI abaixo de 10 ms; desiste em 1,5 s |
| Filtrar enquanto digita | dentro de um quadro (16 ms) para até 500 candidatos, na UI, sem IPC |
| Debounce | marco A: 50 ms só para a nova consulta depois de `/`. Marco C (automático): 80 ms depois da última tecla |
| Memória | cache de histórico com no máximo cerca de 2 MB por shell |

## 7. Tarefas

Siglas como na fase 1: **A** = ana-core, **B** = bruno-terminal, **F** = felipe-qa, **G** = gabi-opensource.

### Marco A: fish e zsh, Tab abre o menu, sugestões do shell + histórico, sem catálogo, desligado por padrão

| # | Resp. | Tarefa | Depende de | Pronto quando | Teste |
|---|---|---|---|---|---|
| AC1 | A | `core::shell_integration`: scripts de fish e zsh (marcas 133, OSC 7, marcas privadas com nonce e `ready`), gravação na pasta por usuário e injeção no `SpawnOptions` quando `autocomplete.enabled` | fase 1 (T6, T14) | fish e zsh abrem com a configuração do usuário intacta e mandam `ready` | integração em macOS e Linux com `HOME` falso: o alias do `.zshrc` funciona, o `HISTFILE` não muda, o `ZDOTDIR` volta ao original, `TERMBAY_NONCE` está vazio dentro da sessão; com o recurso desligado, os argumentos são iguais aos de `default_shell()` |
| AC2 | A | `OscScanner` + `ShellState` e ligação na ponte (só com integração) | AC1 | os estados no prompt, executando e fim são confiáveis | unitários no estilo dos do batcher: sequências partidas em pontos aleatórios, bytes iguais e na ordem, só o OSC privado removido, nonce errado ignorado, sequência gigante limitada |
| AC3 | A | widgets de completion: fish (`complete -C`) e zsh (captura de `compadd`, crédito ao fzf-tab), ligados à sequência privada em todos os keymaps, com resposta codificada e limitada | AC1, AC2 | a resposta chega com linha, cursor e candidatos | integração com fish e zsh reais num pty: `ls fix` + trigger lista os arquivos da pasta de teste; nomes com espaço, `;`, quebra de linha e ESC chegam corretos; trigger enviado com um `sleep` rodando é recusado pelo core |
| AC4 | A | `CompletionEngine` + leitores de histórico do fish e do zsh | AC3 | sugestões juntas, ordenadas, limpas e com plano de inserção | unitários com arquivos de exemplo (zsh estendido e metafied); remoção de `\r`/`\n`/ESC; sufixo contra DEL + texto |
| AC5 | A | comando `pty_complete`, evento `pty-shell`, seção `[autocomplete]` no `core::config`, capability | AC4 | a UI consegue pedir e receber completions pelo Tauri | `tauri::test` como os de `commands.rs`: ok, unavailable, timeout, cancelamento; teste da capability só na janela principal |
| AC6 | B | `autocomplete/controller.ts`: teclas, filtro local, bytes de inserção (colagem entre colchetes), IME, buffer alternativo; `PtySession.complete/onShell` nos transportes | AC5 (com dublê antes) | todas as regras da seção 1 funcionam com transporte falso | Vitest: Tab abre, ↑/↓ movem, Enter insere sem `\r`, Esc fecha, buffer alternativo repassa o Tab, `unavailable` repassa o Tab, desligado não registra handler |
| AC7 | B | `autocomplete/menu.ts`: listbox no cursor, abre para cima na borda, tema, `aria`, fecha em blur, scroll e resize | AC6 | o menu aparece no lugar certo com WebGL e com DOM | Vitest da função pura de posição (cursor, célula, área visível → x, y, para cima); jsdom para a lista |
| AC8 | F | e2e, desempenho e segurança do marco A | AC5, AC7 | metas da seção 6 medidas e checklist da seção 5 assinado | e2e (tauri-driver, Linux) em fish e zsh: `ls fix` + Tab, Enter insere e não executa; Tab no vim vai para o vim; desligado, Tab nativo. Benchmark com scanner ligado e desligado; teste do `cat` de uma resposta forjada que não pode mostrar nada |
| AC9 | G | avisos de licença nos scripts adaptados, `CREDITS.md`, conferência de que nada veio de iTerm2, kitty ou Warp, seção no README ("o que o termbay injeta no seu shell e como desligar") | AC1, AC3 | cada trecho adaptado tem origem e licença registradas | checagem na revisão do PR; job de licenças da T5 verde |

### Marco B: bash e PowerShell, ligado por padrão

- **AC10 (A):** script do bash (`--init-file`, emulação de login, `trap DEBUG` no bash 3.2) e widget via `bind -x`.
- **AC11 (A):** PowerShell com `-EncodedCommand`, acorde do PSReadLine e `TabExpansion2`. Leitores de histórico do bash e do PSReadLine.
- **AC12 (F):** testes de integração no Windows da CI (ConPTY passa os OSC?) e no bash 3.2 do macOS.
- **AC13 (G):** créditos do bash-preexec e do VS Code.
- **Fim do marco:** mudar o padrão para `enabled = true` (D15).

### Marco C: abrir enquanto digita (opcional, `trigger = "typing"`)

- **AC14 (A):** fontes inertes no core: executáveis do `PATH` em cache, listagem do diretório atual vinda do OSC 7.
- **AC15 (B):** a UI precisa da linha a cada tecla. No zsh, o gancho `zle-line-pre-redraw` pode reportá-la; no PSReadLine também é possível; o fish e o bash não têm gancho por tecla, então a UI lê o buffer do xterm.js entre a marca `133;B` e o cursor, como o VS Code faz.
- **AC16 (F):** medição do custo por tecla.

## 8. Onde entra no roteiro

**Recomendação: (b), logo depois da fase 1, como "fase 1b", antes do escritório (D11).**

- **Contra (a), antes do marco 2:**
  - Precisa da config (T14) para ser desligável e da linha de base de desempenho (T16) para provar o "custo zero".
  - As abas e a divisão (T12, T13) mudam como os `TerminalView` são criados, e o menu é por painel.
  - Mexeria nos mesmos arquivos que a Ana e o Bruno estão estabilizando.
- **Contra (c), junto com os blocos da fase 5:** atrasa muito um recurso que o Guilherme pediu agora. Além disso, a integração com o shell é a base dos blocos: fazê-la antes deixa a fase 5 menor, não maior.
- **Peso:** o marco A tem cerca de 9 tarefas, sobretudo da Ana. O marco B pode correr em paralelo com o início da fase 2 (o Bruno e o Felipe ficam livres enquanto a Carla começa o escritório), e o marco C fica opcional.

**O que muda no plano da fase 1:** nenhuma tarefa nova. São três cuidados:
1. **T14:** o parser da config ignora seções desconhecidas sem erro, para o `[autocomplete]` entrar depois.
2. **T12/T13:** cada painel tem um container próprio onde uma camada sobreposta possa ser posicionada (já é o natural).
3. **T16:** o script de benchmark aceita rodar com uma opção de configuração, para comparar ligado e desligado.

A tabela "Terminal: recursos por fase" da arquitetura usa uma numeração diferente do roteiro (blocos na "fase 2", autocompletar na "fase 3"). Ela passa a apontar para este documento; vale alinhar essa numeração numa revisão futura.

## 9. Decisões que precisam do aval do Guilherme

| # | Decisão | Recomendação |
|---|---|---|
| D11 | Quando entra | fase 1b, logo depois do marco 3 e antes do escritório |
| D12 | Protocolo | emitir OSC 133 e OSC 7 (padrões) e um OSC privado com nonce para estado e completions; não emitir o 633 do VS Code; injeção sem editar arquivos do usuário |
| D13 | Fonte | o próprio shell da aba, por um widget ligado a uma tecla privada; histórico do shell só para leitura; sem processo extra; catálogo (Fig/Warp, MIT) fica para depois do marco B |
| D14 | Gatilho | Tab abre o menu, e completions do shell só rodam por ação explícita; Enter insere e nunca executa. Abrir sozinho enquanto digita fica para o marco C e usa só fontes inertes. **Atenção:** é menos que "aparece enquanto se digita", por segurança (completions executam código) e para não travar o prompt |
| D15 | Padrão | desligado no marco A (experimental); ligado a partir do marco B, com `enabled = false` para desligar |
| D16 | Onde mora | parser OSC e motor no core, só em ptys com integração; UI só com o menu e o controle de teclas; contrato `pty_complete` + evento `pty-shell` |
| D17 | Plataformas | o marco A sai só com fish e zsh (macOS e Linux), uma exceção temporária ao "multiplataforma"; PowerShell e bash no marco B, antes de ligar por padrão |

## 10. Como ficou o marco A no core (AC1–AC5)

Registro da implementação, para quem revisar (Heitor, Felipe e Gabi).

**Arquivos:** `core/src/shell_integration/` (scripts, injeção, `OscScanner`, `ShellState`, `ShellLink`), `core/src/complete/` (motor e histórico), `core/src/config.rs` (só o `[autocomplete]`, até a T14) e `app/src-tauri/src/complete.rs` (`pty_complete`, `pty-shell`).

**OSC privado:** número `6973`. Formato `ESC ] 6973 ; <nonce> ; <tipo> [; campos] BEL` (ou `ESC \`):

| Tipo | Campos | Quando |
|---|---|---|
| `ready` | protocolo (1), shell, versão, arquivo de histórico | uma vez, no primeiro prompt |
| `prompt` | código de saída do último comando, ou vazio | antes de cada prompt |
| `exec` | | antes de cada comando |
| `complete` | pedido, `insert`/`command`, linha, cursor, início da palavra, depois trios `texto;inserção;descrição` | resposta a um pedido |

Os campos usam `\\` e `\xHH` (os scripts escapam `;`, os bytes menores que 0x20 e 0x7f; o do fish escapa tudo que não é ASCII simples). Posições contam caracteres (code points).

**Diferenças em relação ao desenho:**
- **Gatilho:** em vez de `CSI 2001 ~`, o core digita caracteres do último plano de uso privado do Unicode: U+10FFF0 a U+10FFF9 para os dígitos do número do pedido e U+10FFFD para rodar o widget. O fish 4 descarta sequências `CSI n ~` desconhecidas antes das vinculações, então a sequência do desenho não chegaria ao widget. Nenhum teclado ou IME gera esses caracteres. O número do pedido viaja com o gatilho, e respostas atrasadas são descartadas.
- **Modo vi:** o widget informa `command` quando o editor está no modo normal do vi. O core responde `unavailable`, porque inserir texto ali rodaria comandos do editor.
- **Status `cancelled`:** um pedido substituído por outro mais novo devolve `{ status: "cancelled" }` (a UI trata status desconhecido como timeout).
- **Limites:** até 500 itens do shell (o que a UI filtra localmente, como na seção 6) e até 50 do histórico, em vez de 50 no total.
- **Enter pendente:** quando o usuário envia uma quebra de linha, o core deixa de considerar o shell "no prompt" até a próxima marca `prompt`. Assim o gatilho nunca cai num comando que começou antes da marca `exec`.

**Janela do nonce:** no zsh, ele sai do ambiente na primeira linha do nosso `.zshenv`, depois só do `/etc/zshenv`. No fish, o `--init-command` roda depois do `config.fish`, então programas abertos pelo `config.fish` ainda herdam a variável. É o mesmo processo do usuário; a alternativa (`vendor_conf.d` via `XDG_DATA_DIRS`) também roda depois do `conf.d` do usuário.

**Marcas 133:** o zsh emite `A`, `C` e `D`; o `B` (fim do prompt) fica para os blocos da fase 5, porque exige mexer no `PS1`. O fish 4 já emite as suas, e o script só as emite no fish 3.

**Maiúsculas e zsh sem compsys (ajustes depois do primeiro teste do Guilherme):**
- **zsh com `compinit`:** durante o pedido, o widget usa o contexto `:completion:_termbay_compwidget:*` com `matcher-list '' 'm:{[:lower:][:upper:]}={[:upper:][:lower:]}'`. Primeiro vêm as correspondências exatas e, só quando não há nenhuma, as que ignoram maiúsculas. O estilo é apagado logo depois, então o Tab normal do usuário não muda.
- **zsh sem `compinit`:** o compsys não é carregado, porque isso mudaria o Tab do usuário. O widget usa as globs do próprio zsh, sem diferenciar maiúsculas (`(#i)`, com `extended_glob` local), para arquivos e pastas. Na primeira palavra do comando, inclui também comandos do `$PATH`, aliases, builtins e funções. Nada é executado; a única expansão é a de `~/`.
- **fish:** o `complete --do-complete` já ignora maiúsculas e põe os prefixos exatos primeiro; o script não faz nada a mais.
- **Shell padrão no Unix:** vem primeiro o shell de login da conta (`pw_shell`, via `getpwuid_r`), depois `$SHELL` e por fim a lista fixa. Um app aberto pelo Finder herda um `$SHELL` que pode não ser a escolha do usuário.

**Licenças dos scripts (AC9, revisão da Gabi):** comparei os três scripts com o fzf-tab, o zsh-capture-completion, o VS Code, o Ghostty, o kitty e o iTerm2.
- **`termbay.zsh`:** o miolo do `_termbay_compadd` (a lista do `zparseopts` com as mesmas opções na mesma ordem, o repasse de `-O`/`-A`/`-D` ao builtin, a coleta com `builtin compadd -A ... -D ...` e o gancho via `functions[compadd]`) é uma adaptação reconhecível do `-ftb-compadd` do fzf-tab. Por isso, o arquivo leva o aviso MIT completo do fzf-tab e entrou em `CREDITS.md`. O resto do arquivo é próprio.
- **`zshenv.zsh`:** o truque do `ZDOTDIR` temporário é técnica comum (VS Code, MIT). O kitty e o Ghostty fazem algo parecido em arquivos GPL-3.0, mas o nosso não tem os traços deles (builtins entre aspas, `always`, `autoload` do próprio arquivo); só a forma mínima de restaurar uma variável coincide. Fica como código próprio, com uma nota no cabeçalho.
- **`termbay.fish`:** código próprio. A linha do OSC 7 coincide com a forma natural no fish (`$hostname` e `string escape --style=url`), também usada no Ghostty, cujo arquivo do fish é MIT.
- O escape dos campos segue a convenção do OSC 633 do VS Code (MIT), com código próprio. OSC 7 e OSC 133 são protocolos. Nada veio do Warp, do iTerm2 nem do kitty.
- **Para quem mexer nesses scripts:** não abra o código do kitty, do iTerm2 nem da integração com o zsh do Ghostty como referência. Se adaptar algo de um projeto MIT, registre no cabeçalho e em `CREDITS.md`.

**Medições do AC8 (16/11/2026).** Máquina: Apple M5 (10 núcleos), macOS 26, zsh 5.9 e fish 4.7.1; Linux: contêiner Ubuntu 22.04 (aarch64) no mesmo Mac, zsh 5.8.1. Build `--release`. Como repetir: `cargo test --release -p termbay-core --test throughput -- --ignored --nocapture --test-threads=1` e `cargo test --release -p termbay-core --test shell_integration completion_latency -- --ignored --nocapture`.

| Medida | macOS | Linux | Meta |
|---|---|---|---|
| `cat` de 50 MB pelo pty + batcher, log colorido: desligado / ligado (mediana de 12, ordem alternada) | 201 / 215 MB/s (−6,5%) | 226 / 225 MB/s (+0,7%) | perda < 1% |
| idem, texto simples | 214 / 217 MB/s (−1,5%) | 179 / 186 MB/s (−3,7%) | perda < 1% |
| thread de leitura ocupada, log colorido: desligado / ligado | 15% / 26% | 23% / 34% | |
| CPU extra do scanner em 50 MB, log colorido (um ESC a cada ~20 bytes) | 25,6 ms = 9,9% do tempo do pty | 23,9 ms = 10,0% | |
| idem, texto simples | 1,1 ms = 0,4% | 0,8 ms = 0,35% | |
| p95 de `pty_complete` no core (pedido até a resposta montada), `HOME` temporário, zsh com `compinit` | `git ch` 29,6 ms; `cd ` 8,9 ms; `ls ` 12,0 ms | `git ch` 24,6 ms; `cd ` 9,0 ms; `ls ` 9,5 ms | < 100 ms |
| idem, fish | `git ch` 28,2 ms; `cd ` 8,8 ms; `ls ` 16,3 ms | não medido (ver P2 em [SEGURANCA.md](SEGURANCA.md)) | < 100 ms |

- **Vazão:** nenhuma perda medida. O pty (cerca de 200 MB/s nos dois sistemas) é o gargalo, e a diferença entre ligado e desligado fica dentro do ruído; o "ligado" sair mais rápido no log colorido do macOS se repete, mas a causa não foi confirmada (suspeita: leituras um pouco mais lentas deixam o batcher juntar mais).
- **Folga:** o scanner para em todo ESC, então numa saída colorida ele custa cerca de 10% do tempo de CPU do caminho. Um `memchr::memmem` do prefixo inteiro faz a mesma busca em 2,3–2,5 ms (cerca de 10x menos). Ver P5 em [SEGURANCA.md](SEGURANCA.md).
- **Desligado:** o caminho é o mesmo de antes por construção (`ShellSupport::prepare` devolve `None`, sem scanner, e a saída vai direto ao batcher), e o teste `disabled_bridge_passes_private_sequences_through` confirma que até um OSC 6973 chega intacto à UI.
- **Latência:** o tempo do próprio shell domina (`git ch` chama o `git`). Com a configuração real do Guilherme (zsh sem `compinit`, fish padrão), o p95 ficou entre 1,7 e 26,4 ms.
