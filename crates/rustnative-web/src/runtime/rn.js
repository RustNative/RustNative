// Rust Native's browser runtime (PLAN.md Web milestones A, D, F, G, H).
//
// One fixed module the framework ships, never generated. It has four parts:
//
//   1. the client subset's semantics: integers that overflow as Rust's do,
//      Rust's float and string formatting, UTF-8 lengths, code-point order;
//   2. the realizer: a node as generated client code builds it, turned into
//      the element the server would have written — the same classes, the
//      same attributes, in the same order (a mirror of rustnative_web::dom
//      and ::css, held equal by crates/rustnative-web/tests/runtime.rs);
//   3. the DOM: creating and patching elements keyed by framework key,
//      preserving focus, selection, and scroll;
//   4. islands: attaching generated modules to server-rendered markup,
//      delivering browser events as framework events, batching renders into
//      one per microtask, and carrying out effects.
//
// Nothing here runs at import: the page's bootstrap calls `start()`.

"use strict";

// ---------------------------------------------------------------- hash --

export function cyrb53(text) {
  let h1 = 0xdeadbeef, h2 = 0x41c6ce57;
  for (let i = 0; i < text.length; i++) {
    const ch = text.charCodeAt(i);
    h1 = Math.imul(h1 ^ ch, 2654435761);
    h2 = Math.imul(h2 ^ ch, 1597334677);
  }
  h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507);
  h1 ^= Math.imul(h2 ^ (h2 >>> 13), 3266489909);
  h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507);
  h2 ^= Math.imul(h1 ^ (h1 >>> 13), 3266489909);
  return 4294967296 * (2097151 & h2) + (h1 >>> 0);
}

export function className(prefix, text) {
  return prefix + cyrb53(text).toString(36);
}

// ------------------------------------------------------- integers ----

export const MAX_SAFE = 9007199254740991;

/// A panic in client logic: the error Rust raises for the same operation.
export class Panic extends Error {
  constructor(message) { super(message); this.name = "Panic"; }
}

export function panic(message) { throw new Panic(message); }

const RANGES = {
  i8: [-128, 127], i16: [-32768, 32767], i32: [-2147483648, 2147483647],
  u8: [0, 255], u16: [0, 65535], u32: [0, 4294967295],
  i64: [-MAX_SAFE, MAX_SAFE], isize: [-MAX_SAFE, MAX_SAFE],
  u64: [0, MAX_SAFE], usize: [0, MAX_SAFE],
};
const BITS = { i8: 8, i16: 16, i32: 32, i64: 64, isize: 64, u8: 8, u16: 16, u32: 32, u64: 64, usize: 64 };
const SIGNED = { i8: true, i16: true, i32: true, i64: true, isize: true };

function inRange(value, kind, operation) {
  const range = RANGES[kind];
  if (!(value >= range[0] && value <= range[1])) panic(`attempt to ${operation} with overflow`);
  return value + 0; // never -0
}

export function add(a, b, kind) { return inRange(a + b, kind, "add"); }
export function sub(a, b, kind) { return inRange(a - b, kind, "subtract"); }
export function mul(a, b, kind) {
  const product = a * b;
  if (Math.abs(product) > MAX_SAFE) panic("attempt to multiply with overflow");
  return inRange(product, kind, "multiply");
}
export function div(a, b, kind) {
  if (b === 0) panic("attempt to divide by zero");
  if (Math.abs(a) > 2147483647 || Math.abs(b) > 2147483647) {
    return inRange(Number(BigInt(a) / BigInt(b)), kind, "divide");
  }
  return inRange(Math.trunc(a / b), kind, "divide");
}
export function rem(a, b, kind) {
  if (b === 0) panic("attempt to calculate the remainder with a divisor of zero");
  if (b === -1 && a === RANGES[kind][0] && SIGNED[kind]) panic("attempt to calculate the remainder with overflow");
  return (a % b) + 0;
}
export function neg(a, kind) { return inRange(-a, kind, "negate"); }

/// `value as kind` for an integer target: Rust's wrapping cast for narrow
/// types; for 64-bit types, a result beyond ±(2^53 − 1) is an overflow.
export function castInt(value, kind) {
  if (typeof value === "boolean") return value ? 1 : 0;
  if (typeof value === "string") value = value.codePointAt(0);
  if (!Number.isInteger(value)) {
    // From a float: saturating, NaN is zero.
    if (Number.isNaN(value)) return 0;
    const range = RANGES[kind];
    const truncated = Math.trunc(value);
    if (BITS[kind] === 64) {
      if (truncated > range[1] || truncated < range[0]) panic("attempt to cast with overflow");
      return truncated + 0;
    }
    return Math.min(range[1], Math.max(range[0], truncated)) + 0;
  }
  const bits = BITS[kind];
  const wrapped = SIGNED[kind] ? BigInt.asIntN(bits, BigInt(value)) : BigInt.asUintN(bits, BigInt(value));
  const result = Number(wrapped);
  if (bits === 64 && Math.abs(result) > MAX_SAFE) panic("attempt to cast with overflow");
  return result + 0;
}

export function castFloat(value, kind) {
  if (typeof value === "boolean") value = value ? 1 : 0;
  return kind === "f32" ? Math.fround(value) : value;
}

export function charFromU8(value) { return String.fromCodePoint(value); }

// ---------------------------------------------------- formatting ----

/// A float as Rust's `Display` writes it: the shortest digits that read
/// back to the same value, never in exponent form.
export function float(value, kind) {
  if (Number.isNaN(value)) return "NaN";
  if (value === Infinity) return "inf";
  if (value === -Infinity) return "-inf";
  if (value === 0) return Object.is(value, -0) ? "-0" : "0";
  let text;
  if (kind === "f32") {
    for (let precision = 1; precision <= 9; precision++) {
      text = value.toPrecision(precision);
      if (Math.fround(Number(text)) === value) break;
    }
    text = String(Number(text));
  } else {
    text = String(value);
  }
  return positional(text);
}

function positional(text) {
  const match = /^(-?)(\d+)(?:\.(\d+))?e([+-]\d+)$/.exec(text);
  if (!match) return text;
  const [, sign, whole, fraction = "", exponentText] = match;
  const exponent = Number(exponentText);
  const digits = whole + fraction;
  const point = whole.length + exponent;
  let out;
  if (point <= 0) out = "0." + "0".repeat(-point) + digits;
  else if (point >= digits.length) out = digits + "0".repeat(point - digits.length);
  else out = digits.slice(0, point) + "." + digits.slice(point);
  out = out.replace(/^0+(?=\d)/, "");
  if (out.includes(".")) out = out.replace(/\.?0+$/, "");
  return sign + out;
}

/// A float with `precision` decimals, rounded half to even on its exact
/// binary value, as Rust's `{:.N}` does.
export function fixed(value, precision) {
  if (Number.isNaN(value)) return "NaN";
  if (!Number.isFinite(value)) return value > 0 ? "inf" : "-inf";
  const negative = value < 0 || Object.is(value, -0);
  const view = new DataView(new ArrayBuffer(8));
  view.setFloat64(0, Math.abs(value));
  const bits = view.getBigUint64(0);
  const exponentBits = Number((bits >> 52n) & 0x7ffn);
  let mantissa = bits & 0xfffffffffffffn;
  let exponent;
  if (exponentBits === 0) { exponent = -1074; } else { mantissa |= 1n << 52n; exponent = exponentBits - 1075; }
  // value = mantissa * 2^exponent; scaled = value * 10^precision.
  let numerator = mantissa * 10n ** BigInt(precision);
  let denominator = 1n;
  if (exponent >= 0) numerator <<= BigInt(exponent); else denominator <<= BigInt(-exponent);
  let quotient = numerator / denominator;
  const remainder = numerator % denominator;
  const twice = remainder * 2n;
  if (twice > denominator || (twice === denominator && (quotient & 1n) === 1n)) quotient += 1n;
  let digits = quotient.toString();
  if (precision > 0) {
    digits = digits.padStart(precision + 1, "0");
    digits = digits.slice(0, digits.length - precision) + "." + digits.slice(digits.length - precision);
  }
  return (negative ? "-" : "") + digits;
}

/// A value as `{}` writes it, given its static kind (`"f64"`, `"f32"`,
/// `"int"`, `"str"`, `"char"`, `"bool"`, or `"any"`).
export function display(value, kind) {
  if (kind === "f64" || kind === "f32") return float(value, kind);
  if (typeof value === "number") return Number.isInteger(value) ? String(value + 0) : float(value, "f64");
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "string") return value;
  if (value && typeof value === "object" && value.__display) return value.__display();
  return String(value);
}

const ESCAPES = { '"': '\\"', "\\": "\\\\", "\n": "\\n", "\r": "\\r", "\t": "\\t", "\0": "\\0" };

/// A string as `{:?}` writes it.
// Rust's `is_printable` is false for control, format, surrogate,
// private-use, and unassigned characters, and separators other than the
// space; `escape_debug` also escapes a leading grapheme extender.
const UNPRINTABLE = /^(?:[\p{Cc}\p{Cf}\p{Cs}\p{Co}\p{Cn}\p{Zl}\p{Zp}]|(?! )\p{Zs})$/u;

export function debugString(text, quote) {
  let out = quote;
  let first = true;
  for (const ch of text) {
    const code = ch.codePointAt(0);
    if (ch === quote || (ch in ESCAPES && ch !== (quote === '"' ? "'" : '"'))) out += ESCAPES[ch] ?? "\\" + ch;
    else if (UNPRINTABLE.test(ch) || (first && /^\p{Grapheme_Extend}$/u.test(ch))) out += "\\u{" + code.toString(16) + "}";
    else out += ch;
    first = false;
  }
  return out + quote;
}

