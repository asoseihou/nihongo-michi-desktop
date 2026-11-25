//! Tabs that are ported in phase 2 (Jornada, Revisar, Jogos, Frases,
//! Imersão, Sensei, Anki).

use crate::app::ROUTES;
use leptos::prelude::*;

#[component]
pub fn Soon(route: String) -> impl IntoView {
    let (label, jp) = ROUTES.iter().find(|x| x.0 == route).map(|x| (x.1, x.2)).unwrap_or(("", ""));
    let what = match route.as_str() {
        "jornada" => "Mapa de linhas, estações, chefão e selos, Linha Wagotabi e Linha Viagem 2028.",
        "revisar" => "Revisões SRS no estilo Anki (kanji, vocabulário, gramática e mineração) e a ordem dos traços.",
        "jogos" => "Os 10 minigames (chuva de kana, memória, partículas…).",
        "frases" => "Montar, partículas, raio-X, ditado, leituras e o modo 🎤 Falar.",
        "imersao" => "Vlogs e podcasts do dia, registro de imersão e a linha Wagotabi.",
        "sensei" => "O Sensei fica para depois: a primeira versão sai sem ele.",
        "anki" => "Exportar .txt com GUID, importar e AnkiConnect.",
        _ => "",
    };
    view! {
        <h1>{label}" "<span class="jp muted small">{jp}</span></h1>
        <div class="empty stack" style="align-items:center">
            <div class="bigq" style="font-size:56px">{jp}</div>
            <b>"Esta tela chega na fase 2 do porte."</b>
            <div class="small">{what}</div>
            <div class="tiny faint">"Seu progresso desta área já está salvo: veio no backup e aparece aqui quando a tela for portada."</div>
        </div>
    }
}
