# Gestor de Crédito

Sistema desktop de **fiado**: cadastre clientes, registre dívidas e pagamentos e cobre pelo WhatsApp ou Telegram com um link de pagamento do Mercado Pago. Quando o cliente paga pelo link, o sistema detecta e registra o pagamento sozinho.

Escrito em Rust, com interface em [egui](https://github.com/emilk/egui) e banco SQLite local. Licença [MIT](LICENSE).

## Status

Em desenvolvimento. O que existe hoje:

- Clientes, dívidas e pagamentos, com saldo por cliente e histórico.
- Cobrança por link do Mercado Pago (Checkout Pro), com detecção automática do pagamento.
- Envio da cobrança por WhatsApp e Telegram (pela sua conta), manual ou automático.
- Senha de acesso, com os tokens guardados criptografados.
- Backup local automático, restauração, e envio criptografado ao Google Drive.

O que **ainda não existe**:

- Criptografia do arquivo do banco em si (só os tokens e os backups enviados à nuvem são criptografados).

As integrações com o Mercado Pago, o WhatsApp, o Telegram e o Google Drive compilam e têm a lógica de apoio testada, mas **ainda não foram exercitadas contra os serviços reais**. Trate-as como experimentais.

## Telas

| Seção | O que faz |
|---|---|
| **Transações** | Lista todas as dívidas e pagamentos e permite registrar uma nova transação. |
| **Clientes** | Busca, cadastro, saldo (deve, crédito ou em dia) e histórico. |
| **Cobrança automática** | Gera links de pagamento, acompanha o status e envia aos clientes. Configura o provedor (Mercado Pago). |
| **Canais** | Conecta o WhatsApp (QR code) e o Telegram (sua conta) e define o envio automático. |
| **Backup e segurança** | Senha do sistema, backups locais, restauração e Google Drive. |

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

O envio é feito **pela sua conta pessoal** do Telegram (API de usuário, protocolo MTProto), e não por bot: bots não conseguem iniciar conversa com quem nunca falou com eles. Assim dá para escrever a qualquer número de cliente que tenha Telegram.

1. Em <https://my.telegram.org>, entre com o seu celular, abra *API development tools* e crie um aplicativo. Anote o `api_id` e o `api_hash`.
2. Em **Canais**, clique em **Conectar conta**, informe o `api_id`, o `api_hash` e o seu celular. O Telegram envia um código ao seu aplicativo; digite-o (e a senha da verificação em duas etapas, se a conta tiver).
3. A sessão fica salva e o sistema reconecta nas próximas aberturas.

> **Atenção:** mensagens em massa ou não solicitadas podem fazer o Telegram limitar ou banir a conta. Cada número enviado é salvo como contato na sua conta. Prefira uma conta dedicada à cobrança.

### WhatsApp

Em **Canais**, clique em **Conectar** e leia o QR code em *WhatsApp > Aparelhos conectados > Conectar um aparelho*. A sessão fica salva e o sistema reconecta nas próximas aberturas.

> **Atenção:** o WhatsApp usa a biblioteca não oficial [`whatsapp-rust`](https://github.com/jlucaso1/whatsapp-rust) (protocolo do WhatsApp Web). Isso pode violar os Termos de Uso do WhatsApp e levar ao **bloqueio do número**. Prefira um número dedicado e evite envios em massa. O envio automático vem desligado por padrão.

## Segurança e backup

### Senha do sistema

Na primeira abertura o sistema pede uma senha (ou deixa seguir sem ela). Ela é pedida toda vez que o sistema abre e dela sai a chave (Argon2id) que criptografa, com ChaCha20-Poly1305, os tokens (Mercado Pago, `api_hash` do Telegram e do Google Drive) e os backups enviados à nuvem.

> **Não há como recuperar a senha.** Se for esquecida, os tokens precisam ser cadastrados de novo e os backups criptografados não podem mais ser abertos.

Limites: o arquivo do banco (`fiado.db`) e as sessões do WhatsApp e do Telegram **não** são criptografados. Quem tiver acesso à sua conta do Windows consegue lê-los.

### Backup local

Por padrão o sistema faz um backup a cada 24 horas (e na abertura, se já passou o prazo), guardando os 14 mais recentes em `%APPDATA%\GestorDeCredito\backups\`. Os backups **não incluem as configurações** (tokens e senha). Em **Backup e segurança** dá para fazer um backup na hora e restaurar a partir da lista ou de um arquivo. Antes de restaurar, o estado atual é guardado como `...-antes-de-restaurar.db`, para dar para desfazer. Restaurar troca clientes, dívidas e pagamentos, mas mantém a senha e as configurações atuais.

### Google Drive

Os backups são enviados **criptografados** a uma pasta do seu Drive. O sistema usa o escopo `drive.file`: só enxerga os arquivos que ele mesmo criou. É preciso ter a senha do sistema criada.

1. No [Google Cloud Console](https://console.cloud.google.com/), crie um projeto e ative a **Google Drive API**.
2. Configure a tela de consentimento OAuth e crie uma credencial **ID do cliente OAuth** do tipo **Aplicativo para computador**.
3. Em **Backup e segurança**, clique em **Configurar e conectar**, cole o ID e a chave secreta do cliente e autorize no navegador que abrir.

> **Atenção:** se a tela de consentimento ficar em modo *Em teste*, o Google invalida o token de acesso depois de **7 dias**, e será preciso reconectar. Para evitar isso, publique o aplicativo (*Em produção*); para uso pessoal não é necessário passar pela verificação do Google.

Para restaurar num computador novo, conecte o Drive, baixe o backup pela lista e informe a senha que estava em uso quando ele foi feito.

## Compilando

Requisitos: [Rust](https://rustup.rs) 1.94 ou superior (edição 2024) e, no Windows, as ferramentas de build do Visual Studio (MSVC).

O alvo é o **Windows 10**. Para um computador de 32 bits, instale o alvo e compile com ele: `rustup target add i686-pc-windows-msvc` e `cargo build --release --target i686-pc-windows-msvc` (o executável sai em `target\i686-pc-windows-msvcelease\`). O Windows 7/XP não é suportado pelo Rust atual.

```bash
cargo run                 # executa em modo debug
cargo build --release     # binário otimizado (a compilação é demorada)
cargo test                # testes
```

A primeira compilação é lenta por causa do `whatsapp-rust`, do `grammers` e do SQLite embutido. O perfil de debug está configurado para gerar pouca informação de depuração, para não encher o disco.

Notas sobre dependências:

- `rusqlite` está fixado na versão 0.39 porque o `whatsapp-rust` exige uma versão compatível do `libsqlite3-sys`.
- O `whatsapp-rust` é usado sem a feature `simd` (padrão), que exige Rust nightly.
- O `grammers-session` é usado sem o armazenamento SQLite (que traria outra cópia do SQLite); a sessão do Telegram é gravada em JSON.

## Dados do usuário

Ficam em `%APPDATA%\GestorDeCredito\` (a pasta antiga `CaderninhoFiado` é movida automaticamente):

- `fiado.db`: clientes, transações, cobranças e configurações (os tokens ficam criptografados quando há senha).
- `whatsapp.db` e `telegram.session`: sessões dos canais. Apagá-las (botões **Desvincular aparelho** e **Sair da conta**) exige entrar de novo.
- `backups\`: backups locais.

## Contribuindo

Veja o [`CLAUDE.md`](CLAUDE.md) para a arquitetura e as convenções do código (padrões Strategy e Simple Factory, diálogos, ícones, migrações do banco).

## Créditos

O visual (paleta, raios e geometria dos componentes) é inspirado no tema "Pro Medium" do [PhotoCraft/ArtCraft](https://github.com/storytold/photocraft), licenciado como MIT OR Apache-2.0. Só valores de design foram usados, sem copiar código. Os ícones são do [Phosphor](https://phosphoricons.com/) (MIT).

## Licença

[MIT](LICENSE) © Marcuth