/// Applies a format spec: `{ fill, align, sign, width, zero, radix, upper }`.
export function pad(text, spec, numeric) {
  if (spec.radix) {
    const value = BigInt(text);
    const bits = spec.bits || 64;
    const unsigned = value < 0n ? BigInt.asUintN(bits, value) : value;
    text = unsigned.toString(spec.radix);
    if (spec.upper) text = text.toUpperCase();
    if (spec.alternate) text = { 16: "0x", 2: "0b", 8: "0o" }[spec.radix] + text;
  }
  if (spec.sign && numeric && !text.startsWith("-")) text = "+" + text;
  const width = spec.width || 0;
  const length = [...text].length;
  if (length >= width) return text;
  const missing = width - length;
  if (spec.zero && numeric) {
    const sign = /^[+-]/.test(text) ? text[0] : "";
    return sign + "0".repeat(missing) + text.slice(sign.length);
  }
  const fill = spec.fill ?? " ";
  const align = spec.align ?? (numeric ? ">" : "<");
  if (align === "<") return text + fill.repeat(missing);
  if (align === ">") return fill.repeat(missing) + text;
  const left = Math.floor(missing / 2);
  return fill.repeat(left) + text + fill.repeat(missing - left);
}

// --------------------------------------------------------- strings ----

export function utf8len(text) {
  let length = 0;
  for (const ch of text) {
    const code = ch.codePointAt(0);
    length += code < 0x80 ? 1 : code < 0x800 ? 2 : code < 0x10000 ? 3 : 4;
  }
  return length;
}

export function charCount(text) { return [...text].length; }

const WHITE = /^[\p{White_Space}]+|[\p{White_Space}]+$/gu;
export function trim(text) { return text.replace(WHITE, ""); }
export function trimStart(text) { return text.replace(/^[\p{White_Space}]+/u, ""); }
export function trimEnd(text) { return text.replace(/[\p{White_Space}]+$/u, ""); }
export function splitWhitespace(text) { return text.split(/[\p{White_Space}]+/u).filter((part) => part !== ""); }
export function lines(text) {
  if (text === "") return [];
  const parts = text.split("\n").map((line) => (line.endsWith("\r") ? line.slice(0, -1) : line));
  if (text.endsWith("\n")) parts.pop();
  return parts;
}
export function split(text, separator) { return text.split(separator); }
export function replaceAll(text, from, to) { return text.split(from).join(to); }
export function isWhitespace(ch) { return /^\p{White_Space}$/u.test(ch); }
export function isAlphabetic(ch) { return /^\p{Alphabetic}$/u.test(ch); }
export function isNumeric(ch) { return /^\p{N}$/u.test(ch); }
export function isAlphanumeric(ch) { return isAlphabetic(ch) || isNumeric(ch); }

/// Orders two strings by code point, as Rust does (JavaScript's `<`
/// compares UTF-16 units, which differs for characters past U+FFFF).
export function compareStrings(a, b) {
  const left = [...a], right = [...b];
  for (let i = 0; i < Math.min(left.length, right.length); i++) {
    const difference = left[i].codePointAt(0) - right[i].codePointAt(0);
    if (difference !== 0) return difference < 0 ? -1 : 1;
  }
  return left.length === right.length ? 0 : left.length < right.length ? -1 : 1;
}

/// `a.cmp(b)` for numbers, strings, booleans, and arrays of them.
export function cmp(a, b) {
  if (typeof a === "string") return compareStrings(a, b);
  if (Array.isArray(a)) {
    for (let i = 0; i < Math.min(a.length, b.length); i++) {
      const order = cmp(a[i], b[i]);
      if (order !== 0) return order;
    }
    return a.length === b.length ? 0 : a.length < b.length ? -1 : 1;
  }
  if (a === null || b === null) return a === b ? 0 : a === null ? -1 : 1;
  return a < b ? -1 : a > b ? 1 : 0;
}

export function parseInt_(text, kind) {
  if (text === "") return { Err: { parse: "cannot parse integer from empty string" } };
  const signed = SIGNED[kind];
  if (!(signed ? /^[+-]?\d+$/ : /^\+?\d+$/).test(text)) {
    if (!signed && /^-\d+$/.test(text)) return { Err: { parse: "invalid digit found in string" } };
    return { Err: { parse: /^[+-]$/.test(text) ? "invalid digit found in string" : "invalid digit found in string" } };
  }
  const value = BigInt(text);
  const range = RANGES[kind];
  if (value > BigInt(range[1])) return { Err: { parse: "number too large to fit in target type" } };
  if (value < BigInt(range[0])) return { Err: { parse: "number too small to fit in target type" } };
  return { Ok: Number(value) };
}

export function parseFloat_(text, kind) {
  if (text === "") return { Err: { parse: "cannot parse float from empty string" } };
  const match = /^[+-]?(?:(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?|inf|infinity|nan)$/i.test(text);
  if (!match) return { Err: { parse: "invalid float literal" } };
  const lower = text.toLowerCase().replace(/^\+/, "");
  let value;
  if (/^-?(inf|infinity)$/.test(lower)) value = lower.startsWith("-") ? -Infinity : Infinity;
  else if (/^-?nan$/.test(lower)) value = NaN;
  else value = Number(text);
  return { Ok: kind === "f32" ? Math.fround(value) : value };
}

export function parseBool(text) {
  if (text === "true") return { Ok: true };
  if (text === "false") return { Ok: false };
  return { Err: { parse: "provided string was not `true` or `false`" } };
}

/// The text of an error value (`to_string()` of a parse or server error).
export function errorText(error) {
  if (error && typeof error === "object") {
    if ("parse" in error) return error.parse;
    if ("Transport" in error) return `the call did not complete: ${error.Transport}`;
    if ("Server" in error) return `${error.Server.status}: ${error.Server.message}`;
    if ("Decode" in error) return `unexpected answer: ${error.Decode}`;
  }
  return display(error, "any");
}

// ------------------------------------------------ values and data ----

export function clone(value) {
  if (value === null || typeof value !== "object") return value;
  if (typeof structuredClone === "function") return structuredClone(value);
  return JSON.parse(JSON.stringify(value));
}

export function eq(a, b) {
  if (a === b) return true;
  if (typeof a === "number" && typeof b === "number") return a === b;
  if (a === null || b === null || typeof a !== "object" || typeof b !== "object") return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a)) return a.length === b.length && a.every((item, i) => eq(item, b[i]));
  const keys = Object.keys(a);
  if (keys.length !== Object.keys(b).length) return false;
  return keys.every((key) => Object.prototype.hasOwnProperty.call(b, key) && eq(a[key], b[key]));
}

/// `v[i]`, with Rust's bounds check.
export function at(array, index) {
  if (index < 0 || index >= array.length) {
    panic(`index out of bounds: the len is ${array.length} but the index is ${index}`);
  }
  return array[index];
}

export function unwrap(option, what) {
  if (option === null || option === undefined) panic(what ?? "called `Option::unwrap()` on a `None` value");
  return option;
}

export function unwrapOk(result, what) {
  if (!("Ok" in result)) panic(what ?? `called \`Result::unwrap()\` on an \`Err\` value: ${errorText(result.Err)}`);
  return result.Ok;
}

export function range(start, end) {
  const out = [];
  for (let i = start; i < end; i++) out.push(i);
  return out;
}

export function sum(values, kind) {
  let total = 0;
  for (const value of values) total = kind ? add(total, value, kind) : total + value;
  return total;
}

export function vecRemove(array, index) {
  if (index >= array.length) panic(`removal index (is ${index}) should be < len (is ${array.length})`);
  return array.splice(index, 1)[0];
}

export function vecInsert(array, index, value) {
  if (index > array.length) panic(`insertion index (is ${index}) should be <= len (is ${array.length})`);
  array.splice(index, 0, value);
}

export function retain(array, keep) {
  let write = 0;
  for (let read = 0; read < array.length; read++) {
    if (keep(array[read])) array[write++] = array[read];
  }
  array.length = write;
}

export function sortBy(array, compare) {
  array.sort(compare);
}

export function minMax(values, pick) {
  let best = null;
  for (const value of values) {
    if (best === null || (pick > 0 ? cmp(value, best) >= 0 : cmp(value, best) < 0)) best = value;
  }
  return best;
}

// ------------------------------------------ generated-code helpers ----

/// A variant's name: a unit variant is a string, one with data an object
/// with one key.
export function tag(value) {
  if (typeof value === "string") return value;
  if (value && typeof value === "object") return Object.keys(value)[0];
  return null;
}

export function idx(array, index) {
  if (index < 0 || index >= array.length) panic(`index out of bounds: the len is ${array.length} but the index is ${index}`);
  return index;
}

export function w(object, field, value) { object[field] = value; return object; }
export function push(object, field, value) { object[field] = [...(object[field] || []), value]; return object; }
export function or(option, fallback) { return option === null || option === undefined ? fallback : option; }
export function orElse(option, make) { return option === null || option === undefined ? make() : option; }

export function round(value) { return value < 0 ? -Math.round(-value) : Math.round(value); }
export function signum(value) { return Number.isNaN(value) ? NaN : value > 0 || Object.is(value, 0) ? 1 : -1; }
export function fract(value) { return value - Math.trunc(value); }
export function fmin(a, b) { return Number.isNaN(a) ? b : Number.isNaN(b) ? a : Math.min(a, b); }
export function fmax(a, b) { return Number.isNaN(a) ? b : Number.isNaN(b) ? a : Math.max(a, b); }

export function pow(base, exponent, kind) {
  let result = 1;
  for (let i = 0; i < exponent; i++) result = mul(result, base, kind);
  return result;
}

