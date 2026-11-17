# Roteiro manual do macOS

> O `tauri-driver` não funciona com o WKWebView, então o macOS não tem e2e na CI (Linux e Windows têm, veja o job `e2e` em `.github/workflows/ci.yml`). Este roteiro substitui o e2e no macOS e é executado no fim de cada marco, com o app de release (`tauri build`), aberto **pelo Finder**. Anote o resultado no PR do marco: data, versão do macOS, máquina, `[x]` ou `[ ]` em cada item e o que deu errado.
>
> Atalhos do macOS: seção 6.6 de [FASE1-PLANO.md](FASE1-PLANO.md).

## Preparação

- [ ] `tauri build` sem erro; tamanho do `.app` e do `.dmg` abaixo de 20 MB (o job `bundle` também confere).
- [ ] Monitor de Atividade aberto, filtrando por `termbay`, para anotar a memória.
- [ ] Para o autocompletar: `~/.termbay/config.toml` com `[autocomplete]` e `enabled = true` (ou `TERMBAY_CONFIG` apontando para outro arquivo; um app aberto pelo Finder não herda variáveis do terminal).

## Terminal

- [ ] Abrir pelo Finder: `echo $PATH` mostra o `PATH` do shell de login (Homebrew incluído), não só `/usr/bin:/bin`.
- [ ] `vim`: abre em tela cheia, setas e `:q` funcionam, a tela volta ao normal ao sair.
- [ ] `top`: atualiza, `q` sai.
- [ ] `less` num arquivo grande: rolagem, `/busca`, `q`.
- [ ] `git log` com paginador: cores e `q`.
- [ ] Cores: `ls -G` e um script de 256 cores ou truecolor aparecem certos.
- [ ] Acentos: digitar `ç`, `ã`, `é`, `ü` com teclas mortas (`´` + `e`, `~` + `a`) e colar `ação`; tudo aparece certo no prompt e no `echo`.
- [ ] Copiar (Cmd+C com seleção) e colar (Cmd+V); Cmd+C sem seleção não manda nada ao shell.
- [ ] Cmd+F abre a busca.

## Abas, painéis e atalhos

- [ ] Cmd+T abre aba nova no diretório do painel com foco (`cd /tmp` antes, `pwd` na aba nova).
- [ ] Cmd+D divide à direita e Cmd+Shift+D abaixo; o painel novo abre no mesmo diretório.
- [ ] Cmd+Option+setas mudam o foco entre painéis; clique também muda.
- [ ] Arrastar o divisor redimensiona; `tput cols` muda de acordo; não dá para encolher abaixo do mínimo.
- [ ] Cmd+1…Cmd+9, Cmd+Shift+] e Cmd+Shift+[ trocam de aba; Ctrl+Tab e Ctrl+Shift+Tab também.
- [ ] Cmd+Shift+B esconde e mostra a coluna de abas; Cmd+Shift+O alterna o escritório.
- [ ] Option+letra (por exemplo Option+B no zsh) chega ao shell; Ctrl+C, Ctrl+D, Ctrl+R e Ctrl+W chegam ao shell.
- [ ] Cmd+W com `vim` aberto pede confirmação; Cancelar mantém o painel; Enter logo após a pergunta não fecha (o foco está em Cancelar).
- [ ] Cmd+W com o shell ocioso fecha sem perguntar.
- [ ] `exit` fecha o painel; `exit 1` mantém o painel com a mensagem de saída.
- [ ] Fechar a última aba fecha o app.

## Controle de fluxo

- [ ] `yes` em 3 painéis ao mesmo tempo: a janela continua respondendo; digitar num quarto painel ecoa sem atraso perceptível.
- [ ] Ctrl+C em cada painel devolve o prompt em menos de 1 s.
- [ ] Memória do app (Monitor de Atividade) em repouso, durante o `yes` e 10 s depois: sobe menos de 100 MB e volta perto do repouso.
- [ ] Minimizar a janela com `yes` rodando por 30 s; ao voltar, a janela responde e Ctrl+C devolve o prompt (App Nap).

## Autocompletar

- [ ] Ligado, em dois painéis: `ls ` + Tab abre o menu só no painel com foco; trocar o foco fecha o menu.
- [ ] Enter no menu insere o candidato e não executa.
- [ ] Tab dentro do `vim` vai para o `vim`.
- [ ] Desligado (`enabled = false`, app reaberto): Tab é o do shell.

## Resultado

| Data | macOS | Máquina | Quem | Itens com falha |
|---|---|---|---|---|
| | | | | |
