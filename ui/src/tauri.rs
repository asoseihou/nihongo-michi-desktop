//! Bridge to the Tauri backend (`window.__TAURI__`, enabled by
//! `withGlobalTauri`). Arguments and results cross as JSON text, so serde
//! attributes (flatten, rename…) behave exactly as on the Rust side.

use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    async fn invoke_js(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = listen, catch)]
    async fn listen_js(event: &str, handler: &Closure<dyn FnMut(JsValue)>) -> Result<JsValue, JsValue>;
}

/// True inside the desktop app (false when the UI is opened in a browser).
pub fn available() -> bool {
    js_sys::Reflect::get(&js_sys::global(), &"__TAURI__".into()).map(|v| !v.is_undefined()).unwrap_or(false)
}

/// Label of this webview window ("main" or "aviso").
pub fn window_label() -> String {
    let get = |o: &JsValue, k: &str| js_sys::Reflect::get(o, &k.into()).unwrap_or(JsValue::UNDEFINED);
    let internals = get(&js_sys::global(), "__TAURI_INTERNALS__");
    let meta = get(&internals, "metadata");
    let cur = get(&meta, "currentWindow");
    get(&cur, "label").as_string().unwrap_or_else(|| "main".into())
}

fn to_js<A: Serialize>(args: &A) -> JsValue {
    let text = serde_json::to_string(args).unwrap_or_else(|_| "{}".into());
    js_sys::JSON::parse(&text).unwrap_or(JsValue::NULL)
}

fn err_text(e: JsValue) -> String {
    e.as_string()
        .or_else(|| js_sys::JSON::stringify(&e).ok().and_then(|s| s.as_string()))
        .unwrap_or_else(|| "erro desconhecido".into())
}

/// Calls a backend command. `args` keys are camelCase (Tauri maps them to
/// the Rust snake_case parameters).
pub async fn invoke<T: DeserializeOwned, A: Serialize>(cmd: &str, args: &A) -> Result<T, String> {
    if !available() {
        return Err("fora do app desktop".into());
    }
    let out = invoke_js(cmd, to_js(args)).await.map_err(err_text)?;
    let text = if out.is_undefined() || out.is_null() {
        "null".to_string()
    } else {
        js_sys::JSON::stringify(&out).ok().and_then(|s| s.as_string()).unwrap_or_else(|| "null".into())
    };
    serde_json::from_str(&text).map_err(|e| format!("{cmd}: resposta inesperada ({e})"))
}

/// Fire-and-forget command; errors only go to the console.
pub fn call<A: Serialize + 'static>(cmd: &'static str, args: A) {
    spawn_local(async move {
        if let Err(e) = invoke::<serde_json::Value, _>(cmd, &args).await {
            web_sys::console::warn_1(&format!("{cmd}: {e}").into());
        }
    });
}

#[derive(Serialize)]
pub struct NoArgs {}

/// Listens to a backend event for the lifetime of the page.
pub fn listen<T: DeserializeOwned + 'static>(event: &'static str, mut f: impl FnMut(T) + 'static) {
    if !available() {
        return;
    }
    let cb = Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
        let payload = js_sys::Reflect::get(&e, &"payload".into()).unwrap_or(JsValue::NULL);
        let text = js_sys::JSON::stringify(&payload).ok().and_then(|s| s.as_string()).unwrap_or_else(|| "null".into());
        match serde_json::from_str::<T>(&text) {
            Ok(v) => f(v),
            Err(err) => web_sys::console::warn_1(&format!("{event}: {err}").into()),
        }
    });
    spawn_local(async move {
        let _ = listen_js(event, &cb).await;
        cb.forget();
    });
}