const OPERATIONS = { add, sub, mul, div, rem };

/// `a.checked_op(b)`: the result, or `null` where Rust's is `None`.
export function checked(operation, a, b, kind) {
  try { return OPERATIONS[operation](a, b, kind); } catch (error) { if (error instanceof Panic) return null; throw error; }
}

export function saturating(operation, a, b, kind) {
  const range = RANGES[kind];
  const exact = operation === "add" ? a + b : operation === "sub" ? a - b : a * b;
  return Math.min(range[1], Math.max(range[0], exact)) + 0;
}

export function remEuclid(a, b, kind) {
  const r = rem(a, b, kind);
  return r < 0 ? add(r, Math.abs(b), kind) : r;
}

export function tryFrom(value, kind) {
  const range = RANGES[kind];
  return value >= range[0] && value <= range[1] ? { Ok: value } : { Err: { parse: "out of range integral type conversion attempted" } };
}

export function charFromU32(code) {
  return code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff) ? null : String.fromCodePoint(code);
}

export function asciiUpper(text) { return text.replace(/[a-z]/g, (c) => c.toUpperCase()); }
export function asciiLower(text) { return text.replace(/[A-Z]/g, (c) => c.toLowerCase()); }

export function toDigit(ch, radix) {
  if (radix < 2 || radix > 36) panic("to_digit: radix is too high (maximum 36)");
  const value = /^[0-9a-zA-Z]$/.test(ch) ? parseInt(ch, 36) : NaN;
  return Number.isNaN(value) || value >= radix ? null : value;
}

export function zip(a, b) {
  const out = [];
  for (let i = 0; i < Math.min(a.length, b.length); i++) out.push([a[i], b[i]]);
  return out;
}

export function position(array, test) {
  const index = array.findIndex(test);
  return index < 0 ? null : index;
}

export function lastOf(array) { return array.length ? array[array.length - 1] : null; }

/// `max_by_key` (the last greatest) and `min_by_key` (the first least).
export function byKey(array, key, direction) {
  let best = null, bestKey = null;
  for (const item of array) {
    const itemKey = key(item);
    if (best === null || (direction > 0 ? cmp(itemKey, bestKey) >= 0 : cmp(itemKey, bestKey) < 0)) { best = item; bestKey = itemKey; }
  }
  return best;
}

export function product(values, kind) {
  let total = 1;
  for (const value of values) total = kind ? mul(total, value, kind) : total * value;
  return total;
}

export function dedup(array) {
  let write = 0;
  for (let read = 0; read < array.length; read++) {
    if (write === 0 || !eq(array[read], array[write - 1])) array[write++] = array[read];
  }
  array.length = write;
}

export function swap(array, i, j) {
  idx(array, i); idx(array, j);
  [array[i], array[j]] = [array[j], array[i]];
}

export function swapRemove(array, index) {
  if (index >= array.length) panic(`swap_remove index (is ${index}) should be < len (is ${array.length})`);
  const value = array[index];
  array[index] = array[array.length - 1];
  array.length -= 1;
  return value;
}

export function insets(top, end, bottom, start) { return { top, end, bottom, start }; }

export function constrain(constraints, field, value) {
  const next = { ...constraints };
  value = Math.max(0, value);
  if (field === "min_width" || field === "min_height") {
    next[field] = value;
    const max = field === "min_width" ? "max_width" : "max_height";
    if (next[max] !== null && next[max] < value) next[max] = value;
  } else {
    const min = field === "max_width" ? "min_width" : "min_height";
    next[field] = value > next[min] ? value : next[min];
  }
  return next;
}

export function span(placement, rows, columns) {
  return { ...placement, row_span: Math.max(1, rows), column_span: Math.max(1, columns) };
}

export function a11yOf(role) {
  if (role && typeof role === "object") { const info = a11y("Heading"); info.level = role.Heading; return info; }
  return a11y(role);
}

export function a11yRange(info, min, max, current) {
  const low = Math.min(min, max), high = Math.max(max, min);
  info.value = { range: [Math.fround(min), Math.fround(max), Math.fround(Math.min(Math.max(current, low), high)), 1] };
  return info;
}

export function visual() { return { fg: null, bg: null, border: null, radius: null, font: null, padding: null, shadow: null }; }

export function date(year, month, day) {
  const leap = (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
  const days = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][month - 1];
  return days !== undefined && day >= 1 && day <= days ? { year, month, day } : null;
}

export function dateText(value) {
  const year = value.year < 0 ? "-" + String(-value.year).padStart(4, "0") : String(value.year).padStart(4, "0");
  return `${year}-${String(value.month).padStart(2, "0")}-${String(value.day).padStart(2, "0")}`;
}

// ------------------------------------------- CSS (mirror of css.rs) ----

function roundHalfAway(value) { return value < 0 ? -Math.round(-value) : Math.round(value); }

/// A number with at most three decimals and no trailing zeros.
export function fixed3(value) {
  const milli = roundHalfAway(value * 1000);
  const sign = milli < 0 ? "-" : "";
  const magnitude = Math.abs(milli);
  const whole = Math.floor(magnitude / 1000);
  const fraction = magnitude % 1000;
  if (fraction === 0) return sign + whole;
  return sign + whole + "." + String(fraction).padStart(3, "0").replace(/0+$/, "");
}

export function hex(r, g, b, a = 255) {
  const two = (value) => value.toString(16).padStart(2, "0");
  return "#" + two(r) + two(g) + two(b) + (a === 255 ? "" : two(a));
}

const ALIGN = { Start: "flex-start", Center: "center", End: "flex-end", Stretch: "stretch" };
const OVERFLOW = { Visible: "visible", Clip: "hidden", Scroll: "auto" };

function insetCss(property, value) {
  if (!value || (value.top === 0 && value.end === 0 && value.bottom === 0 && value.start === 0)) return "";
  return `${property}-block:${value.top}px ${value.bottom}px;${property}-inline:${value.start}px ${value.end}px;`;
}

function mode(value) {
  if (value === "Auto" || value === "Fill") return value;
  return { Fixed: value.Fixed };
}

function mainAxis(property, size) {
  if (size === "Auto") return "flex:0 0 auto;";
  if (size === "Fill") return "flex:1 0 auto;";
  return `flex:0 0 auto;${property}:${Math.max(0, size.Fixed)}px;`;
}

function crossAxis(property, size, alignment) {
  if ((alignment === "Stretch" && size === "Auto") || size === "Fill") return "align-self:stretch;";
  if (size === "Auto") return `align-self:${ALIGN[alignment]};`;
  const effective = alignment === "Stretch" ? "Start" : alignment;
  return `${property}:${Math.max(0, size.Fixed)}px;max-${property}:100%;align-self:${ALIGN[effective]};`;
}

/// `css::layout_css`: `flow` is `{t: "column"|"row", a}`, `{t: "grid"}`, or
/// `{t: "root"}`; `container` is `{t: "none"}`, `{t: "linear", ...}`, or
/// `{t: "grid", grid}`.
export function layoutCss(layout, flow, container) {
  let out = "";
  const width = mode(layout.width), height = mode(layout.height);
  if (flow.t === "column") {
    out += mainAxis("height", height);
    out += crossAxis("width", width, layout.align_self ?? flow.a);
  } else if (flow.t === "row") {
    out += mainAxis("width", width);
    out += crossAxis("height", height, layout.align_self ?? flow.a);
  } else if (flow.t === "grid") {
    for (const [property, size, self] of [["width", width, "justify-self"], ["height", height, "align-self"]]) {
      if (size.Fixed !== undefined) out += `${property}:${Math.max(0, size.Fixed)}px;max-${property}:100%;${self}:start;`;
    }
    const placement = layout.grid;
    if (placement) {
      out += `grid-area:${placement.row + 1} / ${placement.column + 1} / span ${Math.max(1, placement.row_span)} / span ${Math.max(1, placement.column_span)};`;
    }
  } else {
    out += height.Fixed !== undefined ? `flex:0 0 auto;height:${Math.max(0, height.Fixed)}px;` : "flex:1 0 auto;";
    out += width.Fixed !== undefined ? `width:${Math.max(0, width.Fixed)}px;align-self:flex-start;` : "align-self:stretch;";
  }
  out += insetCss("margin", layout.margin);
  const constraints = layout.constraints || {};
  if (constraints.min_width > 0) out += `min-width:${constraints.min_width}px;`;
  if (constraints.max_width !== null && constraints.max_width !== undefined) out += `max-width:${constraints.max_width}px;`;
  if (constraints.min_height > 0) out += `min-height:${constraints.min_height}px;`;
  if (constraints.max_height !== null && constraints.max_height !== undefined) out += `max-height:${constraints.max_height}px;`;
  if (container.t === "linear") {
    const style = container.style;
    out += insetCss("padding", style.padding);
    out += `gap:${Math.max(0, style.gap)}px;align-items:${ALIGN[style.align_items]};overflow:${OVERFLOW[style.overflow]};`;
  } else if (container.t === "grid") {
    const grid = container.grid;
    const tracks = (list) => list.map((track) => (track === "Auto" ? "auto" : track.Fixed !== undefined ? `${Math.max(0, track.Fixed)}px` : `${track.Fraction}fr`)).join(" ");
    if (grid.columns.length) out += `grid-template-columns:${tracks(grid.columns)};`;
    if (grid.rows.length) out += `grid-template-rows:${tracks(grid.rows)};`;
    out += insetCss("padding", grid.padding);
    out += `gap:${Math.max(0, grid.gap)}px;`;
  }
  return out;
}

const GENERIC_FAMILIES = new Set(["system-ui", "serif", "sans-serif", "monospace", "cursive", "fantasy", "ui-serif", "ui-sans-serif", "ui-monospace", "ui-rounded"]);

