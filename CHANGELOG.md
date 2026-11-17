# Changelog

Todas as mudanças relevantes do projeto ficam registradas aqui.

O formato segue o [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/), e o projeto usa [Versionamento Semântico](https://semver.org/lang/pt-BR/).

## [Não lançado]

### Adicionado
- Terminal com o shell real do usuário: app Tauri com xterm.js, CLI `termbay` e core em Rust com pty (macOS, Linux e Windows).
- CI multiplataforma (macOS, Linux e Windows) com testes, limite de tamanho do binário e checagem de licenças das dependências Rust e npm.
- Autocompletar experimental, desligado por padrão (`[autocomplete] enabled = true` em `~/.termbay/config.toml`): menu com as completions do próprio shell e o histórico, para fish e zsh no macOS e no Linux. Veja `docs/AUTOCOMPLETE.md`.
- Arquitetura inicial em `docs/ARQUITETURA.md`.
- Time de agentes do projeto em `.claude/agents/`.
- Base open source: licença MIT, guia de contribuição, código de conduta, política de segurança e modelos de issue e pull request.
