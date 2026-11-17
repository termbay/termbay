# Fase 1: plano do terminal base

> Status: proposta, 16/11/2026. As decisões da seção 2 entram em [ARQUITETURA.md](ARQUITETURA.md) como "proposto" e só valem depois do aceite.
> Objetivo: um terminal de uso geral, leve e em binário único (Tauri + core em Rust + xterm.js), com abas e divisão de tela, nos três sistemas. É o nosso diferencial ([PROJETOS-PARECIDOS.md](PROJETOS-PARECIDOS.md)); o escritório vem depois.

## 1. Escopo

**Entra:**
- App desktop Tauri que abre o shell padrão do usuário (zsh, bash, fish, PowerShell) num pty, com xterm.js.
- Abas e divisão de tela (horizontal e vertical), cada painel com seu próprio pty.
- Recursos de terminal da fase 1 na arquitetura: temas, histórico (scrollback configurável), busca no scrollback e configuração em arquivo.
- Atalhos por sistema (Cmd no macOS, Ctrl+Shift no Linux e no Windows).
- CI em macOS, Linux e Windows, medição do tamanho do binário, checagem de licenças das dependências e arquivo de licenças de terceiros dentro do pacote.

**Fica de fora (e onde vai entrar):**

| Fora da fase 1 | Fase | O que a fase 1 deixa pronto para não atrapalhar |
|---|---|---|
| Servidor de eventos, `termbay hook`, tradutor | 2 | `core/` é uma biblioteca sem Tauri, com espaço para os módulos `events` e `state`. O `main` do binário decide entre subcomando e janela antes de iniciar o Tauri, então `termbay hook` roda sem abrir janela. Na fase 1 não há nenhuma porta aberta |
| Painel do escritório (PixiJS) | 2 | O layout da janela tem dois espaços: área de terminais e um painel lateral vazio e escondido (`display: none`, sem custo). A pasta `ui/src/office/` só nasce na fase 2 |
| Recrutador, `termbay init`, `doctor` | 3 | O parser da CLI (em `core::cli`) já tem o lugar dos subcomandos; na fase 1 só existem `termbay`, `--version` e `--help` |
| Modo navegador (`termbay serve`) | 2 ou depois (D7) | A UI fala com o pty por uma interface `PtyTransport`; a fase 1 só implementa a do Tauri. O `serve` vai trazer uma implementação por WebSocket, sem mexer no terminal |
| Blocos (OSC 133), paleta de comandos | 2 | nada específico |
| Instalar `termbay` no PATH, assinatura de código, publicação de releases, atualização automática, AppImage | depois | A CI gera os pacotes sem assinatura, só para medir e testar |

## 2. Decisões a fechar antes de começar

| # | Decisão | Recomendação | Motivo |
|---|---|---|---|
| D1 | Versão do Tauri | **Tauri 2.x**, última minor estável, fixada no `Cargo.lock` e no `package-lock.json` (`tauri`, `@tauri-apps/cli` e `@tauri-apps/api` na mesma minor) | A 2.x tem `Channel` para streaming, sistema de permissões (capabilities) e suporta os três sistemas |
| D2 | Frontend | **TypeScript puro, sem framework.** Abas e divisão são pouca interface; o escritório será canvas (PixiJS). Se um dia surgir uma tela de configurações grande, reavaliar algo mínimo (Preact) com nova decisão | Menos peso, menos dependências para auditar |
| D3 | Node no build | **Aceito só como ferramenta de build** (Vite + TypeScript + `@tauri-apps/cli`), com versão LTS fixada em `.nvmrc` e `engines`, gerenciador **npm** com lockfile e `npm ci`. O resultado é HTML/JS/CSS estático embutido no binário pelo Tauri: nada de Node em tempo de execução | O xterm.js e os addons são distribuídos pelo npm; copiar arquivos minificados à mão dificulta atualizar e auditar licenças. O princípio é "sem Node para rodar", e isso se mantém |
| D4 | Empacotamento do xterm.js | Pacotes `@xterm/xterm`, `@xterm/addon-fit`, `@xterm/addon-search`, `@xterm/addon-webgl` e `@xterm/addon-unicode11` (todos MIT), empacotados pelo Vite em `ui/dist/`. Renderizador **WebGL**, com volta automática ao renderizador DOM se o WebGL falhar ou perder o contexto | WebGL é o mais rápido; o DOM cobre WebViews sem WebGL (alguns WebKitGTK) |
| D5 | Estrutura de pastas e workspace Cargo | Workspace Cargo na raiz com dois membros: `core/` (crate `termbay-core`, biblioteca sem Tauri: `pty`, `config`, `cli`; depois `events` e `state`) e `app/src-tauri/` (crate do binário `termbay`: `main` fino, comandos Tauri, ponte com a UI). `ui/` com o frontend (`terminal/`, `layout/`, `transport/`). Um `Cargo.lock` e um perfil de release único: `lto = true`, `codegen-units = 1`, `strip = true`, `opt-level = "s"` (ajustar depois de medir); `panic` continua em `unwind` para que a falha de uma thread de pty não derrube a janela | Mantém a visão da arquitetura (core com pty, eventos, estado e CLI) e deixa o core testável e reutilizável no `serve` |
| D6 | Pty ↔ UI | **Comandos Tauri** para o controle (`pty_open`, `pty_write`, `pty_resize`, `pty_close`) e um **`Channel` do Tauri por pty** para a saída, enviada como bytes crus (`ArrayBuffer`), sem JSON nem base64. No core, uma thread de leitura por pty agrupa a saída (até ~8 ms ou 64 KB por envio). Controle de fluxo simples: a UI confirma os bytes já desenhados pelo xterm.js e o core pausa a leitura acima de um limite (ex.: 1 MB pendente). Eventos globais do Tauri só para avisos raros (pty encerrado) | O `Channel` é ordenado e feito para streaming; eventos globais são mais lentos e serializam tudo em JSON |
| D7 | Modo navegador (`termbay serve`) | **Fora da fase 1.** Entra junto ou logo depois do servidor de eventos (fase 2), porque usa o mesmo servidor local em `127.0.0.1` com token; o polimento continua na fase 5 | Na fase 1 seria abrir uma porta só para isso. As fronteiras de D5 e D6 já deixam o caminho livre |
| D8 | Configuração do usuário | `~/.termbay/config.toml` (crate `toml`), com caminho alternativo por `TERMBAY_CONFIG`. Arquivo ausente = padrões; arquivo inválido = padrões e aviso visível, nunca falha ao abrir. Recarrega ao reiniciar (recarga ao vivo fica para depois) | Mesmo nome nos três sistemas, no mesmo padrão do `~/.claude` e do `.termbay/` do projeto |
| D9 | Plataformas mínimas | macOS 10.15+, Windows 10 1809+ (exigência do ConPTY) com WebView2, Linux com `webkit2gtk-4.1` (Ubuntu 22.04+, Debian 12+, Fedora 38+). Pacotes da fase 1: `.app`/`.dmg`, instalador NSIS e `.deb`/`.rpm` | São os mínimos do Tauri 2 e do ConPTY. AppImage embute o WebKitGTK e estoura os 20 MB, por isso fica fora |
| D10 | Licenças das dependências | `cargo-deny` (licenças, crates proibidos, avisos de segurança, origem) e `license-checker-rseidelsohn` no npm, com lista permitida: MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, CC0-1.0, Unicode-3.0. **Exceção por crate, nunca global**, para MPL-2.0 que vem do próprio Tauri (provavelmente `cssparser`, `selectors`, `option-ext`; confirmar no primeiro `cargo deny check`), sem modificar o código desses crates. `cargo-about` gera o arquivo de licenças de terceiros que vai dentro do pacote | MPL-2.0 é copyleft fraco, por arquivo, e compatível com distribuir um app MIT, mas não está na lista do `CLAUDE.md`, então precisa de aceite explícito |