function family(name) {
  if (GENERIC_FAMILIES.has(name) || name.includes(",")) return name;
  return `"${name.replace(/["\\\n]/g, "")}"`;
}

function visualDeclarations(style) {
  if (!style) return "";
  let out = "";
  if (style.fg) out += `color:${style.fg};`;
  if (style.bg) out += `background-color:${style.bg};`;
  if (style.border) out += `border:1px solid ${style.border};`;
  if (style.radius !== null && style.radius !== undefined) out += `border-radius:${style.radius}px;`;
  if (style.font) out += `font-family:${family(style.font.family)};font-size:${style.font.size}px;font-weight:${style.font.weight};`;
  if (style.padding) out += insetCss("padding", style.padding);
  if (style.shadow) out += `box-shadow:${style.shadow};`;
  return out;
}

const CURSORS = {
  Default: "default", Pointer: "pointer", Text: "text", Crosshair: "crosshair", Move: "move", NotAllowed: "not-allowed",
  ResizeVertical: "ns-resize", ResizeHorizontal: "ew-resize", Wait: "wait", Progress: "progress", Help: "help",
};

const STATE_SELECTORS = [["hover", ":hover"], ["focus", ":focus"], ["active", ":active"], ["disabled", ":is(:disabled,[aria-disabled=true])"]];

/// `css::visual_rules`.
export function visualRules(style, states, opacity, cursor) {
  let normal = visualDeclarations(style);
  if (Math.abs(Math.fround(opacity) - 1) > 1.1920929e-7) normal += `opacity:${fixed3(opacity)};`;
  if (cursor) normal += `cursor:${CURSORS[cursor]};`;
  let rules = normal ? `&{${normal}}` : "";
  for (const [state, selector] of STATE_SELECTORS) {
    const declarations = visualDeclarations(states && states[state]);
    if (declarations) rules += `&${selector}{${declarations}}`;
  }
  return rules || null;
}

function implies(condition, other) {
  const same = (a, b) => b === null || b === undefined || a === b;
  if (!same(condition.state, other.state) || !same(condition.scheme, other.scheme) || !same(condition.dir, other.dir)
    || !same(condition.motion, other.motion) || !same(condition.pointer, other.pointer)) return false;
  if (other.min === null || other.min === undefined) return true;
  return condition.min !== null && condition.min !== undefined && condition.min >= other.min;
}

function conditionKey(condition) {
  return JSON.stringify([condition.state, condition.scheme, condition.min, condition.dir, condition.motion, condition.pointer]);
}

function wrap(media, selector, body) {
  return media.length === 0 ? `&${selector}{${body}}` : `@media ${media.join(" and ")}{&${selector}{${body}}}`;
}

function sizeValueOf(size) {
  if (size === "Auto") return { t: "auto" };
  if (size === "Fill") return { t: "fill" };
  return { t: "len", css: `${Math.max(0, size.Fixed)}px` };
}

function contextualBody(flow, width, height, alignSelf) {
  let out = "";
  const main = (property, value) => {
    if (value.t === "len") out += `flex:0 0 auto;${property}:${value.css};`;
    else if (value.t === "auto") out += `flex:0 0 auto;${property}:auto;`;
    else out += `flex:1 0 auto;${property}:auto;`;
  };
  const cross = (property, value, alignment) => {
    if ((alignment === "Stretch" && value.t === "auto") || value.t === "fill") out += `${property}:auto;align-self:stretch;`;
    else if (value.t === "auto") out += `${property}:auto;align-self:${ALIGN[alignment]};`;
    else {
      const effective = alignment === "Stretch" ? "Start" : alignment;
      out += `${property}:${value.css};max-${property}:100%;align-self:${ALIGN[effective]};`;
    }
  };
  if (flow.t === "column") { main("height", height); cross("width", width, alignSelf ?? flow.a); }
  else if (flow.t === "row") { main("width", width); cross("height", height, alignSelf ?? flow.a); }
  else {
    for (const [property, value] of [["width", width], ["height", height]]) {
      out += value.t === "len" ? `${property}:${value.css};` : `${property}:auto;`;
    }
  }
  return out;
}

const SELF_KEYWORDS = { start: "Start", center: "Center", end: "End", stretch: "Stretch" };

/// `css::node_rules`, from set descriptors.
export function nodeRules(layout, flow, sets, display) {
  const contextual = [];
  for (const set of sets) for (const entry of set.ctx) contextual.push(entry);
  if (contextual.length === 0) return null;
  const groups = [];
  for (const entry of contextual) {
    const key = conditionKey(entry.c);
    if (!groups.some((group) => group.key === key)) groups.push({ key, condition: entry.c });
  }
  groups.sort((a, b) => (a.condition.min ?? 0) - (b.condition.min ?? 0));
  let rules = "";
  for (const { condition } of groups) {
    let width = sizeValueOf(mode(layout.width));
    let height = sizeValueOf(mode(layout.height));
    let alignSelf = layout.align_self ?? null;
    let shown;
    for (const entry of contextual) {
      if (!implies(condition, entry.c)) continue;
      const value = entry.v;
      if (entry.p === "width" && value.t !== "none") width = value;
      else if (entry.p === "height" && value.t !== "none") height = value;
      else if (entry.p === "align-self") {
        if (value.t === "auto") alignSelf = null;
        else if (SELF_KEYWORDS[value.t]) alignSelf = SELF_KEYWORDS[value.t];
      } else if (entry.p === "display") {
        if (value.t === "hidden") shown = false;
        else if (value.t === "shown") shown = true;
      }
    }
    let body = contextualBody(flow, width, height, alignSelf);
    if (shown === false) body += "display:none;";
    else if (shown === true) body += `display:${display};`;
    rules += wrap(condition.media, condition.sel, body);
  }
  return rules;
}

const LAYERS = ["l", "v", "d", "n"];

/// The rules one render needs (`css::StyleSheet`).
export class Sheet {
  constructor() { this.rules = new Map(LAYERS.map((layer) => [layer, new Map()])); }
  add(layer, text) {
    const name = className(layer, text);
    const rules = this.rules.get(layer);
    if (!rules.has(name)) rules.set(name, `@layer rn-${layer}{${text.split("&").join("." + name)}}`);
    return name;
  }
  addNamed(layer, name, text) {
    const rules = this.rules.get(layer);
    if (!rules.has(name)) rules.set(name, `@layer rn-${layer}{${text.split("&").join("." + name)}}`);
    return name;
  }
  layout(css) { return css ? this.add("l", `&{${css}}`) : null; }
  entries() {
    const out = [];
    for (const layer of LAYERS) for (const [name, rule] of [...this.rules.get(layer)].sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0))) out.push([name, rule]);
    return out;
  }
}

// ------------------------------------------ realizer (mirror of dom.rs) ----

function el(tag) { return { t: tag, a: [], c: [] }; }
function setAttr(element, name, value) {
  const existing = element.a.find((pair) => pair[0] === name);
  if (existing) existing[1] = value; else element.a.push([name, value]);
}
function getAttr(element, name) {
  const pair = element.a.find((entry) => entry[0] === name);
  return pair ? pair[1] : null;
}

const NATIVE_FOCUSABLE = new Set(["button", "input", "select", "textarea", "a"]);
const FORM_CONTROLS = new Set(["button", "input", "select", "textarea"]);

function nativeRole(tag, type, sized) {
  switch (tag) {
    case "span": case "label": return "Label";
    case "button": return "Button";
    case "input":
      return { checkbox: "CheckBox", radio: "RadioButton", range: "Slider", number: "SpinButton" }[type] ?? "TextInput";
    case "textarea": return "TextInput";
    case "progress": return "ProgressBar";
    case "select": return sized ? "List" : "ComboBox";
    case "hr": return "Separator";
    case "a": return "Link";
    case "img": return "Image";
    case "ul": return "List";
    case "li": return "ListItem";
    case "dialog": return "Dialog";
    case "h1": case "h2": case "h3": case "h4": case "h5": case "h6": return "Heading" + tag[1];
    default: return "None";
  }
}

const ARIA_ROLES = {
  Button: "button", TextInput: "textbox", Group: "group", CheckBox: "checkbox", RadioButton: "radio", Slider: "slider",
  ProgressBar: "progressbar", List: "list", ListItem: "listitem", TabList: "tablist", Tab: "tab", TabPanel: "tabpanel",
  Heading: "heading", Image: "img", Canvas: "img", Link: "link", Dialog: "dialog", Toolbar: "toolbar", Menu: "menu",
  MenuItem: "menuitem", Tree: "tree", TreeItem: "treeitem", Table: "table", Cell: "cell", Status: "status",
  ComboBox: "combobox", SpinButton: "spinbutton", Separator: "separator", Alert: "alert",
};

function roleKey(a11y) { return a11y.role === "Heading" ? "Heading" + a11y.level : a11y.role; }

/// Converts runtime nodes to elements (`dom::Realizer`).
export class Realizer {
  constructor(sheet, scope, root) {
    this.sheet = sheet;
    this.scope = scope;
    this.keys = new Set();
    const collect = (node) => { this.keys.add(node.k); for (const child of node.children || []) collect(child); };
    collect(root);
  }
  elementId(key) { return this.scope + key; }
  related(key) { return this.keys.has(key) ? this.scope + key : null; }
  root(node) { return this.element(node, { t: "root" }); }

