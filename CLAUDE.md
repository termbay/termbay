# officeAgents

Terminal leve e open source, no estilo do Warp, com um escritório 2D que mostra os agentes do Claude trabalhando. A fonte da verdade é `docs/ARQUITETURA.md`: leia antes de qualquer mudança e atualize quando uma decisão mudar.

## Regras do projeto (valem para todos os agentes)

### Open source desde o primeiro commit
- **Licença:** MIT (proposta em `docs/ARQUITETURA.md`). Todo arquivo novo precisa ser compatível com ela.
- **Dependências:** só com licença permissiva compatível com MIT (MIT, Apache-2.0, BSD, ISC, Zlib, CC0). Nada de GPL/AGPL nem de licenças proprietárias. Na dúvida, pergunte à Gabi (`gabi-opensource`).
- **Arte:** sprites, sons e fontes próprios ou CC0, com a origem registrada em `assets/CREDITS.md`. Nada de assets de jogos existentes.
- **Código reaproveitado:** código ou texto adaptado de outro projeto (por exemplo, o ECC) mantém o aviso de licença e entra em `CREDITS.md`.
- **Segredos:** nunca commitar `.env`, tokens, chaves ou dados pessoais.
- **Escrita pública:** escreva como se qualquer pessoa fosse ler: commits claros, PRs explicando o porquê, nada de referência a contexto privado.

### Princípios técnicos
- **Terminal primeiro:** ele tem que ser bom mesmo sem agentes.
- **Conta do usuário:** o app usa o `claude` oficial já logado e nunca guarda credenciais.
- **Leve:** binário menor que 20 MB, sem Node nem Python em tempo de execução, escritório parado quando escondido.
- **Local e privado:** servidor só em `127.0.0.1`, com token de sessão. Sem telemetria por padrão.
- **Multiplataforma:** macOS, Linux e Windows. Não use nada específico de um sistema sem alternativa para os outros.

### Convenções
- Código, nomes e comentários em inglês, para receber contribuidores de qualquer lugar. Documentação em pt-BR por enquanto.
- Commits no formato Conventional Commits (`feat:`, `fix:`, `docs:`...).
- **Autoria:** commits, PRs, releases e créditos aparecem somente em nome do Guilherme Golfeto (GuiGolfeto). Nunca adicionar `Co-Authored-By` do Claude, "Generated with Claude Code" ou qualquer menção ao Claude como autor ou contribuidor.
- Toda mudança de comportamento vem com teste.

## O time

| Agente              | Nome   | Cargo                                            |
|---------------------|--------|--------------------------------------------------|
| `heitor-arquiteto`  | Heitor | arquitetura, decisões, divisão de tarefas        |
| `ana-core`          | Ana    | core em Rust: pty, servidor de eventos, CLI      |
| `bruno-terminal`    | Bruno  | interface do terminal: xterm.js, abas, Tauri     |
| `carla-escritorio`  | Carla  | escritório 2D: PixiJS, personagens, balões       |
| `davi-integracao`   | Davi   | hooks do Claude Code, tradutor de eventos, recrutador |
| `felipe-qa`         | Felipe | testes, CI, desempenho, segurança                |
| `gabi-opensource`   | Gabi   | licença, contribuição, comunidade, docs públicas |

Fluxo sugerido: o Heitor divide a tarefa, os especialistas implementam, o Felipe revisa e testa, e a Gabi confere licenças e documentação antes de publicar.
