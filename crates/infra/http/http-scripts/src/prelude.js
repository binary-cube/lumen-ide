// The HTTP client API for a request's script, in plain JavaScript over what the app
// gives (`__in`): nothing native, nothing reaching out. What the script leaves is read back
// from `__out` (as JSON) when it ends.
const __out = {
  globals: Object.assign({}, __in.globals),
  variables: {},
  logs: [],
  tests: [],
  headers: {},
};

// Bytes as hexadecimal text: what the native hashes take and give.
function __utf8Hex(text) {
  let out = "";
  const s = String(text);
  for (let i = 0; i < s.length; i++) {
    let c = s.codePointAt(i);
    if (c > 0xffff) i++;
    const bytes =
      c < 0x80 ? [c]
      : c < 0x800 ? [0xc0 | (c >> 6), 0x80 | (c & 63)]
      : c < 0x10000 ? [0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63)]
      : [0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63)];
    for (const b of bytes) out += (b < 16 ? "0" : "") + b.toString(16);
  }
  return out;
}
const __B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
function __hexBase64(hex, urlSafe) {
  let bits = "";
  for (let i = 0; i < hex.length; i += 2) bits += parseInt(hex.substr(i, 2), 16).toString(2).padStart(8, "0");
  let out = "";
  for (let i = 0; i < bits.length; i += 6) out += __B64[parseInt(bits.substr(i, 6).padEnd(6, "0"), 2)];
  while (out.length % 4) out += "=";
  return urlSafe ? out.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "") : out;
}
function __base64Hex(text) {
  const s = String(text).replace(/-/g, "+").replace(/_/g, "/").replace(/=+$/, "");
  let bits = "";
  for (const ch of s) bits += __B64.indexOf(ch).toString(2).padStart(6, "0");
  let out = "";
  for (let i = 0; i + 8 <= bits.length; i += 8) out += parseInt(bits.substr(i, 8), 2).toString(16).padStart(2, "0");
  return out;
}
function __hexText(hex) {
  let bytes = [];
  for (let i = 0; i < hex.length; i += 2) bytes.push(parseInt(hex.substr(i, 2), 16));
  return decodeURIComponent(bytes.map((b) => "%" + b.toString(16).padStart(2, "0")).join(""));
}

// `crypto`: hashes and HMACs fed text, hexadecimal or Base64, read as either.
function __hasher(algorithm, keyHex) {
  let data = "";
  const self = {
    updateWithText(text) { data += __utf8Hex(text); return self; },
    updateWithHex(hex) { data += String(hex).toLowerCase(); return self; },
    updateWithBase64(text) { data += __base64Hex(text); return self; },
    digest() {
      const hex = keyHex === undefined ? __digest(algorithm, data) : __hmac(algorithm, keyHex, data);
      return { toHex: () => hex, toBase64: (urlSafe) => __hexBase64(hex, urlSafe) };
    },
  };
  return self;
}
function __keyed(algorithm) {
  return {
    withTextSecret: (secret) => __hasher(algorithm, __utf8Hex(secret)),
    withHexSecret: (hex) => __hasher(algorithm, String(hex).toLowerCase()),
    withBase64Secret: (text) => __hasher(algorithm, __base64Hex(text)),
  };
}
const crypto = {
  md5: () => __hasher("md5"),
  sha1: () => __hasher("sha1"),
  sha256: () => __hasher("sha256"),
  sha384: () => __hasher("sha384"),
  sha512: () => __hasher("sha512"),
  hmac: {
    md5: () => __keyed("md5"),
    sha1: () => __keyed("sha1"),
    sha256: () => __keyed("sha256"),
    sha384: () => __keyed("sha384"),
    sha512: () => __keyed("sha512"),
    sha3: (bits) => __keyed("sha3_" + String(bits || 256)),
  },
};