  element(node, flow) {
    switch (node.kind) {
      case "label": {
        const level = node.a11y.role === "Heading" && node.a11y.level >= 1 && node.a11y.level <= 6 ? node.a11y.level : 0;
        const element = el(level ? "h" + level : "span");
        element.c.push(node.text);
        return this.decorate(element, node, flow, "rn-label", { t: "none" }, level ? "Heading" + level : "Label", node.text);
      }
      case "button": {
        const element = el("button");
        setAttr(element, "type", "button");
        element.c.push(node.text);
        return this.decorate(element, node, flow, "rn-button", { t: "none" }, "Button", node.text);
      }
      case "input": {
        const element = el("input");
        setAttr(element, "type", "text");
        setAttr(element, "name", node.k);
        setAttr(element, "value", node.value);
        return this.decorate(element, node, flow, "rn-input", { t: "none" }, "TextInput", null);
      }
      case "tabs": {
        const id = this.elementId(node.k);
        const element = el("div");
        setAttr(element, "role", "tablist");
        node.labels.forEach((label, index) => {
          const tab = el("button");
          setAttr(tab, "type", "button");
          setAttr(tab, "role", "tab");
          setAttr(tab, "id", `${id}.${index}`);
          setAttr(tab, "aria-selected", index === node.selected ? "true" : "false");
          setAttr(tab, "tabindex", index === node.selected ? "0" : "-1");
          setAttr(tab, "data-i", String(index));
          tab.c.push(label);
          element.c.push(tab);
        });
        return this.decorate(element, node, flow, "rn-tabs", { t: "none" }, "TabList", null);
      }
      case "control": return this.control(node, flow);
      case "canvas": {
        const element = el("div");
        element.c.push(node.svg);
        return this.decorate(element, node, flow, "rn-canvas", { t: "none" }, "None", null);
      }
      case "surface": {
        const element = el("div");
        if (node.foreign) setAttr(element, "data-foreign", node.foreign);
        return this.decorate(element, node, flow, "rn-surface", { t: "none" }, "None", null);
      }
      case "grid": {
        const element = el("div");
        for (const child of node.children) element.c.push(this.element(child, { t: "grid" }));
        return this.decorate(element, node, flow, "rn-grid", { t: "grid", grid: node.grid }, "None", null);
      }
      case "column":
      case "row": {
        const inner = { t: node.kind, a: node.col.align_items };
        return this.linear(node, flow, inner, { t: "linear", style: node.col }, node.kind === "column" ? "rn-col" : "rn-row");
      }
      default:
        throw new Error("unknown node kind " + node.kind);
    }
  }

  linear(node, flow, inner, container, kind) {
    const role = node.a11y.role;
    const children = node.children;
    const list = role === "List" && children.length > 0 && children.every((child) => child.a11y.role === "ListItem");
    const [tag, native] = role === "Dialog" ? ["dialog", "Dialog"] : list ? ["ul", "List"] : ["div", "None"];
    const element = el(tag);
    if (tag === "dialog") setAttr(element, "open", "");
    const items = [];
    const realized = [];
    for (const child of children) {
      let item = this.element(child, inner);
      if (list) item = asListItem(item);
      if (child.index !== null && child.index !== undefined) realized.push(child.index);
      items.push(item);
    }
    let classes = kind;
    if (node.virt) {
      classes = kind + " rn-vlist";
      const style = node.virt;
      const first = realized.length ? Math.min(...realized) : 0;
      const end = realized.length ? Math.max(...realized) + 1 : 0;
      const offset = (index) => extentOffset(style, index);
      const before = offset(first);
      const after = Math.max(0, offset(style.item_count) - offset(Math.min(end, style.item_count)));
      const property = style.axis === "Horizontal" ? "width" : "height";
      const spacer = (length) => {
        const spacer = el("div");
        setAttr(spacer, "aria-hidden", "true");
        setAttr(spacer, "class", `rn rn-spacer ${this.sheet.layout(`${property}:${length}px;`) ?? ""}`);
        return spacer;
      };
      element.c.push(spacer(before), ...items, spacer(after));
      setAttr(element, "data-vlist", `${style.axis === "Horizontal" ? "row" : "column"} ${style.item_count}`);
    } else {
      element.c.push(...items);
    }
    return this.decorate(element, node, flow, classes, container, native, null);
  }

  control(node, flow) {
    const control = node.control;
    const kind = typeof control === "string" ? control : Object.keys(control)[0];
    const data = typeof control === "string" ? {} : control[kind];
    const decorate = (element, native) => this.decorate(element, node, flow, "rn-control", { t: "none" }, native, null);
    switch (kind) {
      case "Checkbox":
      case "Toggle": {
        const input = el("input");
        setAttr(input, "type", "checkbox");
        if (kind === "Checkbox" ? data.checked : data.on) setAttr(input, "checked", "");
        if (kind === "Toggle") setAttr(input, "role", "switch");
        return this.labelledControl(node, flow, input, data.label, "CheckBox");
      }
      case "Radio": {
        const input = el("input");
        setAttr(input, "type", "radio");
        if (data.selected) setAttr(input, "checked", "");
        return this.labelledControl(node, flow, input, data.label, "RadioButton");
      }
      case "Slider": {
        const element = el("input");
        setAttr(element, "type", "range");
        setAttr(element, "min", String(data.min));
        setAttr(element, "max", String(data.max));
        setAttr(element, "value", String(data.value));
        return decorate(element, "Slider");
      }
      case "Spinner": {
        const element = el("input");
        setAttr(element, "type", "number");
        setAttr(element, "min", String(data.min));
        setAttr(element, "max", String(data.max));
        setAttr(element, "step", "1");
        setAttr(element, "value", String(data.value));
        return decorate(element, "SpinButton");
      }
      case "Progress": {
        const element = el("progress");
        setAttr(element, "max", "100");
        if (data.percent !== null && data.percent !== undefined) setAttr(element, "value", String(data.percent));
        return decorate(element, "ProgressBar");
      }
      case "Select": return decorate(options(el("select"), data.options, data.selected), "ComboBox");
      case "ListBox": {
        const select = el("select");
        setAttr(select, "size", String(Math.min(10, Math.max(2, data.items.length))));
        return decorate(options(select, data.items, data.selected), "List");
      }
      case "DatePicker": {
        const element = el("input");
        setAttr(element, "type", "date");
        const { year, month, day } = data.date;
        setAttr(element, "value", `${String(year).padStart(4, "0")}-${String(month).padStart(2, "0")}-${String(day).padStart(2, "0")}`);
        return decorate(element, "TextInput");
      }
      case "Separator": return decorate(el("hr"), "Separator");
      case "Link": {
        const element = el("a");
        setAttr(element, "href", data.href ?? "#");
        element.c.push(data.text);
        return this.decorate(element, node, flow, "rn-control", { t: "none" }, "Link", data.text);
      }
      case "MultilineText": {
        const element = el("textarea");
        element.c.push(data.value);
        return this.decorate(element, node, flow, "rn-input", { t: "none" }, "TextInput", null);
      }
      default:
        return decorate(el("div"), "None");
    }
  }

  labelledControl(node, flow, input, label, native) {
    setAttr(input, "id", this.elementId(node.k));
    if (native === "RadioButton") {
      const owner = node.k.includes("~") ? "r" + node.k.split("~")[0] : "r";
      setAttr(input, "name", this.scope + owner);
    }
    if (node.disabled) setAttr(input, "disabled", "");
    this.accessibility(input, node, native, label);
    let wrapper = el("label");
    wrapper.c.push(input, label);
    wrapper = this.classes(wrapper, node, flow, "rn-control rn-check", { t: "none" }, "inline-flex");
    wrapper.k = node.k;
    return wrapper;
  }

  decorate(element, node, flow, kind, container, native, visibleText) {
    setAttr(element, "id", this.elementId(node.k));
    element.k = node.k;
    const display = container.t === "grid" ? "grid" : container.t === "linear" ? "flex" : "revert";
    this.classes(element, node, flow, kind, container, display);
    if (node.disabled) {
      if (FORM_CONTROLS.has(element.t)) setAttr(element, "disabled", "");
      else setAttr(element, "aria-disabled", "true");
    }
    if (element.t === "input" || element.t === "select") {
      native = nativeRole(element.t, getAttr(element, "type"), getAttr(element, "size") !== null);
    }
    this.accessibility(element, node, native, visibleText);
    return element;
  }

  classes(element, node, flow, kind, container, display) {
    const classes = ["rn", kind];
    const layout = this.sheet.layout(layoutCss(node.layout, flow, container));
    if (layout) classes.push(layout);
    const visual = visualRules(node.style, node.states, node.opacity ?? 1, node.cursor);
    if (visual) classes.push(this.sheet.add("v", visual));
    for (const set of node.sets || []) {
      if (set.d) classes.push(this.sheet.addNamed("d", set.d, set.rules));
    }
    const rules = nodeRules(node.layout, flow, node.sets || [], display);
    if (rules) classes.push(this.sheet.add("n", rules));
    const deduped = classes.filter((name, index) => index === 0 || name !== classes[index - 1]);
    setAttr(element, "class", deduped.join(" "));
    if (node.hidden) setAttr(element, "hidden", "");
    if (node.layout.direction) setAttr(element, "dir", node.layout.direction === "Rtl" ? "rtl" : "ltr");
    if (node.command) setAttr(element, "data-command", node.command);
    if (node.shared) setAttr(element, "data-shared", node.shared);
    if (node.index !== null && node.index !== undefined) setAttr(element, "data-index", String(node.index));
    return element;
  }

