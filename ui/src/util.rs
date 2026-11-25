//! Small helpers shared by the views (port of `01-core.js` utilities).

use wasm_bindgen::JsCast;

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

/// `md(s)`: escaped text with **bold** and line breaks.
pub fn md(s: &str) -> String {
    let e = esc(s);
    let mut out = String::new();
    let mut rest = e.as_str();
    while let Some(i) = rest.find("**") {
        let after = &rest[i + 2..];
        match after.find("**") {
            Some(j) if j > 0 => {
                out.push_str(&rest[..i]);
                out.push_str("<b>");
                out.push_str(&after[..j]);
                out.push_str("</b>");
                rest = &after[j + 2..];
            }
            _ => {
                out.push_str(&rest[..i + 2]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out.replace('\n', "<br>")
}

pub fn safe_url(u: &str) -> String {
    if u.starts_with("https://") {
        u.to_string()
    } else {
        "#".into()
    }
}

pub fn yt_search(q: &str) -> String {
    format!("https://www.youtube.com/results?search_query={}", js_sys::encode_uri_component(q))
}

pub fn clamp(x: f64, a: f64, b: f64) -> f64 {
    x.max(a).min(b)
}

pub fn rand() -> f64 {
    js_sys::Math::random()
}

pub fn rand_n(n: usize) -> usize {
    if n == 0 {
        0
    } else {
        ((rand() * n as f64).floor() as usize).min(n - 1)
    }
}

pub fn shuffle<T: Clone>(a: &[T]) -> Vec<T> {
    let mut v = a.to_vec();
    for i in (1..v.len()).rev() {
        let j = rand_n(i + 1);
        v.swap(i, j);
    }
    v
}

pub fn pick<T: Clone>(a: &[T]) -> Option<T> {
    if a.is_empty() {
        None
    } else {
        Some(a[rand_n(a.len())].clone())
    }
}

/// `uid()`: time in base 36 plus 5 random base-36 digits.
pub fn uid() -> String {
    const B: f64 = 60_466_176.0; // 36^5
    let r = radix36((rand() * B) as u64 + B as u64);
    format!("{}{}", radix36(js_sys::Date::now() as u64), &r[1..])
}

fn radix36(mut n: u64) -> String {
    const D: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".into();
    }
    let mut s = Vec::new();
    while n > 0 {
        s.push(D[(n % 36) as usize]);
        n /= 36;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

pub fn window() -> web_sys::Window {
    web_sys::window().expect("window")
}

pub fn document() -> web_sys::Document {
    window().document().expect("document")
}

pub fn by_id(id: &str) -> Option<web_sys::HtmlElement> {
    document().get_element_by_id(id).and_then(|e| e.dyn_into().ok())
}

pub fn scroll_top() {
    window().scroll_to_with_x_and_y(0.0, 0.0);
}

pub fn scroll_into_view(id: &str) {
    if let Some(e) = document().get_element_by_id(id) {
        e.scroll_into_view();
    }
}

/// Value of the input/select/textarea that fired an event.
pub fn ev_value(ev: &web_sys::Event) -> String {
    let t = ev.target();
    if let Some(i) = t.as_ref().and_then(|t| t.dyn_ref::<web_sys::HtmlInputElement>()) {
        return i.value();
    }
    if let Some(s) = t.as_ref().and_then(|t| t.dyn_ref::<web_sys::HtmlSelectElement>()) {
        return s.value();
    }
    if let Some(a) = t.as_ref().and_then(|t| t.dyn_ref::<web_sys::HtmlTextAreaElement>()) {
        return a.value();
    }
    String::new()
}

pub fn ev_checked(ev: &web_sys::Event) -> bool {
    ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()).map(|i| i.checked()).unwrap_or(false)
}

/// Shows a "+N XP" floater above the clicked element (`addXP(n, el)`).
pub fn xp_float(n: f64, ev: Option<&web_sys::MouseEvent>) {
    let Some(ev) = ev else { return };
    let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
    let r = el.get_bounding_client_rect();
    let doc = document();
    let Ok(f) = doc.create_element("div") else { return };
    f.set_class_name("xpfloat");
    f.set_text_content(Some(&format!("+{} XP", n)));
    let _ = f.set_attribute("style", &format!("left:{}px;top:{}px", r.left() + r.width() / 2.0 - 20.0, r.top() - 10.0));
    if let Some(b) = doc.body() {
        let _ = b.append_child(&f);
        let f2 = f.clone();
        leptos::prelude::set_timeout(move || f2.remove(), std::time::Duration::from_millis(1000));
    }
}

/// Percent string for a bar.
pub fn pct(frac: f64) -> String {
    format!("{}%", (clamp(frac, 0.0, 1.0) * 100.0).round())
}

/// `await sleep(ms)`.
pub async fn sleep(ms: i32) {
    let p = js_sys::Promise::new(&mut |res, _| {
        let _ = window().set_timeout_with_callback_and_timeout_and_arguments_0(&res, ms);
    });
    let _ = wasm_bindgen_futures::JsFuture::from(p).await;
}

/// Forces a style flush (`void el.getBoundingClientRect()`), so a CSS
/// transition starts from the values just set.
pub fn reflow(el: &web_sys::Element) {
    let _ = el.get_bounding_client_rect();
}