// JSON Web Tokens: signed with a shared secret (HS256 / HS384 / HS512) or a key in PEM
// (RS256/384/512, PS256/384/512, ES256/384; signed natively).
const __JWT_HMAC = { HS256: "sha256", HS384: "sha384", HS512: "sha512" };
function __pemHex(key, algorithm) {
  const text = String(key);
  if (!text.includes("-----BEGIN")) throw new Error(algorithm + " needs a key in PEM");
  return __base64Hex(text.replace(/-----[^-]+-----/g, "").replace(/\s+/g, ""));
}
function __answer(text) {
  if (text.startsWith("error:")) throw new Error(text.slice(6));
  return text.slice(3);
}
const jwt = {
  sign(payload, key, options) {
    const algorithm = (options && options.algorithm) || "HS256";
    const bits = __JWT_HMAC[algorithm];
    const claims = Object.assign({}, payload);
    if (options) {
      if (options.issuer) claims.iss = options.issuer;
      if (options.audience) claims.aud = options.audience;
      if (options.subject) claims.sub = options.subject;
      if (options.expiresIn) claims.exp = Math.floor(Date.now() / 1000) + Number(options.expiresIn);
    }
    const part = (o) => __hexBase64(__utf8Hex(JSON.stringify(o)), true);
    const head = { alg: algorithm, typ: "JWT" };
    if (options && options.keyid) head.kid = options.keyid;
    const body = part(head) + "." + part(claims);
    const signed = bits
      ? __hmac(bits, __utf8Hex(key), __utf8Hex(body))
      : __answer(__jwtSign(algorithm, __pemHex(key, algorithm), __utf8Hex(body)));
    return body + "." + __hexBase64(signed, true);
  },
  decode(token) {
    const [head, body] = String(token).split(".");
    return { header: JSON.parse(__hexText(__base64Hex(head))), payload: JSON.parse(__hexText(__base64Hex(body))) };
  },
  verify(token, key, options) {
    const [head, body, signature] = String(token).split(".");
    const { header, payload } = jwt.decode(token);
    const allowed = options && options.algorithms;
    if (allowed && !allowed.includes(header.alg)) throw new Error(header.alg + " isn't allowed");
    const bits = __JWT_HMAC[header.alg];
    const pem = String(key).includes("-----BEGIN");
    // A key in PEM never checks a shared-secret token: its public key would be the secret.
    if (bits && pem) throw new Error(header.alg + " needs a shared secret, not a key");
    const signed = __utf8Hex(head + "." + body);
    const valid = bits
      ? __hexBase64(__hmac(bits, __utf8Hex(key), signed), true) === signature
      : __answer(__jwtVerify(header.alg, __pemHex(key, header.alg), signed, __base64Hex(signature))) === "true";
    if (!valid) throw new Error("invalid signature");
    if (options && options.audience && payload.aud !== options.audience) throw new Error("audience mismatch");
    if (options && options.issuer && payload.iss !== options.issuer) throw new Error("issuer mismatch");
    if (payload.exp && payload.exp < Date.now() / 1000) throw new Error("expired");
    return payload;
  },
};

// Waiting: `await sleep(ms)`, and timers run in the order they're due.
const __timers = [];
let __timerId = 0;
let __clock = 0;
function sleep(ms) {
  __sleep(Number(ms) || 0);
  return Promise.resolve();
}
function setTimeout(fn, ms) {
  const id = ++__timerId;
  __timers.push({ id, due: __clock + (Number(ms) || 0), fn });
  return id;
}
function clearTimeout(id) {
  const at = __timers.findIndex((t) => t.id === id);
  if (at >= 0) __timers.splice(at, 1);
}
function __runTimer() {
  if (!__timers.length) return false;
  __timers.sort((a, b) => a.due - b.due);
  const next = __timers.shift();
  __sleep(Math.max(0, next.due - __clock));
  __clock = next.due;
  try {
    next.fn();
  } catch (e) {
    if (!(e && e.__exit)) __out.error = e && e.message ? e.message : String(e);
  }
  return true;
}

function __text(value) {
  if (value === undefined || value === null) return "";
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value);
  } catch (e) {
    return String(value);
  }
}

function __headers(list) {
  return {
    valueOf(name) {
      const found = list.find((h) => h.name.toLowerCase() === String(name).toLowerCase());
      return found ? found.value : null;
    },
    valuesOf(name) {
      return list
        .filter((h) => h.name.toLowerCase() === String(name).toLowerCase())
        .map((h) => h.value);
    },
    findByName(name) {
      const found = list.find((h) => h.name.toLowerCase() === String(name).toLowerCase());
      return found
        ? { name: found.name, getRawValue: () => found.value, tryGetSubstitutedValue: () => found.value }
        : null;
    },
    all() {
      return list.map((h) => ({ name: h.name, value: h.value }));
    },
  };
}

