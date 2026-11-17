# Segurança do termbay

> Status: 16/11/2026. Revisão do marco A do autocompletar (AC8), escrita para servir de base à T18.
> Escopo: integração com o shell (`core/src/shell_integration/`), motor de sugestões (`core/src/complete/`), ponte Tauri (`app/src-tauri/src/{bridge,commands,complete}.rs`), menu na UI (`ui/src/autocomplete/`) e a configuração de segurança do Tauri (`app/src-tauri/tauri.conf.json`, `capabilities/main.json`).
> Fora do escopo por enquanto: `termbay serve` (ainda não existe), hooks do Claude Code e escritório.

## 1. Ameaças consideradas

| # | De onde vem | Exemplo |
|---|---|---|
| A1 | Saída não confiável no terminal | `cat` de um arquivo baixado, log com sequências de escape, shell remoto via `ssh`, programa que imprime OSC 133 ou imita o OSC privado `6973` |
| A2 | Um programa lendo o terminal no lugar do shell | vim, less, REPL, `sudo` pedindo senha, um `zsh` aninhado: o gatilho do autocompletar não pode cair neles |
| A3 | Sugestão maliciosa ou com bytes de controle | nome de arquivo com `\r`, ESC, `ESC[201~` ou caracteres bidirecionais; uma resposta forjada |
| A4 | Outro usuário local | plantar código na pasta de scripts que o shell carrega |
| A5 | Conteúdo na webview | dados do terminal ou de uma sugestão virando HTML ou script; poluição de protótipo atingindo o IPC |
| A6 | Efeitos nos arquivos do usuário | edição de `.zshrc`/`config.fish`, histórico gravado pelo termbay |

Um processo do **mesmo usuário** não é considerado adversário: ele já pode ler os arquivos do usuário, abrir o dispositivo do pty e fazer coisas piores. O que ele consegue contra o autocompletar está em "Riscos aceitos".

## 2. Mitigações existentes

| Ameaça | Mitigação | Onde está o teste |
|---|---|---|
| A1: marcas forjadas | O core só confia em marcas do OSC `6973` com o nonce da sessão: 128 bits de `/dev/urandom`, comparação em tempo constante, nunca impresso no `Debug`. Todo OSC `6973`, com nonce certo ou errado, é removido antes da tela. Sequência aberta limitada a 1 MiB. OSC 133 e 7 seguem para o xterm.js, mas não mudam o estado do core | `osc.rs` (unitários); `core/tests/shell_integration.rs`: `zsh_ignores_forged_output_*` e `fish_ignores_forged_output_*` (`cat` de arquivo com 133, 7 e 6973 com nonce errado, vazio, truncado, estendido e em maiúsculas, handshake de outro shell, respostas para os próximos pedidos, sequência de 1,5 MiB e sequências cortadas por CAN) |
| A1: menu aberto por saída | O menu só abre como resposta a um pedido da UI (Tab). Uma resposta forjada não vira marca sem o nonce, e respostas atrasadas são descartadas pelo número do pedido. OSC 133 C/D forjados só **fecham** o menu | mesmos testes; `link.rs` (`timeout_and_late_replies`) |
| A2: gatilho em outro programa | O core só escreve o gatilho com o shell "no prompt" segundo as marcas com nonce, e deixa de considerar o prompt assim que um Enter é enviado. Na UI, o buffer alternativo (vim, less, top) repassa o Tab direto. Com `unavailable`, a UI envia o próprio Tab ao programa | `check_refused_while_running`; `check_full_screen_programs` (less e vim reais: pedido recusado sem escrever nada e o vim recebe exatamente o Tab); `controller.test.ts` ("passes Tab to the program in the alternate buffer") |
| A3: sugestão executando | O core descarta candidatos que precisam de byte de controle e limpa textos e descrições (C0, C1, DEL, controles bidirecionais). A UI limpa de novo, recalcula os DELs pelo tamanho da palavra digitada (uma resposta com 500 DELs apaga no máximo a palavra), insere entre colchetes de colagem quando o shell ativou o modo, e Enter no menu só insere. O menu usa `textContent`, nunca HTML | `complete/mod.rs` (unitários); `controller.test.ts` ("Enter inserts the selection and never sends CR"); `ui/src/autocomplete/security.test.ts` (respostas hostis com e sem colagem entre colchetes) |
| A3: completions executam código | Completions do shell só rodam por ação explícita (Tab ou o atalho), como o Tab nativo (D14) | `controller.test.ts` |
| A4: scripts plantados | Scripts embutidos no binário, gravados em pasta por versão com `0700`. Cada nível da pasta precisa ser uma pasta de verdade (não symlink), do usuário atual e sem escrita para grupo ou outros. Arquivo com conteúdo diferente ou symlink no lugar de um script é apagado e regravado com `0600` | `mod.rs` (`install_writes_private_files_and_repairs_them`); `script_folder_refuses_unsafe_permissions` (0770, 0707, 0777, 0775 e 0722 em níveis diferentes, symlink no lugar da pasta, symlink no lugar do script) |
| A4/A1: nonce herdado | zsh: o shim tira `TERMBAY_NONCE` do ambiente na primeira linha do nosso `.zshenv`, antes de qualquer arquivo do usuário. fish: o script tira a variável no `--init-command` | `zsh_integration`, `fish_integration` (sessão sem `TERMBAY_*`); `zsh_startup_files_never_see_the_nonce` (`.zshenv`, `.zprofile`, `.zshrc` e `.zlogin` do usuário rodam `env` e não veem o nonce) |
| A5: webview | CSP com `script-src 'self'`, `connect-src` só para o IPC, `object-src 'none'`, `base-uri 'none'`, `frame-ancestors 'none'`. `withGlobalTauri: false`. Capability `main` só na janela `main`, com os seis comandos do pty e `listen`/`unlisten`; sem plugins de shell, fs ou http | `capability_allows_pty_complete_only_in_the_main_window` (`complete.rs`) |
| A6: arquivos do usuário | Nenhum dotfile é editado: zsh via `ZDOTDIR` temporário que devolve o original, fish via `--init-command`. O histórico é só lido (no máximo 2 MiB do fim do arquivo), fica em memória e não vai para logs | `zsh_integration` e `fish_integration` (instantâneo do `HOME` temporário antes e depois); `history.rs` |
| Recurso desligado | Com `enabled = false` (padrão do marco A), nada é injetado: argumentos e ambiente iguais aos de antes, nenhum scanner na saída, nenhum handler de tecla | `disabled_changes_nothing` (`complete.rs`); `disabled_bridge_passes_private_sequences_through` (`security_tests.rs`: um OSC 6973 chega intacto à UI); `attach.test.ts` |