## 3. Tarefas

Siglas: **A** = ana-core, **B** = bruno-terminal, **F** = felipe-qa, **G** = gabi-opensource.

| # | Resp. | Tarefa | Depende de | Pronto quando | Teste que comprova |
|---|---|---|---|---|---|
| T1 | A | Workspace Cargo, crate `termbay-core` vazio e crate do binário `termbay` em `app/src-tauri/` com `--version` e `--help` (parser em `core::cli`), perfil de release de D5 | D1, D5 | `cargo build` e `cargo test` passam nos três sistemas | teste de integração que roda o binário com `--version` e confere a versão do `Cargo.toml` |
| T2 | B | `ui/` com Vite + TypeScript estrito, xterm.js (D4) e um `PtyTransport` falso de eco, para desenvolver no navegador com `npm run dev` (só para desenvolvimento, não é o `serve`). Janela Tauri carregando `ui/dist/`, com CSP restrita e capabilities mínimas | T1 (só a janela; a UI começa em paralelo) | a janela abre nos três sistemas e mostra o xterm.js ecoando o que é digitado | `tsc --noEmit` e Vitest: o transporte falso devolve o que recebe e o terminal escreve o retorno |
| T3 | F | CI no GitHub Actions em `macos-latest`, `ubuntu-22.04` e `windows-latest`: `cargo fmt --check`, `clippy -D warnings`, `cargo test`, `npm ci`, `npm run lint`, `npm test`, `tauri build`. Cache de Cargo e npm | T1 (estende quando T2 chegar) | todo PR roda nos três sistemas e bloqueia o merge se falhar | o próprio pipeline verde; um PR de verificação com um teste quebrado tem que ficar vermelho |
| T4 | F | Medição de tamanho: o binário de release e cada pacote (D9) entram no resumo do job, e a CI falha acima de 20 MB | T3 | o tamanho aparece em todo PR, por sistema | baixar o limite temporariamente num PR de verificação e ver o job falhar |
| T5 | G | `deny.toml` e checagem de licenças do npm (D10) na CI; política e exceções explicadas no `CONTRIBUTING.md` | T1, T2, T3 | `cargo deny check` e a checagem do npm passam, e cada exceção tem motivo escrito | PR de verificação adicionando uma dependência GPL tem que falhar |
| T6 | A | Módulo `core::pty` com `portable-pty`: descobrir o shell padrão (`$SHELL`; no Windows `pwsh`, depois `powershell`, depois `cmd`), abrir como shell de login no macOS (o PATH de quem abre pelo Finder vem vazio), definir `TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM=termbay`; escrever, redimensionar, ler a saída por callback, encerrar sem deixar processo órfão. Gerenciador de sessões por id | T1 | API documentada, sem nenhuma dependência do Tauri | testes de integração nos três sistemas: abre o shell, envia `echo` com um marcador e lê o marcador; redimensiona e confere o tamanho (`stty size` / `$Host.UI.RawUI.WindowSize`); captura o código de saída; após encerrar, o processo filho não existe mais |
| T7 | A | Ponte Tauri (D6): comandos `pty_open/write/resize/close`, `Channel` por pty com bytes crus, agrupamento da saída, evento `pty-exit` | T2, T6 | a UI consegue abrir, usar e fechar um pty pelo Tauri | testes unitários do agrupador (junta, mantém a ordem, envia no tempo limite, nunca perde bytes) e dos comandos com o runtime de teste do Tauri (`tauri::test`) |
| T8 | B | `TauriPtyTransport`: liga o xterm.js aos comandos e ao canal, redimensiona com `addon-fit` (com debounce), copiar e colar, colagem entre colchetes, foco, WebGL com volta para DOM | T7 | **marco 1** (seção 4) | Vitest com transporte falso: digitação vira `write`, redimensionar vira `resize`, saída vira `term.write`, perda de contexto WebGL troca de renderizador |
| T9 | F | Teste ponta a ponta do marco 1 com `tauri-driver` no Linux (xvfb) e no Windows: abre o app, digita `echo` e encontra a saída. No macOS (sem WebDriver para WKWebView), roteiro manual curto em `docs/` executado a cada marco | T8, T3 | e2e na CI em Linux e Windows; roteiro manual do macOS escrito | o próprio e2e; ele precisa falhar se o pty não responder |
| T10 | A + B | Controle de fluxo (D6): a UI confirma os bytes desenhados; o core pausa a leitura acima do limite e retoma abaixo dele | T8 | `yes` ou `cat` de um arquivo grande não trava a janela e Ctrl+C interrompe em menos de 1 s | teste unitário da janela de confirmação no core; e2e/benchmark (T16) com `yes` por 10 s e memória abaixo do limite combinado |
| T11 | B | Mapa de atalhos por sistema (nova aba, fechar, dividir, mudar foco, busca, copiar/colar), sem roubar teclas do shell (Ctrl+C, Ctrl+D etc.). Teclas mortas e IME funcionando (acentos do pt-BR) | T8 | atalhos funcionam nos três sistemas e `ç`, `ã`, `é` chegam ao shell | Vitest da resolução de atalhos por sistema; item obrigatório no roteiro manual (acentos) |
| T12 | B | Abas: abrir, fechar, trocar, reordenar, título vindo do OSC 0/2 do shell; fechar a aba encerra o pty; fechar a última fecha a janela. Layout da janela com o painel lateral vazio e escondido reservado ao escritório | T8, T11 | dá para trabalhar com várias abas, cada uma com seu shell | Vitest do modelo de abas (TypeScript puro, sem DOM); e2e abrindo duas abas com saídas diferentes |
| T13 | B | Divisão de tela: árvore de painéis (horizontal e vertical), arrastar o divisor, foco por atalho e clique, fechar painel junta o espaço. Cada painel tem seu pty | T12 | divisões aninhadas funcionam e cada painel redimensiona seu pty | Vitest da árvore (dividir, fechar recolhe, proporções somam 1, foco vai para o vizinho); e2e com dois painéis |
| T14 | A | Módulo `core::config` (D8): esquema com shell e argumentos, fonte e tamanho, tema, tamanho do scrollback; padrões; erros legíveis; comando Tauri `get_config` | T1 | arquivo ausente ou inválido nunca impede o app de abrir | testes unitários: arquivo ausente, válido, inválido (mensagem com linha) e `TERMBAY_CONFIG` |
| T15 | B | Aplicar a config na UI: temas próprios (um escuro e um claro, cores escritas por nós e registradas como próprias), fonte, scrollback, cursor | T8, T14 | trocar o tema no arquivo e reiniciar muda o visual | Vitest do mapeamento config → opções do xterm.js, incluindo tema desconhecido voltando ao padrão |
| T16 | F | Desempenho: tempo de abertura, memória parada com uma aba, vazão (`cat` de 50 MB) e o teste do `yes`, nos três sistemas. Resultados em `docs/DESEMPENHO.md` como linha de base da fase 2 | T10 | números registrados por sistema | script de benchmark versionado e reproduzível; a CI roda a parte de vazão e avisa em caso de regressão grande |
| T17 | B | Busca no scrollback com `addon-search`: atalho, próxima, anterior, diferenciar maiúsculas | T8, T11 | busca funciona em saídas longas | Vitest do controlador da busca com addon falso |
| T18 | F | Revisão de segurança: capabilities do Tauri só com os nossos comandos e só na janela principal, CSP sem `unsafe-eval` e sem origem remota, nenhum plugin `shell`/`fs`/`http`, nenhuma porta aberta na fase 1, nada de conteúdo do terminal em logs | T7, T12 | checklist assinado no PR | teste que lê `tauri.conf.json` e as capabilities e falha se a CSP ou as permissões forem afrouxadas |
| T19 | G | Docs públicas: README com "como compilar" por sistema (dependências do Linux, WebView2), `CONTRIBUTING.md` com os comandos de lint e teste, arquivo de licenças de terceiros gerado por `cargo-about` + npm e incluído no pacote, `CHANGELOG.md` | T5, T13 | alguém de fora consegue compilar seguindo só o README | job da CI que regenera o arquivo de licenças e falha se ele estiver desatualizado |

