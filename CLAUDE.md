# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Projeto

Sistema desktop de fiado (clientes, dívidas e pagamentos) em Rust, MIT, alvo Windows. Interface em egui/eframe, banco SQLite local, cobrança automática por link do Mercado Pago enviado por WhatsApp ou Telegram. Textos da interface e comentários estão em português do Brasil.

## Comandos

```
cargo build                      # debug
cargo build --release            # LTO + opt-level "s" + panic=abort: compila devagar
cargo test                       # todos os testes (unitários, ficam nos próprios módulos)
cargo test paid_charge           # um teste, por trecho do nome
cargo test billing::channel      # um módulo
```

Não há lint/CI configurados. No Windows o `.exe` fica travado enquanto o app está aberto, então o `cargo build` falha com "Acesso negado" se uma instância ainda estiver rodando. Feche o processo antes de recompilar.

A primeira compilação é demorada por causa do `whatsapp-rust` e do SQLite embutido. Um `timeout` curto mata o `rustc` e parece erro de compilação (código 143).

## Restrições de dependências (não "atualize" sem ler)

- `rusqlite` está fixado em **0.39**: o `whatsapp-rust-sqlite-storage` exige `libsqlite3-sys ^0.37`, e só uma versão de crate pode ter `links = "sqlite3"`. Subir o rusqlite quebra a resolução.
- `whatsapp-rust` é usado com `default-features = false`: a feature `simd` (padrão) exige Rust nightly. Features ativas: `sqlite-storage, tokio-transport, tokio-runtime, tokio-native, ureq-client`.
- `eframe` 0.37 tem API nova: `eframe::App` implementa `ui(&mut Ui, frame)` e `logic(ctx, frame)` (não existe `update`), e os painéis são `egui::Panel::left/bottom(..).show(ui, ..)` (não `SidePanel`).

## Arquitetura

O fluxo atravessa vários arquivos; os pontos que não são óbvios lendo um só:

**Threads e eventos.** Toda a UI e o `Db` (`rusqlite::Connection`, não é `Sync`) vivem na thread da interface. Trabalho de rede roda em threads descartáveis (`std::thread::spawn`) que devolvem o resultado por um `mpsc` como `services::Event`. `App::handle_events` (em `app.rs`) é o único lugar que aplica esses resultados ao banco. Nunca chame `PaymentProvider` ou `MessageChannel::send` direto da thread da UI. Se adicionar um novo trabalho assíncrono, crie uma variante de `Event` e trate-a em `handle_events`. `App::logic` pede repaint a cada 1 s e dispara a consulta de pagamentos a cada 30 s.

**Services e revision.** `Services` (`services.rs`) é o estado compartilhado passado a cada tela: `db`, provedor de pagamento, `channels`, canal de eventos e mensagem de status. Qualquer mudança de dados deve chamar `services.changed()`, que incrementa `revision`; as telas guardam em cache suas listas e recarregam quando `revision` muda.

**Padrões (pedidos explicitamente pelo dono do projeto, mantenha-os):**
- *Strategy:* `billing::PaymentProvider` (Mercado Pago, `mock`), `billing::channel::MessageChannel` (WhatsApp, Telegram) e `ui::View` (uma por seção do sidebar).
- *Simple Factory:* `billing::factory::create_provider`, `Channels::get(kind)` e `ui::create_view(Section)`.
- *Formulários em diálogos:* criação/edição nunca é inline; use `ui::dialogs::form_dialog` (`egui::Modal`: Esc/clique fora cancela, Enter envia). Cada formulário é uma struct com `show(ctx, services) -> Outcome` e fica em `Option<...Form>` na view, que a descarta quando `show` não devolve `Open`. Os de cliente e transação são compartilhados em `dialogs.rs`. Erros de validação aparecem dentro do diálogo (a barra de status fica atrás do modal).
- *Ícones:* Phosphor via `egui-pop-phosphor` (fork para egui 0.37; o crate continua se chamando `egui_phosphor`). Fontes registradas em `main.rs`; use `egui_phosphor::regular::NOME` no texto (`format!("{}  Rótulo", icon::PLUS)`). Os nomes ficam em `variants/codepoints.rs` da crate. `Section::icon()` define o do sidebar.
- Nova seção do sidebar: variante em `ui::Section` (+ `ALL` e `label`), arquivo `ui/*_view.rs` e um braço em `create_view`. Controles maiores (`ui::enlarge_controls`) valem para o painel central e para os diálogos, não para o sidebar.

**Cobrança sem webhook.** O PC fica atrás de roteador, então o pagamento é detectado por *polling*: ao gerar o link, grava-se uma `charges` com um `external_ref` próprio (`fiado-<cliente>-<nanos>`); depois `payment_status(external_ref)` consulta `/v1/payments/search`. Quando aprovado, `Db::mark_charge_paid` marca a cobrança e insere a transação de pagamento numa única transação SQL, com `external_id` único: é idempotente e não duplica o lançamento.

**Canais.**
- *Telegram* (`channel/telegram.rs`): bot não inicia conversa pelo telefone. O cliente abre `t.me/<bot>?start=<código>`; uma thread de long polling (`getUpdates`) recebe o `/start <código>` e emite `Event::TelegramLinked`, e `App` grava o `chat_id`. O código é aleatório por cliente (`ensure_telegram_link_code`), nunca o id do cliente, para ninguém se passar por outro. `configure()` para a thread anterior por uma flag `stop`.
- *WhatsApp* (`channel/whatsapp.rs`): `whatsapp-rust` é assíncrono, então o canal tem seu próprio runtime tokio; `send()` usa `Runtime::block_on` e portanto também só deve ser chamado de thread de trabalho. A sessão fica em `whatsapp.db`; "Desvincular" apaga esse arquivo. O status `WaitingQr(String)` carrega o payload que `channels_view` desenha como QR com retângulos do egui.
- Envio automático ao gerar a cobrança é opcional por canal (setting `channel_<chave>_auto`, desligado por padrão para evitar disparos no WhatsApp).

**Banco (`db.rs`).** Migrações por `PRAGMA user_version` (hoje v3) em `Db::migrate`: para mudar o esquema, acrescente um `if version < N`, nunca edite uma migração existente. Dinheiro é sempre `i64` em centavos (`money.rs` converte de/para texto); saldo = soma(dívidas) − soma(pagamentos), positivo significa que o cliente deve. Configurações (token do Mercado Pago, token do Telegram, provedor ativo) ficam na tabela `settings`.

**Dados do usuário.** `%APPDATA%\CaderninhoFiado\` (`fiado.db`, `whatsapp.db`). O nome da pasta é anterior ao título atual da janela ("Gestor de Crédito"); mudar a pasta exige migrar os dados.

## Pendências conhecidas

Backup local e para o Google Drive com restauração, criptografia do banco e dos tokens (hoje em texto puro em `settings`) e compatibilidade com Windows antigo (o Rust atual não compila para Windows 7/XP) ainda não foram feitos.