## 3. Configuração do Tauri

- **`freezePrototype: false`.** É o padrão do Tauri. Ligado, ele congela `Object.prototype` antes do código da página, para que uma poluição de protótipo não altere as chamadas de IPC. Hoje o risco é baixo: só roda o nosso código (`script-src 'self'`, sem conteúdo remoto, sem `eval`), com poucas dependências (xterm.js e `@tauri-apps/api`), e nenhum dado do terminal vira objeto por `JSON.parse` com mesclagem. **Pendência:** testar com `true` (o xterm.js e os addons não podem depender de atribuir propriedades que já existem no protótipo) e ligar se nada quebrar. Isso precisa de teste no app, não só no Vitest.
- **`dangerousDisableAssetCspModification: ["style-src"]`.** O Tauri acrescenta nonces e hashes à CSP dos assets. Quando um nonce aparece no `style-src`, o navegador passa a ignorar o `'unsafe-inline'`, e os elementos `<style>` que o xterm.js cria em tempo de execução deixariam de valer. Desligar a modificação **só** no `style-src` mantém os scripts protegidos por hash. Risco aceito: injeção de CSS. Ela exigiria um ponto de injeção de HTML, que não existe (o menu usa `textContent`), e o vazamento por CSS esbarra em `img-src`/`font-src`/`connect-src` restritos. **Pendência:** registrar essa decisão na `ARQUITETURA.md`, porque hoje ela não está escrita em lugar nenhum.
- **Capability.** `pty_complete` entra só na janela `main`, junto com os comandos da T7. O `build.rs` declara a lista de comandos, então um comando novo sem permissão não pode ser chamado de nenhuma janela.

## 4. Riscos aceitos