## 4. Ordem, paralelismo e marcos

```
Aceite das decisões D1–D10
   │
   ├─ A: T1 ──► T6 ──► T7 ──────────► T10 ──► T14
   ├─ B: T2 (UI no navegador) ──────► T8 ──► T11 ──► T12 ──► T13 ──► T15, T17
   ├─ F:  T3 ──► T4 ─────────────────► T9 ──► T16, T18
   └─ G:        T5 ─────────────────────────────────► T19
                         ▲ marco 1          ▲ marco 2      ▲ marco 3
```

- **Paralelo desde o início:** Ana no core (T1, T6) e Bruno na UI com o transporte falso (T2), sem esperar um pelo outro. Felipe monta a CI assim que T1 existir. Gabi entra quando houver dependências reais para checar.
- **Marco 1, "abre uma janela com um shell funcionando" (T1–T8):** nos três sistemas, a janela abre com o shell padrão; dá para usar `vim`, `top`, `git log` com paginador, `ssh`, cores de 256 e truecolor, Ctrl+C, redimensionar e acentos. Uma aba só, sem divisão. CI verde, tamanho medido.
- **Marco 2, abas e divisão (T9–T13):** controle de fluxo, atalhos, abas e divisão de tela, com e2e. Escopo, contratos e tarefas revisados na seção 6.
- **Marco 3, fim da fase 1 (T14–T19):** configuração, temas, busca, linha de base de desempenho, revisão de segurança, licenças e README. Binário abaixo de 20 MB nos três sistemas.

## 5. Riscos e mitigação

| Risco | Mitigação |
|---|---|
| **WebKitGTK no Linux:** varia entre distribuições; WebGL lento ou ausente; tela branca com alguns drivers (NVIDIA + DMA-BUF) | CI em `ubuntu-22.04` desde T3; volta automática ao renderizador DOM; documentar o contorno `WEBKIT_DISABLE_DMABUF_RENDERER=1` e testar à mão em Fedora e Arch antes do marco 3 |
| **ConPTY no Windows:** exige Windows 10 1809+, redesenha a tela ao redimensionar, diferenças de sequências de escape, descoberta do shell | `portable-pty` (usado pelo WezTerm); informar ao xterm.js a versão do ConPTY (`windowsPty`) para o reflow; testes de T6 rodando no Windows da CI desde o primeiro PR |
| **Desempenho do xterm.js com saída grande** | WebGL, agrupamento de saída, bytes crus no `Channel`, controle de fluxo (T10) e benchmark (T16) antes das abas ficarem prontas |
| **Custo do IPC do Tauri** | `Channel` binário em vez de eventos JSON; se a vazão não bastar, medir antes de buscar alternativa (ex.: protocolo próprio do Tauri) |
| **Licenças MPL-2.0 dentro do Tauri** (D10) | exceção por crate, com aceite do Guilherme e revisão da Gabi; `cargo deny` bloqueia qualquer nova sem revisão |
| **Tamanho acima de 20 MB** | medição em todo PR (T4), perfil de release enxuto, sem plugins Tauri desnecessários, AppImage fora |
| **Atalhos e teclado:** conflito com o shell, IME e teclas mortas no WebKit | mapa por sistema (T11) e acentos como item obrigatório do roteiro manual |
| **Ambiente do shell no macOS:** app aberto pelo Finder sem o PATH do usuário | shell de login em T6, com teste |
| **Processos órfãos ao fechar abas ou o app** | encerramento explícito em T6 e T12, com teste de que o filho não sobrevive |
| **e2e no macOS:** WKWebView não tem WebDriver | testes do core e da UI cobrem a lógica; roteiro manual curto a cada marco |

## 6. Marco 2 revisado: abas e divisão de tela

> Status: proposta, 16/11/2026, depois do marco 1 (T1–T8) e do marco A do autocompletar. Substitui o escopo de T9–T13 da seção 3 e o marco 2 da seção 4; o resto do plano continua valendo. As decisões novas (D18–D26) estão na seção 6.7 e entram em [ARQUITETURA.md](ARQUITETURA.md) como "proposto".

### 6.1 O que o código já tem (e o que muda no plano por causa disso)

| Item | Estado hoje | Consequência para o marco 2 |
|---|---|---|
| `pty_ack` | existe, mas `PtyBridge::ack` ignora o valor | T10 é quase só core: falta contar, pausar e retomar |
| Acks na UI | `connectPty` confirma `chunk.byteLength` no callback do `term.write`; `TauriPtySession.ack` junta os acks de um microtask num só `pty_ack` | a parte do Bruno na T10 já está feita; só precisa continuar valendo com abas escondidas |
| Fila de escrita | uma thread de escrita por pty no bridge; o `pty_write` nunca bloqueia | o Ctrl+C chega ao shell mesmo com a leitura pausada (caminhos independentes) |
| Agrupador | `spawn_batcher`, 8 ms ou 64 KiB, fila de entrada sem limite | o controle de fluxo conta os bytes **antes** do agrupador, para limitar também essa fila |
| Autocompletar | um `AutocompleteController` e um menu por `TerminalView`, presos ao container do painel; no core, um `ShellLink` por pty; histórico compartilhado | vários painéis já funcionam por construção; falta teste e o descarte certo ao fechar |
| `PtyOpenOptions` (TS) | já tem `cwd?` e `env?`, e o transporte os envia, mas o `pty_open` em Rust os **ignora** (o Tauri descarta argumentos desconhecidos) | o contrato do `cwd` precisa ser fechado (6.3); o `env` sai do tipo |
| OSC 7 | **o core não interpreta.** O `OscScanner` só lê o OSC 6973 privado; os OSC 133 e 7 seguem intactos para o xterm.js, e só são emitidos com a integração ligada | o diretório de um painel novo não pode vir do OSC 7 (6.3, D23) |
| `[autocomplete] trigger` | lido pelo core, mas a UI não recebe: não existe `get_config`, e `attachAutocomplete` usa sempre o Tab | `get_config` mínimo entra no marco 2 (D22) |
| Menu do macOS | o app não define menu, então vale o padrão do Tauri, que reserva Cmd+W para "Fechar janela" (e o copiar/colar do Cmd depende do menu Editar) | a T11 troca o menu padrão por um próprio |
| Layout | `#terminals` com um único `TerminalView` e `#office-panel` escondido ao lado | a barra de abas e a árvore de painéis vão dentro de `#terminals`; o escritório continua irmão (6.2) |
| e2e | ainda não existe (a T9 não foi feita) | a T9 abre o marco 2, porque as tarefas seguintes dependem dela para ter e2e |

### 6.2 Escopo e ordem

**Entra no marco 2:**
1. **T9:** e2e com `tauri-driver` (Linux e Windows) e o roteiro manual do macOS.
2. **T10:** controle de fluxo (6.5). Vem antes das abas: com vários painéis, um `yes` sem limite trava todos.
3. **Contratos novos** (6.3): `cwd` no `pty_open`, `pty_info`, limite de ptys, `get_config` mínimo.
4. **T12 + T13:** um modelo só, em TypeScript puro, para abas e árvore de painéis (6.4), depois o DOM.
5. **T11:** atalhos (6.6) e menu próprio do macOS.
6. Autocompletar com vários painéis e `trigger` vindo da config.
7. Pendências de segurança: P6 (`freezePrototype`), P7 (registro do `style-src`, feito nesta revisão) e P8 (e2e do autocompletar).