const client = {
  global: {
    set(name, value) {
      __out.globals[String(name)] = __text(value);
    },
    get(name) {
      const value = __out.globals[String(name)];
      return value === undefined ? null : value;
    },
    isEmpty() {
      return Object.keys(__out.globals).length === 0;
    },
    clear(name) {
      delete __out.globals[String(name)];
    },
    clearAll() {
      for (const key of Object.keys(__out.globals)) delete __out.globals[key];
    },
    // Headers sent with every request after it (in a pre-request script: this one too).
    headers: {
      set(name, value) {
        __out.headers[String(name)] = __text(value);
      },
      clear(name) {
        __out.headers[String(name)] = null;
      },
    },
  },
  log(...values) {
    __out.logs.push(values.map(__text).join(" "));
  },
  test(name, run) {
    try {
      run();
      __out.tests.push({ name: String(name), failure: null });
    } catch (e) {
      __out.tests.push({ name: String(name), failure: e && e.message ? e.message : String(e) });
    }
  },
  assert(condition, message) {
    if (!condition) throw new Error(message === undefined ? "Assertion failed" : String(message));
  },
  exit() {
    throw { __exit: true };
  },
};

const request = {
  method: __in.request.method,
  url: {
    getRaw: () => __in.request.url,
    tryGetSubstituted: () => __in.request.url,
  },
  body: {
    getRaw: () => __in.request.body,
    tryGetSubstituted: () => __in.request.body,
  },
  headers: __headers(__in.request.headers),
  environment: {
    get(name) {
      const value = __in.environment[String(name)];
      return value === undefined ? null : value;
    },
  },
  variables: {
    set(name, value) {
      __out.variables[String(name)] = __text(value);
    },
    get(name) {
      const value = __out.variables[String(name)];
      return value === undefined ? null : value;
    },
  },
  iteration: () => __in.iteration || 0,
  templateValue: (n) => (__in.templates ? __in.templates[n] : null),
};

// Every scope's variables, one object (`client.variables`).
client.variables = {
  global: client.global,
  environment: request.environment,
  file: {
    get(name) {
      const value = __in.file[String(name)];
      return value === undefined ? null : value;
    },
  },
  request: request.variables,
  get(name) {
    for (const scope of [request.variables, client.global, request.environment, client.variables.file]) {
      const value = scope.get(name);
      if (value !== null) return value;
    }
    return null;
  },
};

const response = __in.response
  ? {
      status: __in.response.status,
      headers: __headers(__in.response.headers),
      // A JSON body given parsed (its text once, parsed here).
      body: (function () {
        const type = __in.response.content_type || "";
        // A stream (server-sent events, NDJSON): its lines and its messages, each told.
        if (type.indexOf("event-stream") >= 0 || type.indexOf("ndjson") >= 0 || type.indexOf("stream+json") >= 0) {
          const text = __in.response.body;
          const lines = text.split(/\r?\n/);
          const parse = (m) => {
            try {
              return JSON.parse(m);
            } catch (e) {
              return m;
            }
          };
          const messages = () => {
            if (type.indexOf("event-stream") < 0) return lines.filter((l) => l.trim()).map(parse);
            const out = [];
            let data = [];
            for (const line of lines.concat([""])) {
              if (line === "") {
                if (data.length) out.push(parse(data.join("\n")));
                data = [];
              } else if (line.startsWith("data:")) {
                data.push(line.slice(5).replace(/^ /, ""));
              }
            }
            return out;
          };
          const each = (items, subscriber, onFinish) => {
            let stopped = false;
            for (const item of items) {
              if (stopped) break;
              subscriber(item, () => (stopped = true), () => {});
            }
            if (onFinish) onFinish();
          };
          return {
            text,
            onEachLine: (subscriber, onFinish) => each(lines.filter((l) => l !== ""), subscriber, onFinish),
            onEachMessage: (subscriber, onFinish) => each(messages(), subscriber, onFinish),
            toString: () => text,
          };
        }
        if (type.indexOf("json") < 0) return __in.response.body;
        try {
          return JSON.parse(__in.response.body);
        } catch (e) {
          return __in.response.body;
        }
      })(),
      contentType: {
        mimeType: __in.response.content_type,
        charset: __in.response.charset,
      },
      // The cookies the server set (`Set-Cookie`), each with its attributes.
      cookies() {
        return __in.response.headers
          .filter((h) => h.name.toLowerCase() === "set-cookie")
          .map((h) => {
            const parts = h.value.split(";").map((p) => p.trim());
            const [name, ...rest] = parts[0].split("=");
            const cookie = { name: name, value: rest.join("=") };
            for (const attribute of parts.slice(1)) {
              const [key, ...value] = attribute.split("=");
              cookie[key.toLowerCase()] = value.length ? value.join("=") : true;
            }
            return cookie;
          });
      },
      cookiesByName(name) {
        return this.cookies().filter((c) => c.name === name);
      },
    }
  : undefined;
