# termbay

Um terminal leve e open source, no estilo do Warp, onde seus agentes do Claude trabalham num escritório em 2D.

- **Terminal de verdade:** abre o seu shell (fish, zsh, bash, PowerShell...) e roda qualquer comando, com abas verticais e divisão de tela.
- **Autocompletar (experimental):** menu de sugestões com as completions do seu próprio shell e o seu histórico. Vem desligado; veja abaixo.
- **Escritório (planejado):** quando o Claude usa agentes, cada um vira um funcionário com nome e mesa, e balões mostram o que estão fazendo e com quem estão conversando.
- **Time automático (planejado):** `termbay init` lê o escopo do projeto e monta o time de agentes.
- **Sua conta:** usa o Claude Code oficial com o seu próprio login. O app não guarda credenciais.
- **Leve:** binário pequeno, roda em macOS, Linux e Windows.

> Status: em desenvolvimento, ainda sem versão publicada. O terminal já abre o shell real (Tauri + xterm.js), com abas verticais e divisão de tela; o escritório ainda não existe. Veja [docs/ARQUITETURA.md](docs/ARQUITETURA.md).

> O termbay é um projeto independente, sem relação com a Anthropic nem com a Microsoft. "Claude" e "Claude Code" são marcas da Anthropic.

## Abas, painéis e atalhos

As abas ficam numa coluna à esquerda, que pode ser recolhida. Cada aba pode ser dividida em painéis, lado a lado ou um abaixo do outro, e cada painel tem o seu próprio shell. Abas e painéis novos abrem na pasta do painel com foco (no Windows, por enquanto, na pasta pessoal).

No macOS os atalhos usam **Cmd**; no Linux e no Windows, **Ctrl+Shift**, para não tomar as teclas do shell (Ctrl+C, Ctrl+R, Alt+letra e AltGr continuam chegando a ele).

| Ação | macOS | Linux e Windows |
|---|---|---|
| Nova aba | Cmd+T | Ctrl+Shift+T |
| Fechar painel (a aba fecha com o último) | Cmd+W | Ctrl+Shift+W |
| Dividir à direita | Cmd+D | Ctrl+Shift+D |
| Dividir abaixo | Cmd+Shift+D | Ctrl+Shift+S |
| Mover o foco entre painéis | Cmd+Option+←/→/↑/↓ | Ctrl+Shift+←/→/↑/↓ |
| Próxima / anterior aba | Cmd+Shift+] / Cmd+Shift+[, ou Ctrl+Tab / Ctrl+Shift+Tab | Ctrl+Tab / Ctrl+Shift+Tab |
| Ir para a aba 1–8 / última | Cmd+1…8 / Cmd+9 | Ctrl+Shift+1…8 / Ctrl+Shift+9 |
| Recolher/mostrar a coluna de abas | Cmd+Shift+B | Ctrl+Shift+B |
| Copiar / colar | Cmd+C / Cmd+V | Ctrl+Shift+C / Ctrl+Shift+V |
| Busca no scrollback (reservado, ainda não existe) | Cmd+F | Ctrl+Shift+F |
| Mostrar/esconder o escritório (reservado) | Cmd+Shift+O | Ctrl+Shift+O |

- Fechar um painel com um programa rodando (por exemplo, `npm run dev` ou `ssh`) pede confirmação.
- `exit` fecha o painel. Se o shell sair com erro, o painel fica aberto com o aviso, e Enter fecha.
- Fechar a última aba encerra o app, nos três sistemas.

**Conflitos conhecidos:**
- **Dividir abaixo no Linux e no Windows é Ctrl+Shift+S**, porque Ctrl+Shift+E e Ctrl+Shift+U são do IBus (emoji e Unicode) e Ctrl+Shift+R recarrega a página no WebView2.
- **Ctrl+Shift+←/→ no Windows** é usado pelo PSReadLine para selecionar palavras; no termbay, move o foco entre painéis. Os atalhos serão configuráveis numa versão futura.
- **No macOS,** o menu do app não tem "Fechar janela", para que Cmd+W, Cmd+T e Cmd+D cheguem ao terminal.

## Autocompletar (experimental)

Ao apertar **Tab** no prompt, o termbay mostra um menu, junto do cursor, com as sugestões que o seu shell daria e linhas do seu histórico. Setas escolhem, Tab ou Enter inserem (o Enter não executa o comando) e Esc fecha.

**Vem desligado.** Para ligar, crie `~/.termbay/config.toml` (ou aponte outro arquivo com a variável `TERMBAY_CONFIG`):

```toml
[autocomplete]
enabled = true
# trigger = "ctrl+space"  # para manter o Tab do próprio shell
# history = false         # para não usar o histórico
```

A mudança vale para as abas abertas depois dela.

**Shells:** fish e zsh, no macOS e no Linux. Nos outros shells e no Windows, o terminal funciona normalmente, só que sem o menu.

**O que o termbay injeta no seu shell**, e só quando o autocompletar está ligado:

- **Scripts próprios** numa pasta do termbay, dentro da pasta de dados do seu usuário (`~/Library/Application Support/termbay/` no macOS, `~/.local/share/termbay/` no Linux). Eles marcam o início do prompt e de cada comando e respondem aos pedidos de sugestão. O código está em [`core/src/shell_integration/scripts/`](core/src/shell_integration/scripts/).
- **Nenhum arquivo seu é editado.** O fish recebe `--init-command`, que roda depois do seu `config.fish`. O zsh começa por um `ZDOTDIR` temporário, que devolve o seu `ZDOTDIR` logo na primeira linha e carrega os seus arquivos de sempre.
- **Variáveis temporárias** (`TERMBAY_NONCE` e, no zsh, `TERMBAY_USER_ZDOTDIR`), que os scripts apagam logo no início: no zsh, antes dos seus arquivos; no fish, logo depois do `config.fish`. Os comandos que você digita não as herdam.
- Os scripts nunca executam a linha de comando e só leem o arquivo de histórico do shell. Nada sai da sua máquina.

**Para desligar,** use `enabled = false` ou apague a seção e abra uma aba nova. A aba volta a abrir o shell exatamente como ele seria aberto sem o termbay. Os detalhes estão em [docs/AUTOCOMPLETE.md](docs/AUTOCOMPLETE.md).

## Contribuindo

Contribuições são bem-vindas! Leia o [guia de contribuição](CONTRIBUTING.md) e o [código de conduta](CODE_OF_CONDUCT.md). Para vulnerabilidades, veja [SECURITY.md](SECURITY.md).

## Licença

[MIT](LICENSE). Créditos de terceiros em [CREDITS.md](CREDITS.md).
