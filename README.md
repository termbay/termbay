# termbay

Um terminal leve e open source, no estilo do Warp, onde seus agentes do Claude trabalham num escritório em 2D.

- **Terminal de verdade:** abre o seu shell (fish, zsh, bash, PowerShell...) e roda qualquer comando. Abas e divisão de tela estão a caminho.
- **Autocompletar (experimental):** menu de sugestões com as completions do seu próprio shell e o seu histórico. Vem desligado; veja abaixo.
- **Escritório (planejado):** quando o Claude usa agentes, cada um vira um funcionário com nome e mesa, e balões mostram o que estão fazendo e com quem estão conversando.
- **Time automático (planejado):** `termbay init` lê o escopo do projeto e monta o time de agentes.
- **Sua conta:** usa o Claude Code oficial com o seu próprio login. O app não guarda credenciais.
- **Leve:** binário pequeno, roda em macOS, Linux e Windows.

> Status: em desenvolvimento, ainda sem versão publicada. O terminal já abre o shell real (Tauri + xterm.js); abas e o escritório ainda não existem. Veja [docs/ARQUITETURA.md](docs/ARQUITETURA.md).

> O termbay é um projeto independente, sem relação com a Anthropic nem com a Microsoft. "Claude" e "Claude Code" são marcas da Anthropic.

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
