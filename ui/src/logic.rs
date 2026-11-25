//! View-independent logic shared by several screens: SRS decks and queues
//! (`03-srs.js`), station progress and the trip line (`10-today.js`).

use crate::data::{self, Content, Kanji, Point, Station};
use crate::jp::{kana_set, plain_j};
use crate::store;
use michi_core::time::{add_days_start, day_start, now_ms, parse_key, DAY_MS, MONTH_PT};
use std::rc::Rc;

/* ---------------- lines ---------------- */

pub struct LineInfo {
    pub name: &'static str,
    pub jp: &'static str,
    pub lc: &'static str,
}

pub fn line(l: &str) -> LineInfo {
    let (name, jp, lc) = match l {
        "K" => ("Linha Kana", "かな線", "var(--kana)"),
        "N5" => ("Linha N5 · Genki I", "初級線", "var(--n5)"),
        "N4" => ("Linha N4 · Genki II", "初中級線", "var(--n4)"),
        "N3" => ("Linha N3 · Ponte", "中級線", "var(--n3)"),
        "N2" => ("Linha N2", "中上級線", "var(--n2)"),
        "N1" => ("Linha N1", "上級線", "var(--n1)"),
        "V" => ("Situações de viagem", "旅行線", "var(--trip)"),
        _ => ("Linha Viagem 2028", "旅の路線", "var(--trip)"),
    };
    LineInfo { name, jp, lc }
}

/// `codeBadge(st, sm)` as HTML.
pub fn code_badge(st: &Station, sm: bool) -> String {
    let id = &st.id;
    let n = id.chars().count();
    let head: String = id.chars().take(n.saturating_sub(2)).collect();
    let tail: String = id.chars().skip(n.saturating_sub(2)).collect();
    format!(
        r#"<span class="code {}" style="--lc:{}"><small>{}</small><b>{}</b></span>"#,
        if sm { "sm" } else { "" },
        line(&st.line).lc,
        crate::util::esc(&head),
        crate::util::esc(&tail)
    )
}

/* ---------------- stations ---------------- */

pub fn station_kanji(st: &Station) -> Vec<Kanji> {
    match (st.kanji, data::kanji()) {
        (Some((a, b)), Some(k)) => k.iter().skip(a).take(b.saturating_sub(a) + 1).cloned().collect(),
        _ => vec![],
    }
}

pub fn station_points<'a>(c: &'a Content, st: &Station) -> Vec<&'a Point> {
    st.lessons.iter().filter_map(|l| c.lesson(l)).flat_map(|l| l.points.iter()).collect()
}

pub struct Progress {
    pub g: Option<f64>,
    pub k: Option<f64>,
    pub ka: Option<f64>,
    pub best: f64,
    pub frac: f64,
    pub done: bool,
    pub n_pts: usize,
    pub n_k: usize,
    pub n_kana: usize,
}

/// `stationProgress(st)`.
pub fn station_progress(c: &Content, st: &Station) -> Progress {
    let j = store::read(|s| (s.j.points.clone(), s.j.best.get(&st.id).copied(), s.j.done.contains_key(&st.id)));
    let (pj, best, done) = j;
    let pts = station_points(c, st);
    let ks = station_kanji(st);
    let kn = st.kana.as_deref().map(kana_set).unwrap_or_default();
    let g = (!pts.is_empty()).then(|| {
        pts.iter().filter(|p| pj.get(&p.id).map(|x| x.studied > 0.0).unwrap_or(false)).count() as f64 / pts.len() as f64
    });
    let k = (!ks.is_empty())
        .then(|| ks.iter().filter(|x| store::card(&format!("k:{}", x.k)).is_some()).count() as f64 / ks.len() as f64);
    let ka = (!kn.is_empty())
        .then(|| store::read(|s| kn.iter().filter(|x| s.ks.strength(&x.k) >= 0.8).count()) as f64 / kn.len() as f64);
    let best = best.unwrap_or(0.0);
    let parts: Vec<f64> = [g, k, ka].into_iter().flatten().collect();
    let frac = if done {
        1.0
    } else {
        (if parts.is_empty() { 0.0 } else { parts.iter().sum::<f64>() / parts.len() as f64 * 0.8 })
            + if best >= 80.0 { 0.2 } else { 0.0 }
    };
    Progress { g, k, ka, best, frac: frac.min(1.0), done, n_pts: pts.len(), n_k: ks.len(), n_kana: kn.len() }
}