**Fica para o marco 3:** T14 completa (config), T15 (temas), T16 (linha de base de desempenho; o marco 2 só mede o controle de fluxo), T17 (busca; o atalho já fica reservado), T18 (revisão completa), T19 (docs), e também: reordenar abas arrastando, renomear aba, confirmação ao fechar a **janela** com programas rodando, título da janela, reabrir o shell de um painel que saiu, nome do processo em primeiro plano no título (exigiria consulta periódica).

**Fica para depois da fase 1:** mover painéis entre abas, várias janelas, painel do escritório (fase 2).

**Ordem:**

```
A: A1 (T10) ──► A2 (cwd + limite) ──► A3 (pty_info) ──► A4 (get_config + capabilities)
B: B1 (modelo) ──► B2 (atalhos + menu) ──► B3 (DOM) ──► B4 (transporte) ──► B5 (ciclo de vida) ──► B6 (autocompletar) ──► B7 (P6)
F: F1 (T9) ─────────────► F2 (carga T10, depois de A1) ──► F3 (e2e abas, depois de B5) ──► F4 (segurança) ──► F5 (roteiro macOS)
G: G1 (licenças das deps de e2e) ─────────────────────────────────────────────► G2 (atalhos no README)
```

Ana e Bruno trabalham em paralelo, como na T7/T8: o Bruno usa um `TauriIpc` falso com os contratos da seção 6.3 até os comandos reais chegarem.

### 6.3 Contratos entre core e UI

Tudo é aditivo, exceto a remoção de `env` do `PtyOpenOptions` (que nunca funcionou). Nenhum evento novo: título e diretório exibido vêm do próprio xterm.js, e o resto é consulta sob demanda, sem nada rodando quando o app está parado.

| Item | Tipo | Contrato |
|---|---|---|
| `pty_open` | comando (muda) | entrada `{ cols, rows, onData, cwd? }`. `cwd`: caminho absoluto de uma pasta. Se for relativo, maior que 4096 bytes, inexistente ou não for pasta, o core usa a pasta pessoal, **sem erro** (abrir um terminal nunca falha por causa do diretório). Saída igual: o id. Novo erro: `"too many terminals are open (limit 64)"` quando já há 64 ptys vivos (D18) |
| `pty_info` | comando novo, assíncrono | entrada `{ id }`. Saída `{ shell, cwd, busy, foreground }`, descrita abaixo. Id desconhecido: erro `"no pty session with id N"`, o mesmo texto do `pty_resize`. Calculado só quando chamado |
| `pty_ack` | comando (mesma assinatura, agora com efeito) | `{ id, bytes }`: bytes recebidos pelo canal e já processados pelo xterm.js (callback do `write`). A UI confirma cada byte recebido exatamente uma vez, inclusive em abas escondidas. Id desconhecido continua sendo ignorado; um ack maior que o pendente zera a conta, sem erro |
| `get_config` | comando novo | sem entrada. Saída `{ autocomplete: { enabled, trigger: "tab" \| "ctrl+space", history } }`. A T14 acrescenta campos sem mudar os existentes. Arquivo inválido devolve os padrões, nunca erro |
| `pty-exit`, `pty-shell`, `pty_complete`, `pty_write`, `pty_resize`, `pty_close` | sem mudança | |
| capabilities | Tauri | entram `allow-pty-info`, `allow-get-config` e `core:window:allow-close` (fechar a janela quando a última aba fecha, D19), só na janela `main` |

**`pty_info`, campo a campo:**

```ts
interface PtyInfo {
  /** Nome do programa do shell, sem pasta nem extensão: "zsh", "fish", "bash", "pwsh", "cmd". */
  shell: string;
  /** Diretório atual do processo do shell (não do programa em primeiro plano), ou null quando não dá para saber. */
  cwd: string | null;
  /** true: há um programa rodando no lugar do shell; false: shell ocioso; null: não dá para saber. */
  busy: boolean | null;
  /** Nome do programa em primeiro plano quando busy é true (até 64 caracteres, sem bytes de controle), senão null. */
  foreground: string | null;
}
```

| Campo | macOS | Linux | Windows |
|---|---|---|---|
| `cwd` | `proc_pidinfo(PROC_PIDVNODEPATHINFO)` do pid do shell | `readlink /proc/<pid>/cwd` | `null` no marco 2; no marco B do autocompletar, a integração do PowerShell passa a informar o diretório na marca `prompt` com nonce e o core usa esse valor |
| `busy` / `foreground` | `tcgetpgrp` do mestre do pty (`MasterPty::process_group_leader` do portable-pty) diferente do pid do shell; nome por `proc_name` | idem; nome por `/proc/<pgid>/comm` | processos filhos diretos do shell (`CreateToolhelp32Snapshot`, ignorando `conhost.exe` e `OpenConsole.exe`); sem filhos, `false`; falha na consulta, `null` |
| com integração ativa | `busy` também é `true` quando o `ShellState` está em `Running` | idem | idem |

Caminhos que não são UTF-8 válido viram `null` (o painel novo abre na pasta pessoal). O código por sistema fica em `core::pty` (sem Tauri), com uma função por sistema e alternativa nos três, como pede o princípio multiplataforma.

**Por que o diretório vem do core e não do OSC 7:** o OSC 7 pode ser forjado por qualquer saída (`cat` de um arquivo, um shell remoto). Abrir um painel numa pasta escolhida por um atacante é perigoso: um prompt com `git` dentro de um repositório malicioso pode executar código (`core.fsmonitor`). O diretório do processo, lido do sistema, não pode ser forjado pela saída. Na UI, o OSC 7 continua servindo **só para exibir** o título (6.4), e aí um caminho remoto do `ssh` é até o comportamento desejado.

**Lado da UI (`ui/src/transport/types.ts`):**
- `PtyOpenOptions`: fica `{ cols, rows, cwd? }`; sai `env`.
- `PtySession` ganha `info(): Promise<PtyInfo>`. O transporte de eco devolve `{ shell: "echo", cwd: null, busy: false, foreground: null }`.
- `loadConfig(ipc)` em `ui/src/transport/config.ts` chama `get_config`; fora do Tauri (eco) devolve os padrões.

**Autocompletar com vários painéis:** nenhum contrato muda. Já é um estado por pty no core (`ShellLink` por id, um pedido novo cancela só o anterior do mesmo pty) e um controlador e um menu por painel na UI. Regras que o marco 2 garante e testa:
- só o painel com foco recebe teclas, então só ele abre menu; trocar o foco tira o foco do textarea e o `blur` já fecha o menu;
- fechar um painel descarta controlador, menu e handler de teclas antes do `pty_close`;
- `trigger` vem do `get_config` e vale para todos os painéis abertos depois de carregar;
- o handler global de atalhos (6.6) roda antes do autocompletar, então um atalho do app nunca chega ao menu; o Tab, o Shift+Tab, as setas e o Esc continuam com o menu quando ele está aberto.

### 6.4 Modelo de abas e painéis na UI

**Onde:** `ui/src/layout/model.ts`, TypeScript puro, sem DOM e sem transporte. Cada operação recebe o estado e devolve o novo estado mais uma lista de **efeitos** (`openPty`, `closePty`, `focusPane`, `closeWindow`), que um controlador aplica ao DOM e ao transporte. Assim, todas as regras ficam testáveis no Vitest.

