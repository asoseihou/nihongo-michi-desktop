# Licenças dos dados embutidos (`ui/data/`)

O código do app está sob a licença MIT ([LICENSE](LICENSE)). Os dados abaixo não: cada
arquivo segue a licença da sua origem.

| Arquivo | Origem | Licença |
|---|---|---|
| `kanji.json` | **KANJIDIC2** — © Electronic Dictionary Research and Development Group (EDRDG). Níveis JLPT: kanji-data (David Gouveia, MIT). | [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/) — ver <https://www.edrdg.org/edrdg/licence.html> |
| `strokes.json` | **KanjiVG** — © Ulrich Apel (caminhos SVG da ordem dos traços) | [CC BY-SA 3.0](https://creativecommons.org/licenses/by-sa/3.0/) — <https://kanjivg.tagaini.net> |
| `vocab.json` | open-anki-jlpt-decks (MIT), baseado nas listas JLPT de Jonathan Waller (tanos.co.uk) | MIT / [CC BY](https://creativecommons.org/licenses/by/4.0/) |
| `content.json` | Conteúdo original do Nihongo Michi (lições, pontos de gramática, frases em pedaços, leituras, frases de viagem, mapa da Linha Wagotabi, recursos) | do autor |

Estes arquivos foram convertidos para JSON (campos selecionados, sem mudar o conteúdo) e
continuam sob as mesmas licenças: quem redistribuir o app ou os arquivos deve manter estes
créditos e a licença CC BY-SA dos dados derivados de KANJIDIC2 e KanjiVG.

Wagotabi é um jogo da Wagotabi Limited; o mapa da Linha Wagotabi usa só informações
públicas (notas de atualização e lojas) e não reproduz conteúdo do jogo. Genki é citado
apenas por número de lição.

O som do aviso é uma composição original sintetizada pelo próprio app (`src-tauri/src/sound.rs`).