pub fn main_line(c: &Content) -> Vec<&Station> {
    c.stations.iter().filter(|s| s.line != "V" && s.line != "T").collect()
}

/// `currentStation()`: first main-line station not done.
pub fn current_station(c: &Content) -> Option<&Station> {
    let m = main_line(c);
    let done = store::read(|s| s.j.done.clone());
    m.iter().find(|s| !done.contains_key(&s.id)).copied().or_else(|| m.last().copied())
}

/// `setPosition(stId, markKanji)`: everything before the station counts as passed.
pub fn set_position(st_id: &str, mark_kanji: bool) -> Option<String> {
    let c = data::content()?;
    let m = main_line(&c);
    let idx = m.iter().position(|s| s.id == st_id)?;
    let mut nk = 0;
    let now = now_ms();
    for (i, s) in m.iter().enumerate() {
        if i < idx {
            store::write(&["journey"], |st| {
                st.j.done.entry(s.id.clone()).or_insert(1.0);
            });
            if mark_kanji {
                for k in station_kanji(s) {
                    let id = format!("k:{}", k.k);
                    if store::card(&id).is_none() {
                        store::set_card(
                            &id,
                            Some(michi_core::srs::mark_known(14.0, now, crate::util::rand(), crate::util::rand())),
                        );
                        nk += 1;
                    }
                }
            }
        } else {
            store::write(&["journey"], |st| {
                if st.j.done.get(&s.id) == Some(&1.0) {
                    st.j.done.remove(&s.id);
                }
            });
        }
    }
    store::write(&["profile"], |s| s.p.onboarded = true);
    Some(format!(
        "Trem posicionado em {st_id}.{}",
        if nk > 0 { format!(" {nk} kanji anteriores entraram como revisão.") } else { String::new() }
    ))
}

/* ---------------- trip line ---------------- */

pub fn trip_dates() -> (String, String) {
    store::read(|s| s.p.trip_dates())
}

pub struct Countdown {
    pub days: i64,
    pub total: i64,
    pub during: bool,
    pub after: bool,
}

pub fn trip_countdown() -> Countdown {
    let (s, e) = trip_dates();
    let (Some(s), Some(e)) = (parse_key(&s), parse_key(&e)) else {
        return Countdown { days: 0, total: 0, during: false, after: false };
    };
    let s_ms = michi_core::time::ms_of(s.and_hms_opt(0, 0, 0).unwrap_or_default());
    let e_ms = michi_core::time::ms_of(e.and_hms_opt(0, 0, 0).unwrap_or_default());
    let now = now_ms();
    Countdown {
        days: ((s_ms - now) / DAY_MS).ceil() as i64,
        total: ((e_ms - s_ms) / DAY_MS).round() as i64 + 1,
        during: now >= s_ms && now < e_ms + DAY_MS,
        after: now >= e_ms + DAY_MS,
    }
}

pub fn trip_range() -> String {
    use chrono::Datelike;
    let (s, e) = trip_dates();
    let (Some(s), Some(e)) = (parse_key(&s), parse_key(&e)) else { return String::new() };
    let m = |d: chrono::NaiveDate| MONTH_PT[d.month0() as usize];
    if s.month() == e.month() && s.year() == e.year() {
        format!("{}–{} {} {}", s.day(), e.day(), m(s), s.year())
    } else {
        format!("{} {} – {} {} {}", s.day(), m(s), e.day(), m(e), e.year())
    }
}

pub fn trip_stations(c: &Content) -> Vec<&Station> {
    c.stations.iter().filter(|s| s.line == "T").collect()
}

pub struct Ready {
    pub g: f64,
    pub boss: f64,
    pub spoken: f64,
    pub signs: f64,
    pub frac: f64,
}