```ts
type PaneId = string;
type Direction = "right" | "down";           // ação: dividir à direita / abaixo

type Node =
  | { kind: "pane"; id: PaneId }
  | { kind: "split"; id: string;
      axis: "row" | "column";                  // row = lado a lado (divisor vertical); column = um sobre o outro
      ratio: number;                           // fração do primeiro filho, entre 0.1 e 0.9
      first: Node; second: Node };

interface Tab   { id: string; root: Node; focused: PaneId }
interface Pane  { id: PaneId; ptyId: string | null; state: "starting" | "running" | "exited";
                  exitCode: number | null; shell: string | null;
                  oscTitle: string | null; cwdLabel: string | null }
interface Layout { tabs: Tab[]; active: string; panes: Record<PaneId, Pane> }
```

**Árvore binária (D26):** dividir troca a folha por um `split` com a folha antiga e uma nova, meio a meio (`ratio: 0.5`). Três Cmd+D seguidos dão 50/25/25, como no iTerm2 e no WezTerm. Uma árvore n-ária, com redistribuição, fica para quando alguém pedir.

**Operações:**

| Operação | Regra |
|---|---|
| `newTab()` | cria a aba **depois da ativa**, com um painel; efeito `openPty` com `cwdFrom` = painel com foco (D23) |
| `split(direction)` | divide o painel com foco; o novo recebe o foco; `openPty` com `cwdFrom` = painel dividido |
| `closePane(id)` | o pai `split` é trocado pelo irmão (o espaço volta para ele); o foco vai para o painel mais próximo na árvore (o irmão ou a folha mais próxima dele); efeito `closePty`. Último painel da aba: a aba fecha e a vizinha da direita (ou da esquerda) fica ativa. Última aba: efeito `closeWindow` (D19) |
| `closeTab(id)` | `closePty` de todos os painéis da aba (com a confirmação de D21 antes, se algum estiver ocupado) |
| `focus(dir)` | `left/right/up/down` pela geometria: calcula os retângulos normalizados da árvore e escolhe o painel que se sobrepõe na direção, o mais próximo; empate fica com o mais acima ou à esquerda. Sem vizinho, nada muda |
| `focusPane(id)` | clique no painel |
| `nextTab()` / `prevTab()` | circular |
| `goToTab(n)` | 1 a 8 pela posição; 9 é sempre a última, como nos navegadores |
| `setRatio(splitId, r)` | arrastar o divisor; limitado entre 0.1 e 0.9; o DOM ainda impõe um mínimo de 80 × 48 px por painel |
| `paneExited(id, code)` | aplica D20 |

**Título** (o da aba é o do painel com foco nela), na ordem:
1. `oscTitle`: OSC 0/2 pelo `term.onTitleChange` do xterm.js (o fish, o bash do Ubuntu e o PowerShell via ConPTY já mandam);
2. `cwdLabel`: o último componente do caminho do OSC 7, lido na UI com `term.parser.registerOscHandler(7)` (não consome a sequência). **Só exibição**, nunca usado para abrir pty;
3. `shell`: o campo `shell` do `pty_info`, consultado uma vez depois do `pty_open`.

O título passa por limpeza (sem C0, C1 nem controles bidirecionais, no máximo 80 caracteres) e entra no DOM só por `textContent`. O nome do programa em primeiro plano não entra no título no marco 2, porque exigiria consulta periódica.

**Quando o shell de um painel sai (D20):** com código 0 (o usuário digitou `exit` ou Ctrl+D), o painel fecha, como `closePane`. Com código diferente de 0, ou morto por sinal, o painel fica, com o aviso que o `connectPty` já escreve mais "Enter fecha o painel", e só o Enter e o Cmd+W/Ctrl+Shift+W fazem algo nele. Isso evita que um shell que falha ao abrir feche a janela antes de o erro ser lido. Um `pty-exit` causado pelo nosso `close()` não passa por aqui (o transporte já o ignora).

**DOM (`ui/src/layout/view.ts`):**
- `#terminals` passa a ter `.tabbar` (botões de aba com `role="tab"` e um × para fechar, e um + para nova aba) e `.tab-panels`, com um elemento por aba; só o da aba ativa fica visível (`hidden`).
- Cada folha vira um `div.pane` com o `TerminalView` daquele painel. Um `TerminalView` **nunca é recriado** quando o layout muda: o elemento dele é movido (`appendChild`) para o novo lugar. O `ResizeObserver` que já existe refaz o `fit` e o `pty_resize` (com o debounce de 50 ms, então arrastar o divisor não inunda o shell de `SIGWINCH`).
- **WebGL só na aba visível:** o navegador limita os contextos WebGL (cerca de 16) e cada um gasta memória de GPU. Ao esconder uma aba, os painéis dela descartam o `WebglAddon`; ao mostrar, voltam a carregá-lo pelo `RendererController`. O texto continua sendo processado enquanto está escondido, então os acks continuam.
- Painel com foco: borda de 1 px na cor do cursor; os outros sem destaque.
- **Escritório (fase 2):** continua fora da árvore, como irmão de `#terminals` em `#app` (`aside#office-panel`), com altura total à direita, como no desenho da [ARQUITETURA.md](ARQUITETURA.md). Ele é um por janela, não por aba, e não tem pty. A fase 2 só acrescenta um divisor arrastável entre `#terminals` e o `aside` e o atalho reservado em 6.6. Escondido, continua `display: none`.

### 6.5 Controle de fluxo (T10)

**Problema:** hoje o leitor lê do pty a cerca de 200 MB/s e tudo cai em filas sem limite (agrupador, canal do Tauri, buffer de escrita do xterm.js), que processa bem menos que isso. Com `yes`, a memória cresce sem parar, e o Ctrl+C só aparece depois de a UI mastigar todo o atraso.

**Algoritmo (por pty):**

```
em_voo = bytes entregues à UI − bytes confirmados pela UI
ALTO = 512 KiB, BAIXO = 128 KiB                 (constantes no core, D25)

thread de leitura, antes de cada read():
    espera enquanto pausado e não drenando        (fora de qualquer lock do callback)
depois de entregar n bytes ao agrupador:
    em_voo += n;  se em_voo >= ALTO: pausado = true
pty_ack(n):
    em_voo = max(0, em_voo − n);  se pausado e em_voo <= BAIXO: pausado = false, acorda o leitor
shell saiu ou close/kill:
    drenando = true (o portão abre de vez); o leitor esvazia o pty até o fim ou o DRAIN_TIMEOUT de 500 ms
```

