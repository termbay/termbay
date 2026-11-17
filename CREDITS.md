# Créditos

Código, textos e modelos de terceiros usados ou adaptados no termbay. Arte, sons e fontes ficam em [assets/CREDITS.md](assets/CREDITS.md).

Cada entrada informa o que foi usado, de onde veio, a licença e onde está no repositório.

| O quê | Origem | Licença | Onde |
|-------|--------|---------|------|
| Código de Conduta | [Contributor Covenant 2.1](https://www.contributor-covenant.org/pt-br/version/2/1/code_of_conduct/) (tradução pt-BR oficial) | CC BY 4.0 | `CODE_OF_CONDUCT.md` |
| Captura do `compadd` no zsh (leitura das opções e coleta de candidatos e descrições), adaptada do `-ftb-compadd` | [fzf-tab](https://github.com/Aloxaf/fzf-tab), Copyright (c) 2019-2026 Aloxaf | MIT (aviso completo no cabeçalho do arquivo) | `core/src/shell_integration/scripts/termbay.zsh` (`_termbay_compadd`) |

Agradecimentos (ideias e protocolos, sem código copiado): [zsh-capture-completion](https://github.com/Valodim/zsh-capture-completion) (técnica original de capturar o `compadd`), a integração de shell do [VS Code](https://github.com/microsoft/vscode) (convenção de escape dos campos do OSC 633 e o truque do `ZDOTDIR` temporário) e os protocolos públicos OSC 7 e OSC 133 (FinalTerm).

<!--
Se modelos de cargos forem adaptados do ECC (https://github.com/affaan-m/ECC, MIT),
adicione uma linha aqui e mantenha o aviso de copyright do ECC no arquivo adaptado.
-->