  accessibility(element, node, native, visibleText) {
    const a11y = node.a11y;
    const role = roleKey(a11y);
    const unnamedGroup = a11y.role === "Group" && (a11y.name === null || a11y.name === undefined);
    if (role !== native && !unnamedGroup) {
      const name = ARIA_ROLES[a11y.role];
      if (name) setAttr(element, "role", name);
      if (a11y.role === "Heading") setAttr(element, "aria-level", String(a11y.level));
    }
    if (a11y.name !== null && a11y.name !== undefined && visibleText !== a11y.name && element.t !== "img") setAttr(element, "aria-label", a11y.name);
    if (a11y.desc !== null && a11y.desc !== undefined) setAttr(element, "aria-description", a11y.desc);
    if (a11y.auto !== null && a11y.auto !== undefined) setAttr(element, "data-automation-id", a11y.auto);
    const nativelyFocusable = NATIVE_FOCUSABLE.has(element.t);
    if (a11y.focusable && !nativelyFocusable && getAttr(element, "tabindex") === null) setAttr(element, "tabindex", "0");
    else if (!a11y.focusable && nativelyFocusable && !["input", "select", "textarea"].includes(element.t)) setAttr(element, "tabindex", "-1");
    const nativeValue = ["progress", "select", "textarea"].includes(element.t)
      || (element.t === "input" && ["range", "number", "text", "date"].includes(getAttr(element, "type")));
    if (a11y.value && !nativeValue) {
      if (a11y.value.range) {
        setAttr(element, "aria-valuemin", fixed3(a11y.value.range[0]));
        setAttr(element, "aria-valuemax", fixed3(a11y.value.range[1]));
        setAttr(element, "aria-valuenow", fixed3(a11y.value.range[2]));
      } else if (a11y.value.text !== undefined) {
        setAttr(element, "aria-valuetext", a11y.value.text);
      }
    }
    const nativeCheck = element.t === "input" && ["checkbox", "radio"].includes(getAttr(element, "type"));
    if (a11y.checked && !nativeCheck) setAttr(element, "aria-checked", a11y.checked);
    if (a11y.expanded !== null && a11y.expanded !== undefined) setAttr(element, "aria-expanded", String(a11y.expanded));
    if (a11y.selected !== null && a11y.selected !== undefined) setAttr(element, "aria-selected", String(a11y.selected));
    if (a11y.readonly) FORM_CONTROLS.has(element.t) ? setAttr(element, "readonly", "") : setAttr(element, "aria-readonly", "true");
    if (a11y.required) FORM_CONTROLS.has(element.t) ? setAttr(element, "required", "") : setAttr(element, "aria-required", "true");
    if (a11y.busy) setAttr(element, "aria-busy", "true");
    if (a11y.live) setAttr(element, "aria-live", a11y.live);
    if (a11y.position) { setAttr(element, "aria-posinset", String(a11y.position[0])); setAttr(element, "aria-setsize", String(a11y.position[1])); }
    const label = a11y.labelledby ? this.related(a11y.labelledby) : null;
    if (label) setAttr(element, "aria-labelledby", label);
    const described = (a11y.describedby || []).map((key) => this.related(key)).filter((id) => id);
    if (described.length) setAttr(element, "aria-describedby", described.join(" "));
    const controls = (a11y.controls || []).map((key) => this.related(key)).filter((id) => id);
    if (controls.length) setAttr(element, "aria-controls", controls.join(" "));
  }
}

function options(select, items, selected) {
  if (selected === null || selected === undefined) {
    const empty = el("option");
    setAttr(empty, "value", "");
    setAttr(empty, "selected", "");
    setAttr(empty, "disabled", "");
    setAttr(empty, "hidden", "");
    select.c.push(empty);
  }
  items.forEach((text, index) => {
    const option = el("option");
    setAttr(option, "value", String(index));
    if (selected === index) setAttr(option, "selected", "");
    option.c.push(text);
    select.c.push(option);
  });
  return select;
}

function asListItem(item) {
  if (["span", "div", "h1", "h2", "h3", "h4", "h5", "h6"].includes(item.t)) {
    item.t = "li";
    item.a = item.a.filter(([name, value]) => !(name === "role" && value === "listitem"));
    return item;
  }
  const wrapper = el("li");
  setAttr(wrapper, "class", "rn");
  wrapper.c.push(item);
  wrapper.k = item.k;
  delete item.k;
  return wrapper;
}

function extentOffset(style, index) {
  const extent = style.extent.Fixed ?? style.extent.Estimated;
  return extent * index;
}

/// Normalizes an element to its serialized form (the form the Rust side's
/// JSON has): no empty attribute or child lists, no undefined key.
export function normalize(element) {
  if (typeof element === "string") return element;
  const out = { t: element.t };
  if (element.k !== undefined && element.k !== null) out.k = element.k;
  if (element.a && element.a.length) out.a = element.a.map(([name, value]) => [name, value]);
  if (element.c && element.c.length) out.c = element.c.map(normalize);
  return out;
}

/// Realizes `node` as a tree's root (or, with `flow`, as a child laid out
/// in it) with element ids prefixed by `scope`: `{ element, sheet }`.
export function realize(node, scope = "", flow = { t: "root" }) {
  const sheet = new Sheet();
  const element = new Realizer(sheet, scope, node).element(node, flow);
  return { element: normalize(element), sheet };
}

// ------------------------------------------- node builders (generated) ----

const DEFAULT_LAYOUT = () => ({ width: "Fill", height: "Auto", margin: { top: 0, end: 0, bottom: 0, start: 0 }, align_self: null, constraints: { min_width: 0, max_width: null, min_height: 0, max_height: null }, direction: null, grid: null });
const DEFAULT_CONTAINER = () => ({ padding: { top: 24, end: 24, bottom: 24, start: 24 }, gap: 12, align_items: "Stretch", overflow: "Clip" });

export function layout() { return DEFAULT_LAYOUT(); }
export function containerStyle() { return DEFAULT_CONTAINER(); }
export function a11y(role, focusable = false) {
  return { role, level: null, name: null, desc: null, auto: null, focusable, value: null, checked: null, expanded: null, selected: null, readonly: false, required: false, busy: false, live: null, position: null, labelledby: null, describedby: [], controls: [] };
}
export function heading(level) { const info = a11y("Heading"); info.level = level; return info; }

function base(kind, key, layoutStyle, info) {
  return { k: String(key), kind, layout: layoutStyle ?? DEFAULT_LAYOUT(), a11y: info, style: null, states: null, opacity: 1, cursor: null, hidden: false, disabled: false, command: null, shared: null, index: null, sets: [] };
}

export function label(key, text, layoutStyle) { const node = base("label", key, layoutStyle, a11y("Label")); node.text = String(text); return node; }
export function button(key, text, layoutStyle) { const node = base("button", key, layoutStyle, a11y("Button", true)); node.text = String(text); return node; }
export function textInput(key, value, layoutStyle) { const node = base("input", key, layoutStyle, a11y("TextInput", true)); node.value = String(value); return node; }
export function tabBar(key, labels, selected, layoutStyle) { const node = base("tabs", key, layoutStyle, a11y("TabList", true)); node.labels = labels.map(String); node.selected = selected; return node; }
export function column(key, children, layoutStyle, style) { const node = base("column", key, layoutStyle, a11y("Group")); node.col = style ?? DEFAULT_CONTAINER(); node.virt = null; node.children = flatten(children); return node; }
export function row(key, children, layoutStyle, style) { const node = base("row", key, layoutStyle, a11y("Group")); node.col = style ?? DEFAULT_CONTAINER(); node.virt = null; node.children = flatten(children); return node; }
export function grid(key, gridStyle, layoutStyle, children) { const node = base("grid", key, layoutStyle, a11y("Group")); node.grid = gridStyle; node.children = flatten(children); return node; }
export function virtualList(key, list, layoutStyle, children) { const node = column(key, children, layoutStyle); node.virt = list; return node; }

/// A control node, with the accessibility `Control::accessibility` gives it.
export function control(key, value, layoutStyle) {
  const kind = typeof value === "string" ? value : Object.keys(value)[0];
  const data = typeof value === "string" ? {} : value[kind];
  const roles = { Checkbox: "CheckBox", Toggle: "CheckBox", Radio: "RadioButton", Slider: "Slider", Progress: "ProgressBar", Select: "ComboBox", ListBox: "List", DatePicker: "TextInput", MultilineText: "TextInput", Spinner: "SpinButton", Separator: "Separator", Link: "Link", Image: "Image" };
  const info = a11y(roles[kind], !["Progress", "Separator", "Image"].includes(kind));
  const on = (flag) => (flag ? "true" : "false");
  if (kind === "Checkbox") { info.name = data.label; info.checked = on(data.checked); }
  else if (kind === "Toggle") { info.name = data.label; info.checked = on(data.on); }
  else if (kind === "Radio") { info.name = data.label; info.checked = on(data.selected); }
  else if (kind === "Slider" || kind === "Spinner") info.value = { range: [Math.fround(data.min), Math.fround(data.max), Math.fround(Math.min(Math.max(data.value, Math.min(data.min, data.max)), Math.max(data.max, data.min))), 1] };
  else if (kind === "Progress") { if (data.percent !== null && data.percent !== undefined) info.value = { range: [0, 100, data.percent, 1] }; else info.busy = true; }
  else if (kind === "Select") { if (data.selected !== null && data.selected !== undefined && data.selected < data.options.length) info.value = { text: data.options[data.selected] }; }
  else if (kind === "ListBox") { if (data.selected !== null && data.selected !== undefined && data.selected < data.items.length) info.value = { text: data.items[data.selected] }; }
  else if (kind === "DatePicker") { const { year, month, day } = data.date; info.value = { text: `${String(year).padStart(4, "0")}-${String(month).padStart(2, "0")}-${String(day).padStart(2, "0")}` }; }
  else if (kind === "MultilineText") info.value = { text: data.value };
  else if (kind === "Link") info.name = data.text;
  const node = base("control", key, layoutStyle, info);
  node.control = value;
  return node;
}