- **O que é contado:** os bytes que vão para a UI, depois do `OscScanner` (o OSC 6973 removido nunca é confirmado) e antes do agrupador. Assim, a fila do agrupador também fica limitada.
- **Peças no core, sem Tauri:** uma máquina de estados pura `FlowControl` (`sent(n) -> pausar?`, `acked(n) -> retomar?`), sem relógio nem thread, como o `OutputBatcher`; e um portão de leitura no `PtySession`, verificado **antes** do `read()`, nunca dentro do callback com o lock do `Sink` (senão o `finish` travaria ao sair). Sem portão, o custo é zero (testes e o futuro `serve` podem não usar). A ponte junta as duas peças: conta no callback do leitor e libera no `ack`.
- **Agrupador:** os 8 ms / 64 KiB continuam iguais. Como o prazo de 8 ms sempre envia o que está pendente, todo byte contado chega à UI e acaba confirmado, então o `em_voo` sempre volta a zero e não existe trava. O ALTO é múltiplo do bloco (8 blocos de 64 KiB). Pior caso por pty: ALTO + uma leitura (64 KiB) em voo.
- **Ctrl+C:** a escrita tem thread própria e não passa pelo portão. No Unix, o Ctrl+C faz o próprio pty mandar `SIGINT` e descartar a saída na fila do kernel; no Windows, o ConPTY o converte em `CTRL_C_EVENT`. Depois disso, a UI só precisa desenhar o que já está em voo (até 576 KiB por painel). Com 4 painéis em `yes`, são cerca de 2,3 MiB, menos de 200 ms mesmo a 15 MB/s.
- **Por que 512 KiB e não 1 MB (D6):** com 8 painéis em `yes`, o atraso máximo fica em 4,5 MiB, e o Ctrl+C continua abaixo de 1 s numa máquina modesta. O VS Code usa limites ainda menores. A F2 mede a vazão com os dois valores; trocar é mudar uma constante.
- **Saída com a leitura pausada no macOS:** o kernel não deixa um processo terminar de fechar o terminal enquanto há saída pendente, então um shell que sai com a leitura pausada só termina quando a leitura volta. No app isso não trava, porque a UI continua confirmando e o `em_voo` volta a zero; `pty_close` e o fechamento da janela abrem o portão antes de matar o shell. No Linux e no Windows a própria saída abre o portão.
- **Efeito colateral aceito:** com a leitura pausada, uma resposta do `pty_complete` também espera, e o pedido pode dar `timeout`. Só acontece quando um comando está inundando a saída, ou seja, fora do prompt.
- **Risco a medir:** com a janela minimizada, o macOS (App Nap) e o WebView2 podem desacelerar os timers da página. O xterm.js processa mais devagar, os acks atrasam e um build numa aba escondida pode ficar mais lento. A F2 mede isso; se for grave, a solução é aumentar o ALTO enquanto a janela está escondida, não tirar o limite.

**Critério de pronto:** `yes` por 10 s em 1 e em 4 painéis não trava a janela (digitar num quinto painel ecoa com p95 < 100 ms); Ctrl+C em cada painel devolve o prompt em menos de 1 s; a memória do app sobe menos de 100 MB sobre o repouso e volta perto dele depois.

**Números da F2 (Felipe, 17/11/2026).** Duas partes, ambas versionadas:

- *Vazão no core* (`core/tests/flow_load.rs`, `#[ignore]`; a CI roda com 2 s por cenário no Linux e no macOS e põe a tabela no resumo do job). `yes` num `/bin/sh` real, consumidor fazendo o papel da UI. MacBook (macOS 26, build de debug), 3 s por cenário:

  | Cenário | MB/s | Máximo em voo |
  |---|---:|---:|
  | sem portão, consumidor instantâneo | 221,6 | — |
  | 512 KiB, consumidor instantâneo | 228,8 | 11 KiB |
  | 1 MiB, consumidor instantâneo | 225,6 | 11 KiB |
  | 512 KiB, consumidor a 15 MB/s | 11,3 | 512 KiB |
  | 1 MiB, consumidor a 15 MB/s | 11,4 | 1024 KiB |

  Leitura: o portão não custa vazão quando a UI acompanha, e com uma UI lenta o atraso fica no ALTO, não cresce. 512 KiB e 1 MiB dão a mesma vazão, então fica 512 KiB (D6): metade do atraso para o Ctrl+C sem perda medida. O teste falha se o em voo passar do ALTO + 64 KiB, se a vazão com portão cair abaixo de metade da sem portão, ou se com consumidor lento a vazão não acompanhar o consumidor.
- *No app* (`ui/e2e/specs/flow.test.mjs`, job `e2e` no Linux): 1 e 4 painéis em `yes` por 10 s, latência do eco num painel quieto (inclui a ida e volta do WebDriver), tempo do Ctrl+C até a saída do comando seguinte e memória (RSS do app e dos processos do WebKit) em repouso, sob carga e depois. Na CI só falha acima de 3x as metas (eco 300 ms, Ctrl+C 3 s) ou com mais de 100 MB de memória; os números vão para o resumo do job. **Ainda não rodou**: os números entram aqui depois do primeiro job verde.
- *macOS e App Nap*: sem `tauri-driver` para o WKWebView, ficam no [roteiro manual](ROTEIRO-MANUAL.md) (seção "Controle de fluxo").

### 6.6 Atalhos (T11)

**Regras:**
1. **macOS: Cmd. Linux e Windows: Ctrl+Shift**, como o copiar/colar que o Bruno já fez (`clipboard.ts`).
2. Nunca Ctrl+letra sozinho (é do shell), nunca Option/Alt+tecla (é Meta no shell, e no macOS e no ABNT2 produz caracteres) e, no Linux e no Windows, nunca Ctrl+Alt (é o AltGr: no ABNT2, AltGr+Q é `/`).
3. Ignorados durante composição de IME e teclas mortas (`isComposing` ou `keyCode` 229, como no autocompletar), para os acentos chegarem ao shell.
4. Repetição automática (`ev.repeat`) é ignorada nos atalhos que criam ou fecham, para segurar Cmd+T não abrir 30 abas.
5. Letras pelo `ev.key` e, em teclados não latinos, pelo `ev.code`, como o `letterOf` do `clipboard.ts`; números sempre pelo `ev.code` (`Digit1`…), porque o Shift muda o caractere.
6. **Onde:** um único `keydown` na `window`, em fase de captura (`ui/src/layout/shortcuts.ts`), que resolve o atalho por uma função pura `resolveShortcut(ev, os)`. Quando é atalho do app, chama `preventDefault` e `stopPropagation`, e o evento nunca chega ao xterm.js nem ao autocompletar. O resto segue a cadeia de cada painel: autocompletar → copiar/colar → xterm.js.

