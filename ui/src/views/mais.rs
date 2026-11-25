//! "Mais" on the phone-width tab bar (`Views.mais`).

use leptos::prelude::*;

#[component]
pub fn Mais() -> impl IntoView {
    let cards = [
        ("imersao", "浸", "Imersão", "Vlogs, podcasts, Wagotabi e o seu registro."),
        ("sensei", "先生", "Sensei IA", "Tire dúvidas, corrija frases e gere exercícios novos."),
        ("anki", "暗記", "Anki", "Exporte baralhos, importe seu progresso, sincronize."),
        ("avisos", "🔔", "Central de avisos", "Histórico dos lembretes, perdidos e silenciados."),
        ("ajustes", "設定", "Ajustes", "Rotina, lembretes, metas, voz, backup."),
    ]
    .into_iter()
    .map(|(r, j, t, d)| {
        view! {
            <a class="gamecard" href=format!("#{r}") style="text-decoration:none;color:inherit">
                <span class="gj">{j}</span><span class="gt">{t}</span><span class="small muted">{d}</span>
            </a>
        }
    })
    .collect_view();
    view! { <h1>"Mais"</h1><div class="grid3">{cards}</div> }
}
