# Como contribuir

Que bom que você quer ajudar o officeAgents! Toda contribuição é bem-vinda: código, sprites, temas, tradutores de eventos, documentação, testes e relatos de bug.

Ao participar, você concorda com o nosso [Código de Conduta](CODE_OF_CONDUCT.md).

## Antes de começar

- Leia o [README](README.md) e a [arquitetura](docs/ARQUITETURA.md). Ela explica as decisões e o roteiro.
- O projeto está em **planejamento**: ainda não há código do app. As primeiras contribuições mais úteis são discussões, testes da fase 0 e revisões da arquitetura.
- Para mudanças grandes, abra uma issue antes de começar, pra combinarmos a abordagem.

## Princípios que toda contribuição precisa respeitar

- **Terminal primeiro:** ele tem que ser bom mesmo sem agentes.
- **Conta do usuário:** o app usa o `claude` oficial já logado e **nunca** lê ou guarda credenciais.
- **Leve:** binário pequeno, sem Node nem Python em tempo de execução.
- **Local e privado:** sem servidor externo e sem telemetria por padrão.
- **Multiplataforma:** macOS, Linux e Windows.

## Licenças

O officeAgents é licenciado sob a [MIT](LICENSE). Ao enviar uma contribuição, você concorda que ela seja distribuída sob essa licença.

- **Dependências:** só com licença permissiva compatível com MIT (MIT, Apache-2.0, BSD, ISC, Zlib, CC0). GPL, AGPL e licenças proprietárias não são aceitas.
- **Código de outros projetos:** mantenha o aviso de licença original e adicione uma entrada em [CREDITS.md](CREDITS.md).
- **Arte (sprites, sons, fontes):** precisa ser sua ou CC0, com a origem registrada em [assets/CREDITS.md](assets/CREDITS.md). Não aceitamos assets, personagens ou cenários de jogos existentes.

## Fluxo

1. Faça um fork e crie uma branch a partir de `main` (`feat/abas`, `fix/redimensionar-pty`...).
2. Faça a mudança com testes.
3. Rode lint e testes localmente. Os comandos entram aqui quando o código existir.
4. Abra um pull request preenchendo o modelo.

## Convenções

- **Código, nomes e comentários em inglês**, para receber contribuidores de qualquer lugar. A documentação está em pt-BR por enquanto, e traduções são bem-vindas.
- **Commits** no formato [Conventional Commits](https://www.conventionalcommits.org/pt-br/): `feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`.
- **Toda mudança de comportamento vem com teste.**
- **Nada de segredos ou dados pessoais** em código, fixtures, prints ou gravações de eventos.

## Boas primeiras contribuições

Procure issues com a etiqueta `good first issue`. Alguns exemplos do que vai surgir:

- Novos temas do terminal.
- Sprites e móveis para o escritório (CC0).
- Novos cargos no catálogo do recrutador.
- Tradutores de eventos para outras ferramentas de IA.

## Segurança

Não abra issue pública para vulnerabilidades. Veja [SECURITY.md](SECURITY.md).
