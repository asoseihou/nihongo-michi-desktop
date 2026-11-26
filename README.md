# Nihongo Michi — desktop (Rust)

[![CI](https://github.com/asoseihou/nihongo-michi-desktop/actions/workflows/ci.yml/badge.svg)](https://github.com/asoseihou/nihongo-michi-desktop/actions/workflows/ci.yml)

Versão desktop do **Nihongo Michi** (日本語の道), o app de estudo de japonês que nasceu
como app web (o "artefato"). Feita em **Rust**, com **Tauri 2** (janela nativa com
WebView2, sem Electron) e interface em **Leptos** compilada para WebAssembly pelo **Trunk**.

- Dados no **SQLite** local (`%APPDATA%\io.github.asoseihou.nihongomichi\michi.db`).
- **Importa o backup** do artefato (Ajustes → 💾 Exportar backup) e exporta no mesmo formato.
- **Avisos próprios**, sem notificações do Windows: o agendador roda na bandeja e, na hora
  marcada, abre uma janelinha sempre por cima, no canto da tela, no visual "quadro de
  partidas", com som e os botões **Feito / Adiar / Abrir**. Central de avisos com histórico,
  aviso dos lembretes perdidos com o app fechado e horário de silêncio.
- **Agenda interna**: os blocos da rotina viram eventos recorrentes (RRULE) e os 29 dias da
  viagem de dezembro de 2028 aparecem como eventos de dia inteiro, coloridos por cidade.
  Marcar como feito pela agenda ou pelo aviso alimenta o quadro de Hoje, a sequência de
  dias e o mapa de calor.
- **Modo foco**: o ▶ de cada bloco (ou o cartão em Hoje, ou o aviso) abre um cronômetro numa
  janelinha sempre visível; os minutos entram no dia e cada 3 minutos dão 1 XP.
- **Backup automático** diário (`Documentos\Nihongo Michi\Backups`) e cópia de segurança antes
  de restaurar, zerar ou trazer o progresso de outro PC.
- **Sincronizar entre PCs** por uma pasta compartilhada (OneDrive, Google Drive, Dropbox ou
  pendrive): cada PC grava o próprio arquivo e lê o dos outros, e o progresso se junta sozinho.
- **Atualização automática** pelas releases do GitHub, com o instalador assinado.

Interface em português; o conteúdo de ensino segue em inglês + japonês.

## Instalar

Baixe o `Nihongo Michi_<versão>_x64-setup.exe` em
[Releases](https://github.com/asoseihou/nihongo-michi-desktop/releases) e abra. Ele instala só
para o seu usuário (sem administrador) e funciona no Windows 10/11 de 64 bits.

- O instalador não é assinado. Se aparecer "O Windows protegeu o computador", clique em
  **Mais informações → Executar assim mesmo**.
- Para trazer o progresso da versão web: Ajustes → 📂 Restaurar backup.
- Para os avisos chegarem sem abrir o app, ligue **Abrir com o Windows** em Ajustes → Lembretes.
  Fechar a janela deixa o app na bandeja; para sair, use **Sair** no ícone 道.

## Andamento

| Fase | O que entra | Estado |
|---|---|---|
| 1 | Workspace Tauri 2 + Leptos, SQLite com migrações, dados embutidos, importação do backup, navegação, tela **Hoje** com a rotina, bandeja + agendador + janela de aviso | ✅ |
| 2 | Demais telas: Jornada (estações, chefão, selos, Linha Viagem 2028), Revisar (SRS + ordem dos traços), 10 minigames, Frases (montar, partículas, raio-X, ditado, leituras, 🎤 Falar com Whisper local), Imersão, Anki | ✅ |
| 3 | Agenda completa: mês, semana, dia e lista; repetição; vários lembretes; arrastar e redimensionar; exportar .ics | ✅ |
| 4 | Instalador NSIS e releases no GitHub Actions | ✅ |
| 0.2 | Backup automático, sincronização entre PCs, atualização automática e modo foco | ✅ |

O **Sensei** (IA) fica fora da primeira versão.

## Rodar em modo de desenvolvimento

Pré-requisitos (Windows): Rust estável com o alvo `wasm32-unknown-unknown`, as Build Tools
do Visual Studio (C++), o WebView2 (já vem no Windows 11), `trunk` e `tauri-cli`:

```powershell
rustup target add wasm32-unknown-unknown
cargo install --locked trunk
cargo install --locked tauri-cli --version "^2"
```

O reconhecimento de voz do 🎤 Falar usa o Whisper (whisper.cpp via `whisper-rs`), que
precisa do CMake e do libclang para compilar. Sem administrador:

```powershell
python -m pip install --user cmake libclang
```

```powershell
. .\tools\dev-env.ps1
```

O modelo de voz (~190 MB) é baixado pelo próprio app, quando você pede, e fica na pasta de dados.

```powershell
cargo tauri dev
```

Testes da lógica compartilhada e do backend (SRS, RRULE, rotina, agendador, backup, SQLite):

```powershell
cargo test
```

Instalador (gera `target\release\bundle\nsis\*-setup.exe`):

```powershell
cargo tauri build
```

## Releases

Cada tag `v*` enviada ao GitHub dispara o workflow **Release**, que compila no Windows e
publica o instalador na página de Releases. O workflow **CI** roda os testes a cada push.

O texto da release é a seção da versão em [NOVIDADES.md](NOVIDADES.md) (o app mostra essa parte
depois de se atualizar). O instalador e o `latest.json` da atualização automática são assinados
com a chave do projeto, guardada nos secrets `TAURI_SIGNING_PRIVATE_KEY` e
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` do repositório. A chave fica fora do repositório, em
`%USERPROFILE%\.tauri`, e o `tools/dev-env.ps1` a carrega para o `cargo tauri build` local.

```powershell
git tag v0.2.0
```

```powershell
git push origin v0.2.0
```

## Estrutura

```
crates/michi-core   lógica compartilhada (nativo + wasm): dia de estudo às 04:00, SRS (SM-2),
                    rotina, RRULE, documentos do backup, agenda, itinerário da viagem
src-tauri           backend: SQLite + migrações, comandos, agendador, bandeja, janela de aviso, som,
                    voz (cpal + Whisper), arquivos e AnkiConnect
ui                  interface Leptos (Trunk): shell, telas, janela de aviso; ui/data = dados embutidos
tools               scripts auxiliares (ícones)
```

## Dados e licenças

O código está sob a licença [MIT](LICENSE). Os dados de ensino têm licenças próprias — ver
[DATA-LICENSES.md](DATA-LICENSES.md). Em resumo: KANJIDIC2 (© EDRDG, CC BY-SA 4.0) e
KanjiVG (© Ulrich Apel, CC BY-SA 3.0) continuam sob CC BY-SA, com crédito na tela de Ajustes.
