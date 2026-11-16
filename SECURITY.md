# Política de segurança

## Como reportar uma vulnerabilidade

**Não abra uma issue pública.** Use o reporte privado do GitHub: na aba **Security** do repositório, clique em **Report a vulnerability**.

Inclua, se possível:

- O que acontece e qual o impacto.
- Passos para reproduzir.
- Versão do termbay, do Claude Code e do sistema operacional.

Vamos confirmar o recebimento, investigar e combinar com você a divulgação depois da correção. Quem reportar recebe crédito no changelog, se quiser.

## O que é especialmente sensível

- **Credenciais do Claude:** o termbay nunca deve ler, copiar ou guardar tokens, chaves ou dados de login.
- **Servidor local de eventos:** deve escutar só em `127.0.0.1` e exigir o token da sessão.
- **Hooks:** `termbay hook` nunca deve executar conteúdo recebido nem quebrar a sessão do Claude.
- **Recrutador:** os arquivos gerados em `.claude/agents/` não devem incluir instruções vindas de fontes não confiáveis sem revisão do usuário.

## Versões suportadas

O projeto ainda não tem releases. Esta seção será atualizada na primeira versão.