| Ação | macOS | Linux e Windows |
|---|---|---|
| Nova aba | Cmd+T | Ctrl+Shift+T |
| Fechar painel (a aba fecha com o último) | Cmd+W | Ctrl+Shift+W |
| Dividir à direita (lado a lado) | Cmd+D | Ctrl+Shift+D |
| Dividir abaixo | Cmd+Shift+D | Ctrl+Shift+S |
| Mover o foco entre painéis | Cmd+Option+←/→/↑/↓ | Ctrl+Shift+←/→/↑/↓ |
| Próxima / anterior aba | Cmd+Shift+] / Cmd+Shift+[, e Ctrl+Tab / Ctrl+Shift+Tab | Ctrl+Tab / Ctrl+Shift+Tab |
| Ir para a aba 1–8 / última | Cmd+1…8 / Cmd+9 | Ctrl+Shift+1…8 / Ctrl+Shift+9 |
| Busca no scrollback (T17, marco 3) | Cmd+F (reservado) | Ctrl+Shift+F (reservado) |
| Copiar / colar (já existe) | Cmd+C / Cmd+V pelo menu Editar | Ctrl+Shift+C / Ctrl+Shift+V |
| Recolher/mostrar a coluna de abas (à esquerda, estilo Warp) | Cmd+Shift+B | Ctrl+Shift+B |
| Mostrar/esconder o escritório (fase 2) | Cmd+Shift+O (reservado) | Ctrl+Shift+O (reservado) |

**Escolhas e conflitos conhecidos:**
- **Dividir abaixo no Linux/Windows é Ctrl+Shift+S** porque Ctrl+Shift+E é o seletor de emoji do IBus (GNOME), Ctrl+Shift+U é a entrada Unicode do IBus e Ctrl+Shift+R recarrega a página no WebView2.
- **Ctrl+Shift+←/→ no Windows:** o PSReadLine usa para selecionar palavras. O termbay toma a tecla, como o Windows Terminal faz com Ctrl+Shift+↑/↓. Fica remapeável na T14. No zsh, no bash e no fish, não há uso padrão.
- **Ctrl+Tab** não tem uso no shell (o xterm.js manda um Tab comum) e não interfere com o Tab nem o Shift+Tab do autocompletar.
- **Menu do macOS:** a B2 troca o menu padrão do Tauri por um próprio, com o menu do app (Sobre, Ocultar, Ocultar outros, Mostrar todos, Sair com Cmd+Q), Editar (desfazer, refazer, recortar, copiar, colar, selecionar tudo, de que o Cmd+C/V depende) e Janela (Minimizar com Cmd+M, Zoom, Tela cheia), **sem "Fechar janela"**, para que Cmd+W, Cmd+T e Cmd+D cheguem à página.
- **WebView2:** desligar as teclas do navegador (F5, Ctrl+R, Ctrl+Shift+R, Ctrl+P, Ctrl+F, Ctrl+Shift+I) se o Tauri fixado tiver a opção; senão, o handler global as consome. Ctrl+R precisa continuar chegando ao shell (busca no histórico). Item obrigatório do e2e do Windows.

### 6.7 Decisões que precisam do aval do Guilherme

| # | Decisão | Recomendação | Motivo |
|---|---|---|---|
| D18 | Limite de abas e painéis | sem limite de abas na interface; teto de **64 ptys** no core, com mensagem legível; repetição de tecla ignorada nos atalhos de criar | cada pty custa um processo e quatro threads; o teto só barra acidentes (tecla presa, script) |
| D19 | Fechar a última aba | fecha a janela e encerra o app, **nos três sistemas** (inclusive no macOS) | o app tem uma janela só; ficar aberto sem janela no macOS exigiria menu e reabertura, sem ganho na fase 1 |
| D20 | Shell que sai sozinho | código 0: o painel fecha; código diferente de 0 ou sinal: o painel fica com o aviso, e Enter fecha | sair de propósito não deixa lixo; um shell que falha ao abrir não fecha a janela antes de o erro ser lido |
| D21 | Confirmar antes de fechar | confirmar ao fechar painel ou aba com `busy = true` (o diálogo cita o `foreground`), num diálogo na própria página, sem plugin; `busy = null` não pede. Confirmar ao fechar a janela (Cmd+Q, botão vermelho) fica para o marco 3, com a opção na T14 | protege um `npm run dev` ou um `ssh` de um Cmd+W por engano; fechar a janela exige tratar `CloseRequested` no Rust e fica mais simples junto com a config |
| D22 | T14 no marco 2? | **não.** O marco 2 só ganha o `get_config` com a seção `[autocomplete]` que já existe (corrige o `trigger` ignorado pela UI). A T14 completa continua no marco 3 | a config completa (fonte, tema, scrollback, atalhos) não é necessária para abas e divisão, e mexeria nos mesmos arquivos ao mesmo tempo |
| D23 | Diretório de abas e painéis novos | herdam o diretório do painel com foco, lido do processo do shell pelo core (`pty_info`), **nunca do OSC 7**. No Windows, abrem na pasta pessoal até a integração do PowerShell (marco B) | o OSC 7 pode ser forjado e levar um prompt com `git` a um repositório malicioso; o sistema operacional não pode ser enganado pela saída |
| D24 | Mapa de atalhos | o da seção 6.6, com menu próprio no macOS (sem "Fechar janela") e um único handler global antes do xterm.js | coerente com o copiar/colar atual e sem tomar teclas do shell, exceto o Ctrl+Shift+←/→ do PSReadLine |
| D25 | Limites do controle de fluxo | 512 KiB para pausar e 128 KiB para retomar, por pty, contados depois do scanner e antes do agrupador (a D6 sugeria cerca de 1 MB) | Ctrl+C abaixo de 1 s mesmo com 8 painéis em `yes`, e memória limitada em todas as filas |
| D26 | Estrutura da divisão | árvore binária, divisão meio a meio; o escritório fica fora da árvore | o mais simples que cobre o marco; n-ária só se for pedida |

### 6.8 Tarefas

Siglas como na seção 3. Todas as tarefas com comportamento novo trazem teste (`CLAUDE.md`).

**Ana (core e ponte)**

| # | Tarefa | Depende de | Pronto quando | Teste |
|---|---|---|---|---|
| A1 (T10) | Controle de fluxo da seção 6.5: `FlowControl` puro e portão de leitura no `core::pty`; contagem no callback do leitor do bridge (depois do scanner) e liberação no `PtyBridge::ack`; o portão abre de vez ao sair ou fechar | D25 | `yes` sem acks estabiliza; com acks, volta a fluir; sair ou fechar com a leitura pausada não trava | unitários do `FlowControl` (tabela de `sent`/`acked`, ack maior que o pendente, histerese); portão: pausa bloqueia o `read`, retomada libera, saída com pausa entrega o resto e o `Exit` em até `DRAIN_TIMEOUT` + folga, `close` pausado volta em menos de 3 s; no bridge, com `/bin/sh` (e `cmd` no Windows) rodando um laço infinito: sem acks, os bytes entregues ficam ≤ 576 KiB por 1 s; confirmando tudo, a saída continua; `\x03` com a leitura pausada mais acks devolve o prompt em menos de 1 s |
| A2 | `cwd` no `pty_open` (absoluto, até 4096 bytes, pasta existente; senão a pasta pessoal, sem erro) e teto de 64 ptys (D18) | D23 | o contrato da seção 6.3 vale | `tauri::test` como os de `commands.rs`: `cwd` válido (o `pwd` do shell confere), relativo, inexistente, arquivo, vazio e gigante caem na pasta pessoal; o 65º `pty_open` falha com a mensagem e, depois de fechar um, volta a abrir |
| A3 | `pty_info` com o código por sistema em `core::pty` (seção 6.3); `busy` também a partir do `ShellState` quando há integração; nome limpo e limitado a 64 caracteres | A2 | os quatro campos chegam nos três sistemas, com `null` onde a tabela diz | integração no core: shell ocioso dá `busy: false`; com `sleep 30` dá `true` e `foreground: "sleep"` (`ping -n 30 127.0.0.1` no Windows); depois de `cd` para uma pasta temporária, o `cwd` confere (macOS e Linux) e é `null` no Windows; id desconhecido dá erro; caminho não UTF-8 dá `null` |
| A4 | `get_config` mínimo (D22); `allow-pty-info`, `allow-get-config` e `core:window:allow-close` no `build.rs` e na capability `main` | A3 | a UI lê o `trigger`; nenhum comando novo é chamável fora da janela `main` | unitário: arquivo ausente, válido e inválido; o teste de capability existente estendido aos comandos novos |

**Bruno (UI e Tauri)**

| # | Tarefa | Depende de | Pronto quando | Teste |
|---|---|---|---|---|
| B1 (T12/T13) | `ui/src/layout/model.ts` da seção 6.4: estado, operações e efeitos, sem DOM | D19, D20, D26 | todas as operações da tabela existem com efeitos corretos | Vitest: dividir e fechar (o irmão herda o espaço, `ratio` entre 0.1 e 0.9), foco para o vizinho certo ao fechar, `focus(dir)` em layouts em L e em T (incluindo empates), `goToTab(9)`, fechar a última aba gera `closeWindow`, `paneExited` com 0 e com 1, `cwdFrom` no `openPty` |
| B2 (T11) | `resolveShortcut(ev, os)` puro, handler global em captura (seção 6.6), menu próprio do macOS em `app/src-tauri/src/menu.rs`, teclas do WebView2 | B1, D24 | o mapa funciona nos três sistemas e nenhuma tecla do shell é tomada além das listadas | Vitest da tabela inteira por sistema, mais os negativos: Ctrl+C, Ctrl+D, Ctrl+R, Ctrl+W, Alt+B, AltGr+Q (`ctrlKey` + `altKey`) e Option+letra no macOS chegam ao xterm; composição de IME e `repeat` são ignorados; teste Rust de que o menu do macOS não tem item com Cmd+W, Cmd+T ou Cmd+D |
| B3 (T12/T13) | DOM do layout (seção 6.4): barra de abas, painéis por aba, divisor arrastável com mínimo em pixels, `TerminalView` movido e nunca recriado, WebGL só na aba visível, foco por clique, `#office-panel` intocado | B1 | dá para trabalhar com abas e divisões aninhadas, cada painel com o tamanho certo no pty | Vitest com jsdom: trocar de aba esconde e mostra sem recriar o terminal (mesma instância), o addon WebGL é descartado ao esconder e recarregado ao mostrar (addon falso), o divisor respeita o mínimo; títulos entram por `textContent` |
| B4 | Transporte: `info()`, `cwd` no `open`, `env` removido, `loadConfig`; dublês no transporte de eco e no `TauriIpc` falso | contratos da seção 6.3 (com dublê antes de A2–A4) | a UI fala o contrato novo | Vitest no estilo de `tauri.test.ts`: argumentos do `pty_open` com e sem `cwd`, `pty_info` e `get_config` chamados com os nomes certos, erros viram `Error` |
| B5 | Ciclo de vida: abrir no diretório do painel com foco (D23), títulos (OSC 0/2, OSC 7 só para exibir, nome do shell), saída do shell (D20), confirmação com diálogo na página (D21), fechar a janela na última aba (D19) | B2, B3, B4 | os comportamentos de D19–D21 e D23 funcionam com transporte falso e no app | Vitest: `split` chama `info()` do painel com foco e passa o `cwd`; `cwd: null` abre sem `cwd`; OSC 7 forjado muda o rótulo e nunca o `cwd` do próximo `open`; título com ESC e caracteres bidirecionais é limpo; `busy: true` pede confirmação e cancelar não fecha; `exitCode` 0 fecha, 1 mantém |
| B6 | Autocompletar com vários painéis (seção 6.3) e `trigger` vindo do `get_config` | B5, A4 | cada painel tem o seu menu e o `trigger` da config vale | Vitest: dois painéis com sessões falsas, Tab no painel A só chama `complete()` de A e só mostra o menu de A; trocar o foco fecha o menu; fechar o painel descarta o controlador antes do `close()`; `trigger: "ctrl+space"` deixa o Tab ir para o shell |
| B7 (P6) | Avaliar `freezePrototype: true` | B6 | ligado, se nada quebrar; senão, motivo registrado em `docs/SEGURANCA.md` | rodar o roteiro manual e o e2e com a opção ligada; o teste que lê `tauri.conf.json` passa a exigir `true` se ficar ligado |

**Felipe (testes, CI, desempenho e segurança)**

| # | Tarefa | Depende de | Pronto quando | Teste |
|---|---|---|---|---|
| F1 (T9) | e2e com `tauri-driver` + WebdriverIO no Linux (xvfb, `webkit2gtk-driver`) e no Windows (`msedgedriver` da versão do WebView2), num job da CI. Como o texto do WebGL está num canvas, o build de e2e força o renderizador DOM por uma variável do Vite e lê `.xterm-rows`; um teste confere que o pacote de release não contém essa opção. Roteiro manual do macOS em `docs/ROTEIRO-MANUAL.md` | marco 1 | o e2e do marco 1 (abrir, `echo`, ver a saída) roda na CI; o roteiro está escrito | o próprio e2e, que precisa falhar se o pty não responder (PR de verificação) |
| F2 (T10) | Carga do controle de fluxo com o critério da seção 6.5: 1 e 4 painéis em `yes` por 10 s, latência do eco num quinto painel, Ctrl+C até o prompt, memória antes, durante e depois; ALTO de 512 KiB contra 1 MiB; janela minimizada (App Nap) | A1, F1 | números registrados nesta seção e reaproveitáveis pela T16 | script versionado; a parte de vazão roda na CI e avisa em regressão grande |
| F3 | e2e de abas e divisão: duas abas com saídas diferentes, dividir à direita e abaixo, fechar recolhe, Ctrl+Shift+W com `sleep` rodando pede confirmação, `exit` fecha o painel e `exit 1` não; no Windows, Ctrl+R chega ao shell. Mais o P8: fish e zsh no Linux com `ls fix` + Tab, Enter insere e não executa, Tab no vim vai para o vim, desligado = Tab nativo | B5, B6, F1 | e2e verdes no Linux e no Windows (autocompletar só no Linux) | os próprios e2e |
| F4 | Revisão de segurança do marco 2 em `docs/SEGURANCA.md`: capabilities com os comandos novos e `core:window:allow-close` só na `main`; validação do `cwd`; OSC 7 forjado não muda o diretório de um painel novo; `pty_info` não aparece em logs; títulos sempre por `textContent`; atalhos do WebView2 | A4, B5 | checklist assinado no PR | testes de A2, A4 e B5 citados na tabela de mitigações; o teste de `tauri.conf.json` continua barrando afrouxamento |
| F5 | Rodar o roteiro manual no macOS no fim do marco: abrir pelo Finder (PATH), `vim`, `top`, `less`, `git log`, cores, acentos (`ç`, `ã`, `é`, teclas mortas), copiar e colar, todos os atalhos da seção 6.6, Cmd+W com `vim` aberto, `exit` e `exit 1`, última aba fecha o app, `yes` em 3 painéis com Ctrl+C, minimizar com `yes` rodando, autocompletar ligado em dois painéis | F1, B6, B7 | roteiro executado e resultado anotado no PR do marco | o próprio roteiro |

**Gabi (licenças e docs públicas)**

| # | Tarefa | Depende de | Pronto quando | Teste |
|---|---|---|---|---|
| G1 | Conferir as dependências novas: `webdriverio` e afins (só desenvolvimento, MIT), a feature `Win32_System_Diagnostics_ToolHelp` do `windows-sys` (mesmo crate, MIT/Apache-2.0); nenhuma dependência nova em tempo de execução na UI | F1, A3 | lista permitida da D10 sem exceção nova | job de licenças da T5 verde |
| G2 | README: seção "Abas, painéis e atalhos" com a tabela da seção 6.6 por sistema e os conflitos conhecidos; atualizar o `CONTRIBUTING.md` com como rodar o e2e localmente | B2, F1 | alguém de fora encontra os atalhos e roda o e2e só com a documentação | revisão do PR |

**Heitor:** P7 resolvida nesta revisão (motivo do `dangerousDisableAssetCspModification` registrado na tabela de decisões da [ARQUITETURA.md](ARQUITETURA.md)). P9 (`termbay serve`) continua na fase 2.

**Fim do marco 2:** tudo acima pronto, CI verde nos três sistemas com e2e no Linux e no Windows, roteiro do macOS executado e binário ainda abaixo de 20 MB.

### 6.9 Riscos novos

| Risco | Mitigação |
|---|---|
| Limite de contextos WebGL (cerca de 16) com muitas abas | WebGL só na aba visível (B3); a perda de contexto já cai para o DOM (`RendererController`) |
| Timers desacelerados com a janela minimizada atrasam os acks | medir na F2; se preciso, ALTO maior enquanto escondida, nunca sem limite |
| Menu padrão do macOS rouba Cmd+W | menu próprio (B2), com teste |
| Teclas do navegador no WebView2 (Ctrl+R, F5) | desligar ou consumir (B2); item do e2e do Windows (F3) |
| `busy` não detecta builtins do shell (`read`) sem integração | aceito: no pior caso, fecha sem perguntar, como antes |
| `cwd` indisponível no Windows sem integração | painel novo abre na pasta pessoal até o marco B do autocompletar (D23) |
