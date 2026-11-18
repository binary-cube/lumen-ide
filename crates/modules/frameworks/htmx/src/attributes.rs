//! What htmx's documentation data doesn't hold: the value syntax of the attributes (for
//! hover) and their keywords (for completion), per major version. Written for the editor.

const SWAP: &[&str] = &[
    "innerHTML",
    "outerHTML",
    "textContent",
    "beforebegin",
    "afterbegin",
    "beforeend",
    "afterend",
    "delete",
    "none",
    "transition:true",
    "swap:",
    "settle:",
    "ignoreTitle:true",
    "scroll:top",
    "scroll:bottom",
    "show:top",
    "show:bottom",
    "show:none",
    "focus-scroll:true",
];
/// Swap styles added in htmx 4.
const SWAP_4: &[&str] = &["innerMorph", "outerMorph", "outerSync"];
const TRIGGER: &[&str] = &[
    "click",
    "change",
    "submit",
    "input",
    "keyup",
    "keydown",
    "load",
    "revealed",
    "intersect",
    "every",
    "once",
    "changed",
    "delay:",
    "throttle:",
    "from:",
    "target:",
    "consume",
    "queue:first",
    "queue:last",
    "queue:all",
    "queue:none",
    "root:",
    "threshold:",
];
const TARGET: &[&str] =
    &["this", "closest", "find", "next", "previous", "body", "document", "window"];
const BOOL: &[&str] = &["true", "false"];
const METHODS: &[&str] = &["get", "post", "put", "patch", "delete"];

/// The value's syntax of attribute `name` in htmx `major` (`""`: unknown or no value).
pub fn syntax(name: &str, major: u32) -> &'static str {
    match name {
        "hx-get" | "hx-post" | "hx-put" | "hx-patch" | "hx-delete" | "hx-query" | "hx-action" => {
            "<url>"
        }
        "hx-method" => "get | post | put | patch | delete",
        "hx-trigger" => {
            "<event> [once] [changed] [delay:<time>] [throttle:<time>] [from:<selector>] [target:<selector>] [consume] [queue:first|last|all|none] | every <time> | load | revealed | intersect, …"
        }
        "hx-target" | "hx-include" | "hx-disabled-elt" | "hx-indicator" => {
            "this | closest <selector> | find <selector> | next [<selector>] | previous [<selector>] | <selector>"
        }
        "hx-swap" if major >= 4 => {
            "innerHTML | outerHTML | textContent | beforebegin | afterbegin | beforeend | afterend | innerMorph | outerMorph | outerSync | delete | none [transition:true] [swap:<time>] [settle:<time>] [scroll:top|bottom] [show:top|bottom]"
        }
        "hx-swap" => {
            "innerHTML | outerHTML | textContent | beforebegin | afterbegin | beforeend | afterend | delete | none [transition:true] [swap:<time>] [settle:<time>] [scroll:top|bottom] [show:top|bottom]"
        }
        "hx-swap-oob" => "true | <swap>[:<selector>]",
        "hx-select" => "<selector>",
        "hx-select-oob" => "<selector>[:<swap>], …",
        "hx-boost" => "true | false",
        "hx-push-url" | "hx-replace-url" => "true | false | <url>",
        "hx-on" => "<script>   (hx-on:<event>, hx-on::<htmx-event>)",
        "hx-vals" | "hx-headers" => "{\"name\": \"value\"} | js:{…}",
        "hx-confirm" | "hx-prompt" => "<message>",
        "hx-params" => "* | none | not <param>, … | <param>, …",
        "hx-sync" => {
            "<selector>[:drop | :abort | :replace | :queue first | :queue last | :queue all]"
        }
        "hx-encoding" => "multipart/form-data",
        "hx-ext" => "<extension>, … | ignore:<extension>",
        _ => "",
    }
}

/// The keywords suggested in the value of attribute `name` in htmx `major`.
pub fn values(name: &str, major: u32) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = match name {
        "hx-swap" | "hx-swap-oob" => SWAP.to_vec(),
        "hx-trigger" => TRIGGER.to_vec(),
        "hx-target" | "hx-include" | "hx-disabled-elt" | "hx-indicator" => TARGET.to_vec(),
        "hx-boost" | "hx-push-url" | "hx-replace-url" => BOOL.to_vec(),
        "hx-method" => METHODS.to_vec(),
        "hx-params" => vec!["*", "none", "not"],
        "hx-sync" => vec!["this", "closest", "drop", "abort", "replace", "queue"],
        "hx-encoding" => vec!["multipart/form-data"],
        "hx-vals" | "hx-headers" => vec!["js:"],
        _ => Vec::new(),
    };
    if major >= 4 && name.starts_with("hx-swap") {
        out.extend_from_slice(SWAP_4);
    }
    out
}

/// Attributes written with a suffix (`hx-on:click`, `hx-status:404`): completion inserts
/// the name and `:`.
pub fn takes_suffix(name: &str) -> bool {
    name.ends_with(":*") || matches!(name, "hx-on" | "hx-status")
}

/// Attributes that take no value (their presence is enough).
pub fn bare(name: &str) -> bool {
    matches!(name, "hx-history-elt" | "hx-ignore" | "hx-disable" | "hx-preserve")
}