/// `IntoChildren`: a node, or anything iterable over nodes.
export function flatten(children) {
  const out = [];
  const visit = (child) => {
    if (child === null || child === undefined) return;
    if (Array.isArray(child)) child.forEach(visit);
    else out.push(child);
  };
  visit(children);
  return out;
}

export function extendChildren(target, children) { for (const child of flatten(children)) target.push(child); }

// Modifiers: each returns the node, as the builder methods do.
export function disabled(node, value) { node.disabled = value; return node; }
export function hidden(node, value) { node.hidden = value; return node; }
export function withClass(node, set) { node.sets.push(set); return node; }
export function withAccessibility(node, info) { node.a11y = info; return node; }
export function withStyle(node, style) { node.style = style; return node; }
export function withStateStyle(node, state, style) { node.states = node.states ?? { hover: null, focus: null, active: null, disabled: null }; node.states[state] = style; return node; }
export function withOpacity(node, value) { node.opacity = Math.fround(Math.min(1, Math.max(0, value))); return node; }
export function withCursor(node, cursor) { node.cursor = cursor; return node; }
export function withItemIndex(node, index) { node.index = index; return node; }
export function withSharedId(node, key) { node.shared = String(key); return node; }

// --------------------------------------------------------------- DOM ----

const SVG = "http://www.w3.org/2000/svg";
const PROPERTIES = { value: "value", checked: "checked", selected: "selected", open: "open" };

/// Creates the DOM for an element.
export function create(element, svg = false) {
  if (typeof element === "string") return document.createTextNode(element);
  const inSvg = svg || element.t === "svg";
  const node = inSvg ? document.createElementNS(SVG, element.t) : document.createElement(element.t);
  for (const [name, value] of element.a || []) {
    if (name === "xmlns") continue;
    node.setAttribute(name, value);
  }
  for (const child of element.c || []) node.appendChild(create(child, inSvg));
  if (element.t === "input" && getAttrOf(element, "value") !== null) node.value = getAttrOf(element, "value");
  return node;
}

function getAttrOf(element, name) {
  const pair = (element.a || []).find((entry) => entry[0] === name);
  return pair ? pair[1] : null;
}

function sameNode(old, next) {
  if (typeof old === "string" || typeof next === "string") return typeof old === typeof next;
  return old.t === next.t && (old.k ?? null) === (next.k ?? null);
}

/// Patches `dom` (realizing `old`) to realize `next`; returns the node now
/// in its place.
export function patch(dom, old, next, svg = false) {
  if (typeof next === "string") {
    if (old !== next) dom.nodeValue = next;
    return dom;
  }
  if (!sameNode(old, next)) {
    const replacement = create(next, svg);
    dom.replaceWith(replacement);
    return replacement;
  }
  const inSvg = svg || next.t === "svg";
  const oldAttrs = new Map(old.a || []);
  const nextAttrs = new Map(next.a || []);
  for (const [name] of oldAttrs) if (!nextAttrs.has(name)) {
    dom.removeAttribute(name);
    if (PROPERTIES[name] && name in dom) dom[PROPERTIES[name]] = name === "value" ? "" : false;
  }
  for (const [name, value] of nextAttrs) {
    if (name === "xmlns") continue;
    if (oldAttrs.get(name) !== value) dom.setAttribute(name, value);
    if (name === "value" && dom.value !== value) setValueKeepingSelection(dom, value);
    if (name === "checked" && !dom.checked) dom.checked = true;
    if (name === "selected" && !dom.selected) dom.selected = true;
  }
  if (next.t === "textarea") {
    const text = (next.c || []).join("");
    if (dom.value !== text) setValueKeepingSelection(dom, text);
    return dom;
  }
  patchChildren(dom, old.c || [], next.c || [], inSvg);
  return dom;
}

function setValueKeepingSelection(dom, value) {
  const focused = dom === document.activeElement && typeof dom.selectionStart === "number";
  const [start, end] = focused ? [dom.selectionStart, dom.selectionEnd] : [0, 0];
  dom.value = value;
  if (focused) {
    try { dom.setSelectionRange(Math.min(start, value.length), Math.min(end, value.length)); } catch (_) { /* not a text control */ }
  }
}

function patchChildren(parent, old, next, svg) {
  const doms = [...parent.childNodes];
  const byKey = new Map();
  old.forEach((child, index) => {
    if (typeof child !== "string" && child.k !== undefined && child.k !== null) byKey.set(child.k, index);
  });
  const used = new Set();
  let reference = parent.firstChild;
  const placed = [];
  next.forEach((child, position) => {
    let match = -1;
    if (typeof child !== "string" && child.k !== undefined && child.k !== null) {
      const index = byKey.get(child.k);
      if (index !== undefined && !used.has(index) && sameNode(old[index], child)) match = index;
    } else if (position < old.length && !used.has(position) && sameNode(old[position], child)
      && (typeof old[position] === "string" || old[position].k === undefined || old[position].k === null)) {
      match = position;
    }
    let dom;
    if (match >= 0) {
      used.add(match);
      dom = patch(doms[match], old[match], child, svg);
    } else {
      dom = create(child, svg);
    }
    placed.push(dom);
  });
  old.forEach((_, index) => { if (!used.has(index) && doms[index] && doms[index].parentNode === parent) doms[index].remove(); });
  for (const dom of placed) {
    if (dom !== reference) parent.insertBefore(dom, reference);
    else reference = reference.nextSibling;
    if (dom === reference) reference = reference.nextSibling;
  }
  // Anything after the placed children that is not ours goes.
  while (reference) { const nextSibling = reference.nextSibling; if (!placed.includes(reference)) reference.remove(); reference = nextSibling; }
}

/// Where a server-rendered element and a realized one first differ, or
/// `null` when they agree (the attach check: a mismatch is a bug in one of
/// the two realizers, reported and repaired, never silent).
export function mismatch(dom, element, path = "") {
  if (typeof element === "string") {
    return dom && dom.nodeType === 3 && dom.nodeValue === element ? null : `${path}: text ${JSON.stringify(element)}`;
  }
  if (!dom || dom.nodeType !== 1 || dom.localName !== element.t) return `${path}: <${element.t}>`;
  for (const [name, value] of element.a || []) {
    if (name === "xmlns") continue;
    if (dom.getAttribute(name) !== value) return `${path}/${element.t}: @${name}=${JSON.stringify(value)} but ${JSON.stringify(dom.getAttribute(name))}`;
  }
  const children = element.c || [];
  const childNodes = [...dom.childNodes].filter((node) => node.nodeType === 1 || node.nodeType === 3);
  if (element.t === "textarea") return null;
  if (childNodes.length !== children.length) return `${path}/${element.t}: ${children.length} children but ${childNodes.length}`;
  for (let i = 0; i < children.length; i++) {
    const difference = mismatch(childNodes[i], children[i], `${path}/${element.t}[${i}]`);
    if (difference) return difference;
  }
  return null;
}

// --------------------------------------------------------- islands ----

let styles = null;
const inserted = new Set();

function applySheet(sheet) {
  if (typeof document === "undefined") return;
  if (!styles) {
    styles = new CSSStyleSheet();
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, styles];
  }
  for (const [name, rule] of sheet.entries()) {
    if (inserted.has(name)) continue;
    inserted.add(name);
    try { styles.insertRule(rule, styles.cssRules.length); } catch (error) { console.error("rn: a rule did not parse", rule, error); }
  }
}

const KEYS = {
  Enter: "Enter", " ": "Space", Tab: "Tab", Escape: "Escape", Backspace: "Backspace", ArrowLeft: "ArrowLeft",
  ArrowRight: "ArrowRight", ArrowUp: "ArrowUp", ArrowDown: "ArrowDown", Delete: "Delete", Insert: "Insert",
  Home: "Home", End: "End", PageUp: "PageUp", PageDown: "PageDown",
};

export function keyCode(event) {
  if (KEYS[event.key]) return KEYS[event.key];
  const fn = /^F(\d{1,2})$/.exec(event.key);
  if (fn) return { Function: Number(fn[1]) };
  if ([...event.key].length === 1) return { Character: event.key };
  return { Unknown: event.keyCode || 0 };
}

export const config = { fns: {}, base: "", csrf: null, assets: "/_rn/", version: null };
export const islands = [];
const topics = new Map();

export class Island {
  constructor(index, module, state, root, flow) {
    this.index = index;
    this.module = module;
    this.state = state;
    this.root = root;
    this.flow = flow;
    this.scope = `i${index}-`;
    this.tree = null;
    this.effects = [];
    this.scheduled = false;
    this.failed = null;
    this.renders = 0;
    this.subscriptions = [];
  }

  realize() {
    const node = this.module.view(this.state);
    const { element, sheet } = realize(node, this.scope, this.flow);
    applySheet(sheet);
    return element;
  }

  attach() {
    const element = this.realize();
    const difference = mismatch(this.root, element);
    if (difference) {
      console.error(`rn:mismatch island ${this.index} (${this.module.name}) ${difference}`);
      document.dispatchEvent(new CustomEvent("rn:mismatch", { detail: { island: this.index, at: difference } }));
      const replacement = create(element);
      this.root.replaceWith(replacement);
      this.root = replacement;
    }
    this.tree = element;
    this.listen();
    if (this.module.init) this.run(() => this.module.init(this.state, this.fx()));
  }

  fx() {
    const queue = this.effects;
    const make = (kind) => (...args) => { queue.push([kind, args]); };
    return new Proxy({}, { get: (_, kind) => make(kind) });
  }

