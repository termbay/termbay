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
- **Marco 2, abas e divisão (T9–T13):** controle de fluxo, atalhos, abas e divisão de tela, com e2e.
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