- **Processo do mesmo usuário lê o nonce.** O ambiente inicial continua legível depois do `unset`: `/proc/<pid>/environ` no Linux e `ps eww` no macOS. Conferido no macOS 26: o fish do Homebrew mostra `TERMBAY_NONCE` no `ps eww` durante toda a sessão; o `/bin/zsh` do sistema não mostra o ambiente para o `ps`. Com o nonce, esse processo pode escrever no pty e forjar marcas. O pior caso é o gatilho (caracteres de uso privado) ser digitado num programa ou o menu mostrar sugestões falsas, que continuam sendo só texto, visíveis antes do Enter.
- **Janela do fish.** O `--init-command` roda depois do `config.fish` e do `conf.d`, então programas iniciados por eles herdam `TERMBAY_NONCE` (por exemplo, um `tmux` aberto no `config.fish`). Teste: `fish_known_window_config_fish_children_inherit_the_nonce`. A alternativa (`vendor_conf.d`) também roda depois do `conf.d` do usuário.
- **OSC 133 forjado chega ao xterm.js.** Ele só fecha o menu. Os blocos da fase 5 vão precisar tratá-lo como não confiável.
- **Caracteres do gatilho num candidato ou numa colagem.** Sem colagem entre colchetes, eles disparam o widget, que responde com um número de pedido que ninguém espera. A resposta é descartada.
- **Completions lentas ou com efeitos colaterais** (`kubectl`, `make`) seguram o prompt como no Tab nativo. O core desiste em 1,5 s.

## 5. Pendências

| # | Tipo | O quê | Reprodução / proposta | Sugestão de responsável |
|---|---|---|---|---|
| P1 | bug (baixo) | **Corrida com comando digitado antes da hora.** Uma linha digitada enquanto outro comando roda (`sleep 0.3` + Enter, depois `cat` + Enter) ainda está na fila de entrada quando a marca `prompt` do `sleep` chega. Até a marca `exec` do `cat`, o core acha que está no prompt, e um pedido nessa janela põe o gatilho depois de `cat\r`: o `cat` recebe os caracteres. Efeito: texto lixo no programa seguinte, nunca execução. No zsh a janela cresce com `precmd` lentos do usuário (p10k, starship, vcs_info), porque o nosso gancho roda primeiro | Teste `typed_ahead_command_never_gets_the_trigger` (`#[ignore]`): 20 de 20 no macOS com zsh 5.9 e com fish 4.7, e também no Linux. Proposta para o zsh: emitir `prompt` no `zle-line-init` (via `add-zle-hook-widget`) e só quando `$PENDING` for 0, com o `precmd` emitindo só o código de saída. No fish não há equivalente a `$PENDING`: aceitar o risco ou emitir `prompt` o mais tarde possível | Ana |
| P2 | bug (funcional) | **fish 3.3 não tem `complete --escape`.** No Ubuntu 22.04 (fish 3.3.1) o widget falha e toda resposta vem vazia. A UI devolve o Tab ao shell, então nada quebra, mas o recurso não funciona, embora o código e a documentação digam "fish 3.x ou 4.x" | No Ubuntu 22.04: `fish -c 'complete --escape --do-complete="ls fix"'` termina com `Unknown option '--escape'`. Proposta: sem suporte a `--escape`, usar `complete --do-complete` e `string escape` no candidato, ou declarar a versão mínima e não injetar abaixo dela. A CI do Linux não instala o fish até isso ser resolvido | Ana |
| P3 | lacuna (baixa) | **Permissão dos arquivos de script não é conferida.** Um script com o conteúdo certo, mas com modo `0666`, é mantido. Com as pastas em `0755`, outro usuário poderia alterá-lo entre a instalação e a abertura de uma aba nova. A pasta-base (`Application Support` ou `XDG_DATA_HOME`) também não é conferida | Teste `script_files_writable_by_others_are_repaired` (`#[ignore]`). Proposta: em `write_if_changed`, também regravar quando `mode & 0o022 != 0` ou o dono for outro; e conferir na pasta-base que outros não podem escrever (ou que ela tem sticky bit) | Ana |
| P4 | robustez (baixa) | Se a pasta de scripts sumir com o app aberto, o zsh de uma aba nova abre com `ZDOTDIR` apontando para uma pasta vazia e **não carrega o `.zshrc` do usuário** | Proposta: em `inject`, conferir que os arquivos existem (ou reinstalar) antes de mudar o `ZDOTDIR`, e não injetar se falhar | Ana |
| P5 | desempenho | O scanner para em todo ESC. Em saída colorida, isso custa cerca de 10% do tempo de CPU do caminho do pty no macOS, sem perda de vazão medida porque o pty é o gargalo. Numa máquina com pty mais rápido, a folga diminui | Ver números em [AUTOCOMPLETE.md](AUTOCOMPLETE.md), seção 10. Proposta: no estado normal, procurar o prefixo inteiro `ESC ] 6973 ;` com `memchr::memmem` (cerca de 10x mais rápido nessa saída) e tratar à parte só um prefixo partido no fim da leitura | Ana |
| P6 | config | Avaliar `freezePrototype: true` | ver seção 3 | Bruno |
| P7 | doc | Registrar na `ARQUITETURA.md` o motivo do `dangerousDisableAssetCspModification` | ver seção 3 | Heitor |
| P8 | teste | e2e com tauri-driver (Linux) do AC8: `ls fix` + Tab, Enter insere e não executa, Tab no vim vai para o vim, desligado = Tab nativo. Hoje isso está coberto por partes (core com shells reais, UI com Vitest), sem um teste do app inteiro | | Felipe |
| P9 | futuro | `termbay serve`: só em `127.0.0.1`, com token de sessão; o WebSocket precisa do mesmo contrato do `pty_complete` sem abrir o gatilho a outra origem | | Ana, Felipe |