/// `stationReady(st)`: grammar + boss + sentences spoken + sign kanji read.
pub fn station_ready(c: &Content, st: &Station) -> Ready {
    let pr = station_progress(c, st);
    let sents: Vec<String> = station_points(c, st).iter().flat_map(|p| p.s.iter().map(|s| s.id.clone())).collect();
    let (sp, sg) = store::read(|s| (s.j.spoke.clone(), s.j.signs.clone()));
    let spoken = if sents.is_empty() {
        0.0
    } else {
        sents.iter().filter(|id| sp.get(*id).map(|x| *x != 0.0).unwrap_or(false)).count() as f64 / sents.len() as f64
    };
    let signs = if st.signs.is_empty() {
        0.0
    } else {
        st.signs.iter().filter(|x| sg.get(&plain_j(&x.0)).copied().unwrap_or(0.0) >= 1.0).count() as f64
            / st.signs.len() as f64
    };
    let boss = if pr.best >= 80.0 { 1.0 } else { pr.best / 100.0 };
    let g = pr.g.unwrap_or(0.0);
    Ready { g, boss, spoken, signs, frac: (g + boss + spoken + signs) / 4.0 }
}

pub fn trip_ready(c: &Content) -> f64 {
    let sts = trip_stations(c);
    if sts.is_empty() {
        0.0
    } else {
        sts.iter().map(|s| station_ready(c, s).frac).sum::<f64>() / sts.len() as f64
    }
}

pub fn city_ready(c: &Content, city: &str) -> f64 {
    let sts: Vec<&Station> = trip_stations(c).into_iter().filter(|s| s.city.as_deref() == Some(city)).collect();
    if sts.is_empty() {
        0.0
    } else {
        sts.iter().map(|s| station_ready(c, s).frac).sum::<f64>() / sts.len() as f64
    }
}

/// `tripHeadline(cd)` as HTML.
pub fn trip_headline(cd: &Countdown) -> String {
    if cd.after {
        "おかえり！ Viagem concluída".into()
    } else if cd.during {
        "Você está no Japão! 🇯🇵".into()
    } else {
        format!(r#"Faltam <span class="num">{}</span> dias"#, cd.days)
    }
}

/* ---------------- SRS decks ---------------- */

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Deck {
    Kanji,
    Vocab,
    Grammar,
    Mined,
}

impl Deck {
    pub const ALL: [Deck; 4] = [Deck::Kanji, Deck::Vocab, Deck::Grammar, Deck::Mined];
    pub fn id(&self) -> &'static str {
        match self {
            Deck::Kanji => "kanji",
            Deck::Vocab => "vocab",
            Deck::Grammar => "grammar",
            Deck::Mined => "mined",
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Deck::Kanji => "Kanji 2k",
            Deck::Vocab => "Vocabulário JLPT",
            Deck::Grammar => "Gramática",
            Deck::Mined => "Mineração",
        }
    }
    pub fn jp(&self) -> &'static str {
        match self {
            Deck::Kanji => "漢字",
            Deck::Vocab => "語彙",
            Deck::Grammar => "文法",
            Deck::Mined => "採掘",
        }
    }
    pub fn lc(&self) -> &'static str {
        match self {
            Deck::Kanji => "var(--n3)",
            Deck::Vocab => "var(--n4)",
            Deck::Grammar => "var(--n5)",
            Deck::Mined => "var(--kana)",
        }
    }
    /// Daily limit of new cards (`limitKey`).
    pub fn limit(&self) -> f64 {
        store::read(|s| match self {
            Deck::Kanji => s.p.new_kanji,
            Deck::Vocab => s.p.new_vocab,
            Deck::Grammar => s.p.new_grammar,
            Deck::Mined => s.p.new_mined,
        })
    }
    pub fn ready(&self) -> bool {
        match self {
            Deck::Kanji => data::kanji().is_some(),
            Deck::Vocab => data::vocab().is_some(),
            Deck::Grammar => data::content().is_some(),
            Deck::Mined => true,
        }
    }
    pub fn ids(&self) -> Vec<String> {
        match self {
            Deck::Kanji => data::kanji().map(|k| k.iter().map(|x| format!("k:{}", x.k)).collect()).unwrap_or_default(),
            Deck::Vocab => {
                data::vocab().map(|v| v.iter().map(|x| format!("v:{}|{}", x.0, x.1)).collect()).unwrap_or_default()
            }
            Deck::Grammar => {
                let c: Option<Rc<Content>> = data::content();
                store::read(|s| {
                    s.j.points
                        .iter()
                        .filter(|(pid, st)| {
                            st.studied > 0.0 && c.as_ref().map(|c| c.point(pid).is_some()).unwrap_or(false)
                        })
                        .map(|(pid, _)| format!("g:{pid}"))
                        .collect()
                })
            }
            Deck::Mined => store::read(|s| s.mined.list.iter().map(|m| format!("m:{}", m.id)).collect()),
        }
    }
    pub fn of_id(id: &str) -> Option<Deck> {
        match id.chars().next()? {
            'k' => Some(Deck::Kanji),
            'v' => Some(Deck::Vocab),
            'g' => Some(Deck::Grammar),
            'm' => Some(Deck::Mined),
            _ => None,
        }
    }
}