  run(step) {
    if (this.failed) return;
    try {
      step();
    } catch (error) {
      this.failed = error;
      console.error(`rn: island ${this.index} (${this.module.name}) failed:`, error);
      this.root.setAttribute("data-rn-failed", "");
      document.dispatchEvent(new CustomEvent("rn:failure", { detail: { island: this.index, message: String(error && error.message || error) } }));
      return;
    }
    this.schedule();
  }

  dispatch(event) { this.run(() => this.module.update(this.state, event, this.fx())); }

  deliver(message) {
    if (!this.module.message) return;
    this.run(() => this.module.message(this.state, message, this.fx()));
  }

  schedule() {
    if (this.scheduled) return;
    this.scheduled = true;
    queueMicrotask(() => {
      this.scheduled = false;
      this.render();
      const effects = this.effects.splice(0);
      for (const [kind, args] of effects) perform(this, kind, args);
    });
  }

  render() {
    const element = this.realize();
    this.root = patch(this.root, this.tree, element);
    this.tree = element;
    this.renders++;
    this.syncControlled(this.root, element);
  }

  // A control shows what the state says, even when the state refused the
  // person's change (a controlled input).
  syncControlled(dom, element) {
    if (!dom || typeof element === "string") return;
    if (element.t === "input") {
      const type = getAttrOf(element, "type");
      if (type === "checkbox" || type === "radio") {
        const checked = getAttrOf(element, "checked") !== null;
        if (dom.checked !== checked) dom.checked = checked;
      } else {
        const value = getAttrOf(element, "value") ?? "";
        if (dom.value !== value) setValueKeepingSelection(dom, value);
      }
      return;
    }
    if (element.t === "select") {
      const option = (element.c || []).findIndex((child) => getAttrOf(child, "selected") !== null);
      if (option >= 0 && dom.selectedIndex !== option) dom.selectedIndex = option;
    }
    const children = element.c || [];
    const nodes = [...dom.childNodes];
    for (let i = 0; i < children.length && i < nodes.length; i++) this.syncControlled(nodes[i], children[i]);
  }

  keyOf(target) {
    for (let node = target; node && node !== this.root.parentNode; node = node.parentNode) {
      if (node.nodeType === 1 && node.id && node.id.startsWith(this.scope)) {
        const key = node.id.slice(this.scope.length);
        return /\.\d+$/.test(key) && node.getAttribute("role") === "tab" ? { key: key.replace(/\.\d+$/, ""), tab: Number(node.getAttribute("data-i")) } : { key, element: node };
      }
    }
    return null;
  }

  listen() {
    const root = this.root;
    const on = (type, handler) => root.addEventListener(type, handler);
    on("click", (event) => {
      const found = this.keyOf(event.target);
      if (!found) return;
      if (found.tab !== undefined) { this.dispatch({ type: "TabSelected", target: found.key, index: found.tab }); return; }
      const element = found.element;
      if (element.localName === "input" || element.localName === "select" || element.localName === "textarea") return;
      if (element.localName === "a" && element.getAttribute("href") === "#") event.preventDefault();
      this.dispatch({ type: "Click", target: found.key });
    });
    on("input", (event) => {
      const found = this.keyOf(event.target);
      if (!found || !found.element) return;
      const element = found.element;
      const type = element.type;
      if (element.localName === "textarea" || (element.localName === "input" && (type === "text" || type === "search" || type === "email" || type === "password"))) {
        this.dispatch({ type: "TextChanged", target: found.key, value: element.value });
      } else if (type === "range" || type === "number") {
        if (element.value !== "" && /^-?\d+$/.test(element.value)) this.dispatch({ type: "ValueChanged", target: found.key, value: Number(element.value) });
      }
    });
    on("change", (event) => {
      const found = this.keyOf(event.target);
      if (!found || !found.element) return;
      const element = found.element;
      if (element.type === "checkbox") this.dispatch({ type: "Toggled", target: found.key, on: element.checked });
      else if (element.type === "radio") this.dispatch({ type: "Toggled", target: found.key, on: true });
      else if (element.localName === "select") this.dispatch({ type: "SelectionChanged", target: found.key, index: element.value === "" ? null : Number(element.value) });
      else if (element.type === "date") {
        const match = /^(\d{4,})-(\d{2})-(\d{2})$/.exec(element.value);
        if (match) this.dispatch({ type: "DateChanged", target: found.key, date: { year: Number(match[1]), month: Number(match[2]), day: Number(match[3]) } });
      }
    });
    const key = (type) => (event) => {
      const found = this.keyOf(event.target);
      this.dispatch({ type, target: found ? found.key : null, key: keyCode(event), modifiers: { shift: event.shiftKey, ctrl: event.ctrlKey, alt: event.altKey, meta: event.metaKey } });
    };
    on("keydown", key("KeyDown"));
    on("keyup", key("KeyUp"));
    on("focusin", (event) => { const found = this.keyOf(event.target); if (found) this.dispatch({ type: "FocusGained", target: found.key }); });
    on("focusout", (event) => { const found = this.keyOf(event.target); if (found) this.dispatch({ type: "FocusLost", target: found.key }); });
  }
}

function csrfToken() {
  if (config.csrf) return config.csrf;
  const match = /(?:^|;\s*)__Host-csrf=([^;]+)/.exec(typeof document === "undefined" ? "" : document.cookie);
  return match ? match[1] : null;
}

/// Calls a server function: `Ok(output)` or `Err(ServerFnError)`.
export async function callServer(path, input) {
  const headers = { "content-type": "application/json", accept: "application/json" };
  const token = csrfToken();
  if (token) headers["x-csrf-token"] = token;
  let response;
  try {
    response = await fetch(`${config.base}/_fn/${path}`, { method: "POST", headers, body: JSON.stringify(input), credentials: "same-origin" });
  } catch (error) {
    return { Err: { Transport: String(error && error.message || error) } };
  }
  const text = await response.text();
  if (!response.ok) {
    let message = "";
    try { message = JSON.parse(text).error ?? ""; } catch (_) { /* not JSON */ }
    return { Err: { Server: { status: response.status, message } } };
  }
  try { return { Ok: JSON.parse(text) }; } catch (error) { return { Err: { Decode: String(error.message) } }; }
}

/// Carries out one effect for `island`; replies arrive as messages.
export function perform(island, kind, args) {
  const reply = (message) => island.deliver(message);
  switch (kind) {
    case "call": {
      const [fn, input, answer] = args;
      const path = config.fns[fn] ?? fn;
      callServer(path, input).then((result) => reply(answer(result)));
      break;
    }
    case "after": setTimeout(() => reply(args[1]), args[0]); break;
    case "navigate": navigate(args[0]); break;
    case "back": history.back(); break;
    case "focus": { const target = document.getElementById(island.scope + args[0]); if (target) target.focus(); break; }
    case "copy": navigator.clipboard?.writeText(args[0]).catch(() => {}); break;
    case "store": try { localStorage.setItem(args[0], JSON.stringify(args[1])); } catch (_) { /* storage refused */ } break;
    case "load": {
      let value = null;
      try { const text = localStorage.getItem(args[0]); value = text === null ? null : JSON.parse(text); } catch (_) { value = null; }
      queueMicrotask(() => reply(args[1](value)));
      break;
    }
    case "notify":
      if (typeof Notification !== "undefined" && Notification.permission === "granted") new Notification(args[0], { body: args[1] });
      break;
    case "fetch": {
      const [url, method, body, answer] = args;
      fetch(url, { method, body: method === "GET" ? undefined : body }).then(async (response) => {
        const text = await response.text();
        reply(answer({ Ok: { status: response.status, headers: [...response.headers], body: text } }));
      }, (error) => reply(answer({ Err: String(error && error.message || error) })));
      break;
    }
    case "publish": {
      topics.set(args[0], args[1]);
      for (const other of islands) for (const [topic, map] of other.subscriptions) if (topic === args[0]) other.deliver(map(clone(args[1])));
      break;
    }
    case "subscribe": island.subscriptions.push([args[0], args[1]]); if (topics.has(args[0])) queueMicrotask(() => island.deliver(args[1](clone(topics.get(args[0]))))); break;
    case "download": {
      const url = URL.createObjectURL(new Blob([args[1]], { type: "text/plain" }));
      const link = document.createElement("a");
      link.href = url; link.download = args[0]; link.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
      break;
    }
    case "js": {
      const [module, fn, input, answer] = args;
      import(module).then((loaded) => loaded[fn](input)).then((value) => reply(answer({ Ok: value })), (error) => reply(answer({ Err: String(error && error.message || error) })));
      break;
    }
    default:
      if (extraEffects[kind]) extraEffects[kind](island, args, reply);
      else console.error("rn: unknown effect", kind);
  }
}

/// Effects other runtime modules add (capabilities, Web milestone E).
export const extraEffects = {};

/// Navigation (Web milestone G adds the in-page swap; this is the fallback).
export let navigate = (url) => { location.assign(url); };
export function setNavigate(handler) { navigate = handler; }

/// Attaches every island the page declares.
export async function start() {
  const data = document.getElementById("rn-data");
  if (!data) return;
  const page = JSON.parse(data.textContent);
  Object.assign(config, page.config || {});
  // Generated modules receive this runtime's own namespace.
  const runtime = await import(import.meta.url);
  await Promise.all((page.islands || []).map(async (spec) => {
    const root = document.querySelector(`[data-rn-i="${spec.i}"]`);
    if (!root || spec.kind !== "client") return;
    const loaded = await import(spec.m);
    const module = loaded.default(runtime, spec.fns ?? {});
    const island = new Island(spec.i, module, spec.s, root, spec.flow ?? { t: "root" });
    islands[spec.i] = island;
    island.attach();
  }));
  document.documentElement.setAttribute("data-rn-ready", "");
  document.dispatchEvent(new CustomEvent("rn:ready"));
}