### P1 a P5: resolvidas (Ana, 16/11/2026)

- **P1 (comando digitado antes da hora):**
  - zsh: a marca `prompt` saiu do `precmd` e vai para o `zle-line-init` (via `add-zle-hook-widget`), só quando `$PENDING` é 0. Com letras pendentes sem Enter, ela sai no primeiro `zle-line-pre-redraw` sem nada pendente que não venha de um `accept-line`. Se um plugin trocar o despachante de hooks do zsh, o `precmd` volta a mandar a marca, como antes.
  - fish: não há como ver a entrada pendente. O core cobre esse caso, porque toda tecla passa pela ponte. Um Enter enviado fora do prompt marca digitação antecipada, e então o pedido só sai depois de o prompt ficar 250 ms sem marca `exec` (`TYPEAHEAD_SETTLE`). O comando antecipado começa antes disso.
  - A regra antiga continua valendo nos dois shells: depois de um Enter, só vale um `prompt` que chegue depois dele.
  - Risco restante, só no fish: um `fish_prompt` que leve mais de 250 ms para desenhar.
  - Testes: `typed_ahead_command_never_gets_the_trigger` (zsh, falha com o script anterior), `fish_typed_ahead_command_never_gets_the_trigger`, `zsh_letters_typed_ahead_still_complete` e `link.rs` (`after_typeahead_the_prompt_must_settle`).
- **P2 (fish 3.3):** a versão mínima suportada passa a ser o fish 3.3; abaixo dela o script não manda o handshake, e o menu não aparece. No fish 3.3, sem `complete --escape`, o script escapa os candidatos com `string escape` (mantendo `~/`). Limitação: um nome com quebra de linha chega partido, como o fish 3.3 o imprime. A CI do Linux instala o fish 3.3.1 do Ubuntu 22.04 e o exige. Teste local do caminho: `fish_without_complete_escape`. Toda a suíte também passou num Ubuntu 22.04 com o fish 3.3.1.
- **P3 (permissões):**
  - um script é regravado se tiver outro dono ou puder ser escrito pelo grupo ou por outros;
  - a pasta-base precisa ser do usuário (ou do root);
  - escrita de outros só é aceita com sticky bit;
  - escrita do grupo só quando o grupo é o principal do próprio usuário (o umask 002 do Ubuntu deixa `~/.local/share` em 0775).
  - Testes: `script_files_writable_by_others_are_repaired`, `a_shared_data_folder_is_refused`.
- **P4 (pasta apagada):** o `inject` confere e regrava os scripts antes de cada spawn. Se não conseguir, abre o shell sem integração. Testes: `deleted_scripts_are_written_again_before_a_spawn`, `unrepairable_scripts_leave_the_shell_untouched`.
- **P5 (desempenho):** fora de uma sequência, o scanner faz um `memchr` de ESC e, a partir do primeiro ESC, um `memmem` do prefixo inteiro. Em 50 MB, no macOS (`scanner_cpu_cost`):

  | Saída | Antes | Depois |
  |---|---|---|
  | Log colorido | 25,2 ms (9,5% do tempo do pty) | 4,1 ms (1,6%) |
  | Texto simples | 0,9 ms | 0,9 ms (sem mudança) |