pub struct Queue {
    pub learn: Vec<String>,
    pub rev: Vec<String>,
    pub fresh: Vec<String>,
}

/// `queueFor(deckIds)`.
pub fn queue_for(decks: &[Deck]) -> Queue {
    let now = now_ms();
    let end_today = day_start(now) + DAY_MS;
    let d = store::today();
    let mut q = Queue { learn: vec![], rev: vec![], fresh: vec![] };
    for deck in decks {
        if !deck.ready() {
            continue;
        }
        let mut left = (deck.limit() - d.nw.get(deck.id()).copied().unwrap_or(0.0)).max(0.0);
        for id in deck.ids() {
            match store::card(&id) {
                None => {
                    if left > 0.0 {
                        q.fresh.push(id);
                        left -= 1.0;
                    }
                }
                Some(c) if c.state() == 2 => {
                    if c.due() < end_today {
                        q.rev.push(id);
                    }
                }
                Some(_) => q.learn.push(id),
            }
        }
    }
    q.learn.sort_by(|a, b| {
        let da = store::card(a).map(|c| c.due()).unwrap_or(0.0);
        let db = store::card(b).map(|c| c.due()).unwrap_or(0.0);
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    });
    q.rev = crate::util::shuffle(&q.rev);
    q
}

pub struct Due {
    pub learn: usize,
    pub rev: usize,
    pub fresh: usize,
}

/// `dueCounts(did)`.
pub fn due_counts(deck: Deck) -> Option<Due> {
    if !deck.ready() {
        return None;
    }
    let q = queue_for(&[deck]);
    let now = now_ms();
    Some(Due {
        learn: q
            .learn
            .iter()
            .filter(|id| store::card(id).map(|c| c.due() <= now + 20.0 * 60_000.0).unwrap_or(false))
            .count(),
        rev: q.rev.len(),
        fresh: q.fresh.len(),
    })
}

/// `totalDue()`.
pub fn total_due() -> Due {
    let mut t = Due { learn: 0, rev: 0, fresh: 0 };
    for d in Deck::ALL {
        if let Some(c) = due_counts(d) {
            t.learn += c.learn;
            t.rev += c.rev;
            t.fresh += c.fresh;
        }
    }
    t
}

/// The SRS block completes itself once ≥ 10 reviews were done and nothing
/// is due. The flag is stored so the backend scheduler sees it too.
pub fn srs_block_done() -> bool {
    let d = store::today();
    if d.srs_clear {
        return true;
    }
    if d.rev < 10.0 {
        return false;
    }
    let t = total_due();
    let clear = t.rev + t.learn == 0;
    if clear {
        store::with_today(|d| d.srs_clear = true);
    }
    clear
}

/// `blockDone(b, d)` for today.
pub fn block_done_today(id: &str) -> bool {
    let d = store::today();
    if d.done.get(id).copied().unwrap_or(false) {
        return true;
    }
    if id == "srs" {
        return srs_block_done();
    }
    d.auto_done(id)
}

/// `vlogOfDay()`: weekdays at your level (N5/N4), Saturday a native-speed one.
pub fn vlog_of_day(c: &Content) -> Option<crate::data::Resource> {
    let all: Vec<_> = c
        .resources
        .items
        .iter()
        .filter(|x| x.kind == "vlog" && !x.name.to_lowercase().contains("archive"))
        .cloned()
        .collect();
    if all.is_empty() {
        return None;
    }
    let sat = michi_core::time::local(now_ms()).format("%w").to_string() == "6";
    let items: Vec<_> = all
        .iter()
        .filter(|x| if sat { x.level.contains("N3") } else { x.level.contains("N5") || x.level.contains("N4") })
        .cloned()
        .collect();
    let list = if items.is_empty() { all } else { items };
    let n = (day_start(now_ms()) / DAY_MS).floor() as usize;
    list.get(n % list.len()).cloned()
}

/// When the next review of a card falls (for previews).
pub fn next_day_start(n: i64) -> f64 {
    add_days_start(n, now_ms())
}
