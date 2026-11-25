# Nihongo Michi — desktop (Rust)

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

Interface em português; o conteúdo de ensino segue em inglês + japonês.

## Andamento

| Fase | O que entra | Estado |
|---|---|---|
| 1 | Workspace Tauri 2 + Leptos, SQLite com migrações, dados embutidos, importação do backup, navegação, tela **Hoje** com a rotina, bandeja + agendador + janela de aviso | ✅ |
| 2 | Demais telas: Jornada (estações, chefão, selos, Linha Viagem 2028), Revisar (SRS + ordem dos traços), 10 minigames, Frases (montar, partículas, raio-X, ditado, leituras, 🎤 Falar), Imersão, Anki | em andamento |
| 3 | Agenda completa: mês, semana, dia e lista; repetição; vários lembretes; arrastar e redimensionar | — |
| 4 | Instalador e releases no GitHub Actions | — |

O **Sensei** (IA) fica fora da primeira versão.

## Rodar em modo de desenvolvimento

Pré-requisitos (Windows): Rust estável com o alvo `wasm32-unknown-unknown`, as Build Tools
do Visual Studio (C++), o WebView2 (já vem no Windows 11), `trunk` e `tauri-cli`:

```powershell
rustup target add wasm32-unknown-unknown
cargo install --locked trunk
cargo install --locked tauri-cli --version "^2"
```

```powershell
cargo tauri dev
```

Testes da lógica compartilhada e do backend (SRS, RRULE, rotina, agendador, backup, SQLite):

```powershell
cargo test
```

## Estrutura

```
crates/michi-core   lógica compartilhada (nativo + wasm): dia de estudo às 04:00, SRS (SM-2),
                    rotina, RRULE, documentos do backup, agenda, itinerário da viagem
src-tauri           backend: SQLite + migrações, comandos, agendador, bandeja, janela de aviso, som
ui                  interface Leptos (Trunk): shell, telas, janela de aviso; ui/data = dados embutidos
tools               scripts auxiliares (ícones)
```

## Dados e licenças

O código é do autor. Os dados de ensino têm licenças próprias — ver
[DATA-LICENSES.md](DATA-LICENSES.md). Em resumo: KANJIDIC2 (© EDRDG, CC BY-SA 4.0) e
KanjiVG (© Ulrich Apel, CC BY-SA 3.0) continuam sob CC BY-SA, com crédito na tela de Ajustes.
