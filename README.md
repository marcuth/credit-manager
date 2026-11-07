# Gestor de Crédito

Sistema desktop de **fiado**: cadastre clientes, registre dívidas e pagamentos e cobre pelo WhatsApp ou Telegram com um link de pagamento do Mercado Pago. Quando o cliente paga pelo link, o sistema detecta e registra o pagamento sozinho.

Escrito em Rust, com interface em [egui](https://github.com/emilk/egui) e banco SQLite local. Licença [MIT](LICENSE).

## Status

Em desenvolvimento. O que existe hoje:

- Clientes, dívidas e pagamentos, com saldo por cliente e histórico.
- Cobrança por link do Mercado Pago (Checkout Pro), com detecção automática do pagamento.
- Envio da cobrança por WhatsApp e Telegram, manual ou automático.

O que **ainda não existe**:

- Backup do banco (local e no Google Drive) e restauração.
- Criptografia do banco e dos tokens. Hoje eles ficam em texto puro no SQLite local.
- Compatibilidade com Windows antigo: o Rust atual não compila para Windows 7/XP.

As integrações com o Mercado Pago e com o WhatsApp compilam e têm a lógica de apoio testada, mas **ainda não foram exercitadas contra os serviços reais**. Trate-as como experimentais.

## Telas

| Seção | O que faz |
|---|---|
| **Transações** | Lista todas as dívidas e pagamentos e permite registrar uma nova transação. |
| **Clientes** | Busca, cadastro, saldo (deve, crédito ou em dia), histórico e vínculo do Telegram. |
| **Cobrança automática** | Gera links de pagamento, acompanha o status e envia aos clientes. Configura o provedor (Mercado Pago). |
| **Canais** | Conecta o WhatsApp (QR code) e o Telegram (token do bot) e define o envio automático. |

Cadastros e edições abrem em diálogos. A janela abre maximizada.

## Como funciona a cobrança

1. Em **Cobrança automática**, você gera uma cobrança para um cliente. O sistema cria o checkout no Mercado Pago e guarda o link.
2. O link é enviado ao cliente pelo WhatsApp ou Telegram, manualmente ou, se ativado em **Canais**, assim que a cobrança é gerada.
3. A cada 30 segundos (ou no botão **Verificar pagamentos agora**) o sistema consulta o Mercado Pago por pagamentos aprovados dessa cobrança. Quando encontra, marca a cobrança como paga e lança o pagamento no saldo do cliente. Consultar de novo não duplica o lançamento.

A detecção é por consulta periódica e **não usa webhook**: um PC doméstico atrás de roteador não recebe conexões de fora. A consequência é que o sistema precisa estar aberto para perceber o pagamento. Se estiver fechado, ele percebe na próxima vez que abrir.

## Configuração

### Mercado Pago

Em **Cobrança automática**, clique em **Provedor**, escolha *Mercado Pago* e informe o **Access Token** da sua aplicação (no painel de desenvolvedores do Mercado Pago). Há também o provedor *Simulado*, que aprova qualquer cobrança e serve apenas para testar o fluxo.

### Telegram

1. No Telegram, converse com o `@BotFather` e crie um bot com `/newbot`.
2. Em **Canais**, clique em **Configurar bot** e cole o token.
3. Em **Clientes**, selecione o cliente e use **Copiar link do Telegram**. Envie o link ao cliente: quando ele abri-lo e tocar em *Iniciar*, o Telegram dele fica vinculado.

Bots do Telegram não conseguem iniciar conversa pelo número de telefone, por isso o vínculo é feito por esse link. O código do link é aleatório e individual por cliente.

### WhatsApp

Em **Canais**, clique em **Conectar** e leia o QR code em *WhatsApp > Aparelhos conectados > Conectar um aparelho*. A sessão fica salva e o sistema reconecta nas próximas aberturas.

> **Atenção:** o WhatsApp usa a biblioteca não oficial [`whatsapp-rust`](https://github.com/jlucaso1/whatsapp-rust) (protocolo do WhatsApp Web). Isso pode violar os Termos de Uso do WhatsApp e levar ao **bloqueio do número**. Prefira um número dedicado e evite envios em massa. O envio automático vem desligado por padrão.

## Compilando

Requisitos: [Rust](https://rustup.rs) 1.94 ou superior (edição 2024) e, no Windows, as ferramentas de build do Visual Studio (MSVC).

```bash
cargo run                 # executa em modo debug
cargo build --release     # binário otimizado (a compilação é demorada)
cargo test                # testes
```

A primeira compilação é lenta por causa do `whatsapp-rust` e do SQLite embutido. Os binários de build ocupam vários GB em `target/`.

Notas sobre dependências:

- `rusqlite` está fixado na versão 0.39 porque o `whatsapp-rust` exige uma versão compatível do `libsqlite3-sys`.
- O `whatsapp-rust` é usado sem a feature `simd` (padrão), que exige Rust nightly.

## Dados do usuário

Ficam em `%APPDATA%\CaderninhoFiado\`:

- `fiado.db`: clientes, transações, cobranças e configurações (inclui os tokens, em texto puro por enquanto).
- `whatsapp.db`: sessão do WhatsApp. Apagá-la (botão **Desvincular aparelho**) exige ler um novo QR code.

Enquanto o backup não existe, copie essa pasta periodicamente.

## Contribuindo

Veja o [`CLAUDE.md`](CLAUDE.md) para a arquitetura e as convenções do código (padrões Strategy e Simple Factory, diálogos, ícones, migrações do banco).

## Licença

[MIT](LICENSE) © Marcuth
