# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Projeto

Sistema desktop de fiado (clientes, dívidas e pagamentos) em Rust, MIT, alvo Windows. Interface em egui/eframe, banco SQLite local, cobrança automática por link do Mercado Pago enviado por WhatsApp ou Telegram, com senha de acesso, backup local e no Google Drive. Textos da interface e comentários estão em português do Brasil.

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
- *Tema:* `ui/theme.rs` imita o tema "Pro Medium" do PhotoCraft/ArtCraft (escuro, azul `#378EF0`, botões em pílula). Nunca escreva cores nas telas: use as constantes (`theme::DANGER`, `SUCCESS`...) e os componentes próprios (`Btn::primary/secondary/danger`, `pill_tab`, `nav_item`, `checkbox`, `card`, `badge`, `modal_frame`); botões do `egui` puro destoam. Os tamanhos do PhotoCraft são compactos e todos são multiplicados por `theme::SCALE` (1.2); o sidebar usa o tamanho original (`nav_item`, `Btn::compact`). `theme::apply` roda uma vez em `main.rs`.
- *Ícones:* Phosphor via `egui-pop-phosphor` (fork para egui 0.37; o crate continua se chamando `egui_phosphor`). Fontes registradas em `main.rs`; use `egui_phosphor::regular::NOME` no texto (`format!("{}  Rótulo", icon::PLUS)`). Os nomes ficam em `variants/codepoints.rs` da crate. `Section::icon()` define o do sidebar.
- Nova seção do sidebar: variante em `ui::Section` (+ `ALL` e `label`), arquivo `ui/*_view.rs` e um braço em `create_view`.

**Cobrança sem webhook.** O PC fica atrás de roteador, então o pagamento é detectado por *polling*: ao gerar o link, grava-se uma `charges` com um `external_ref` próprio (`fiado-<cliente>-<nanos>`); depois `payment_status(external_ref)` consulta `/v1/payments/search`. Quando aprovado, `Db::mark_charge_paid` marca a cobrança e insere a transação de pagamento numa única transação SQL, com `external_id` único: é idempotente e não duplica o lançamento.

**Canais.**
- *Telegram* (`channel/telegram.rs`): usa a **conta do usuário** (MTProto, crate `grammers`), não bot, para escrever a qualquer telefone. Exige `api_id`/`api_hash` do usuário. Runtime tokio próprio, como o WhatsApp (`send()` usa `block_on`: só de thread de trabalho). Login em etapas dirigido por `ChannelStatus` (`NeedsLogin` → `AwaitingCode` → `AwaitingPassword` → `Ready`); `TelegramForm` em `channels_view` mostra a etapa certa. O telefone vira usuário com `contacts.ImportContacts` (o número é salvo como contato). A sessão é só `home_dc` + chaves de DC em JSON (`StoredSession`): o `MemorySession` do grammers não persiste sozinho e o `SqliteSession` traria o `libsql` (outro SQLite, risco de símbolos duplicados). O fluxo de `updates` é drenado numa task.
- *WhatsApp* (`channel/whatsapp.rs`): `whatsapp-rust` é assíncrono, então o canal tem seu próprio runtime tokio; `send()` usa `Runtime::block_on` e portanto também só deve ser chamado de thread de trabalho. A sessão fica em `whatsapp.db`; "Desvincular" apaga esse arquivo. O status `WaitingQr(String)` carrega o payload que `channels_view` desenha como QR com retângulos do egui.
- Envio automático ao gerar a cobrança é opcional por canal (setting `channel_<chave>_auto`, desligado por padrão para evitar disparos no WhatsApp).

**Banco (`db.rs`).** Migrações por `PRAGMA user_version` (hoje v4) em `Db::migrate`: para mudar o esquema, acrescente um `if version < N`, nunca edite uma migração existente. Dinheiro é sempre `i64` em centavos (`money.rs` converte de/para texto); saldo = soma(dívidas) − soma(pagamentos), positivo significa que o cliente deve. Configurações ficam na tabela `settings`; as que são segredos (`security::SECRET_KEYS`) vão cifradas — leia/grave sempre por `Services::secret` / `set_secret`, nunca por `db.get_setting` direto.

**Senha e cofre.** `App` tem dois estágios: `LockScreen` (`ui/lock_view.rs`) e `Workspace`. Os `Services` (canais, provedor, Drive) só nascem depois do desbloqueio, porque dependem dos segredos. `security.rs` cria/abre/troca a senha: o banco guarda só o sal (`vault_salt`) e uma prova cifrada (`vault_check`); a chave (`crypto::Vault`, Argon2id + ChaCha20-Poly1305) fica só na memória. Sem senha (`vault_skipped`) os segredos ficam em texto e o Drive fica indisponível. Não há recuperação de senha, e isso é dito ao usuário na interface e no README.

**Backup (`backup/`).** `Db::snapshot_to` usa `VACUUM INTO` (cópia consistente com o app em uso) e **sem a tabela `settings`**: backup nunca leva tokens nem senha. `Db::restore_from` copia online pela API de backup do SQLite e **reaplica as `settings` atuais**, então restaurar não troca a senha. Formato cifrado: `FIADOBK1 | sal(16) | nonce+cifra`; o sal vai no arquivo para um PC novo abrir só com a senha (`open_with_session` tenta antes a chave da sessão quando o sal é o mesmo). `restore_plain` guarda um `...-antes-de-restaurar.db` antes de trocar os dados. O backup automático roda em `Workspace::logic` (a cada 24 h, `Services::backup_due`). Arquivos temporários precisam de nome único (`temp_file`): dois backups simultâneos já colidiram uma vez. `backup/gdrive.rs`: OAuth de app desktop com PKCE e redirecionamento para `127.0.0.1` (a thread de login pede ao UI para abrir o navegador via `Event::OpenUrl`), escopo `drive.file`, pasta própria, rotação de 30 arquivos.

**Dados do usuário.** `%APPDATA%\GestorDeCredito\` (`fiado.db`, `whatsapp.db`, `telegram.session`, `backups\`). A pasta antiga `CaderninhoFiado` é movida automaticamente na primeira abertura (`resolve_data_dir` em `main.rs`); se não der para mover, continua em uso. O nome exibido vem de `ui::APP_NAME`. O banco e as sessões dos canais **não** são criptografados (SQLCipher exigiria compilar o OpenSSL no Windows e conflitaria com o SQLite do WhatsApp).

## Pendências conhecidas

Compatibilidade com Windows antigo (o Rust atual não compila para Windows 7/XP). As integrações com Mercado Pago, WhatsApp, Telegram e Google Drive nunca foram exercitadas contra os serviços reais; só a lógica de apoio tem teste.
