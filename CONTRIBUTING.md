# Como contribuir

Que bom que você quer ajudar o termbay! Toda contribuição é bem-vinda: código, sprites, temas, tradutores de eventos, documentação, testes e relatos de bug.

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

O termbay é licenciado sob a [MIT](LICENSE). Ao enviar uma contribuição, você concorda que ela seja distribuída sob essa licença.

- **Dependências:** só com licença permissiva compatível com MIT. Detalhes logo abaixo.
- **Código de outros projetos:** mantenha o aviso de licença original e adicione uma entrada em [CREDITS.md](CREDITS.md).
- **Arte (sprites, sons, fontes):** precisa ser sua ou CC0, com a origem registrada em [assets/CREDITS.md](assets/CREDITS.md). Não aceitamos assets, personagens ou cenários de jogos existentes.

### Licenças das dependências

A CI confere a licença de toda dependência, direta ou indireta, em Rust e no npm. Se alguma não estiver na lista, o PR fica vermelho.

**Permitidas:** MIT, Apache-2.0 (também com `WITH LLVM-exception`), BSD-2-Clause, BSD-3-Clause, ISC, Zlib, CC0-1.0, Unicode-3.0 e 0BSD. Quando o pacote oferece uma escolha (`MIT OR GPL-3.0`), basta que uma das opções esteja na lista.

**Não aceitas:** GPL, AGPL, LGPL, SSPL, EUPL e outras copyleft fortes, licenças proprietárias, pacotes sem licença e licenças fora do padrão SPDX ("SEE LICENSE IN ..."). Se precisar de algo assim, abra uma issue: quase sempre existe uma alternativa permissiva.

**Exceções (MPL-2.0):** a MPL-2.0 é copyleft fraco, por arquivo. Ela só entra **por exceção, pacote a pacote**, nunca liberada para todos, e só para pacotes que usamos sem modificar. Hoje as exceções são crates trazidos pelo próprio Tauri (`cssparser`, `cssparser-macros`, `selectors`, `dtoa-short` e `option-ext`). Cada uma tem o motivo escrito em [`deny.toml`](deny.toml).

**Para pedir uma exceção:**

1. Abra um PR que adiciona a dependência e a exceção:
   - Rust: uma linha em `exceptions` no [`deny.toml`](deny.toml), com `crate` e `allow`, e um comentário dizendo de onde o crate vem e para que serve.
   - npm: um item em `exceptions` no [`scripts/npm-license-policy.json`](scripts/npm-license-policy.json), com `package`, `license` e `reason`. Use `"devOnly": true` se ele só roda no build: a checagem falha se ele virar dependência de tempo de execução.
2. Explique no PR por que não há alternativa permissiva e confirme que o código não será modificado.
3. Uma pessoa mantenedora revisa antes do merge.

**Para rodar a checagem na sua máquina:**

```sh
# Rust (cargo-deny é só ferramenta de desenvolvimento)
cargo install --locked cargo-deny
cargo deny check

# npm (Node puro, sem dependência extra)
cd ui && npm run licenses

# testes do verificador do npm
node --test "scripts/*.test.mjs"
```

## Ferramentas opcionais

- **[Graphify](https://github.com/Graphify-Labs/graphify)** (Apache-2.0): monta um grafo do código para você ou o seu assistente de IA navegar no repositório sem depender de grep. É só para desenvolvimento: não entra no binário nem nas dependências do projeto, e o grafo gerado (`graphify-out/`) fica fora do git.

  ```sh
  pipx install graphifyy   # ou: uv tool install graphifyy (requer Python 3.11+)
  graphify .               # gera graphify-out/ com graph.html, graph.json e GRAPH_REPORT.md
  ```

  A análise do código é local. Documentos, PDFs e imagens podem passar pelo modelo do seu assistente ou por uma API que você configurar.

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
