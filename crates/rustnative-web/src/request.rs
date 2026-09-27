//! What a page render knows about the request it answers.
//!
//! A server render puts a [`RequestInfo`] in the render's services; a
//! component reads it through [`request`]. Reading it makes the component
//! *dynamic*: a partially prerendered page (`C06-1`) renders its static
//! shell once, with no request, and each component that asked for the
//! request there renders its per-request part in a hole streamed later.

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use rustnative_core::ComponentContext;

use crate::client::ServerRender;

/// The request a page answers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestInfo {
    /// The path (`/notes/7`).
    pub path: String,
    /// The query string, without `?`.
    pub query: String,
    /// The request headers a page may read, lower-case names.
    pub headers: Vec<(String, String)>,
    /// The route's parameters.
    pub params: Vec<(String, String)>,
    /// The request-forgery token forms and server calls carry.
    pub csrf: String,
    /// The nonce the response's content security policy allows.
    pub nonce: String,
    /// What the host allows this request (`W-SL-2`).
    pub limits: HostLimits,
}

impl RequestInfo {
    /// A request for `path` (`/notes?sort=new`).
    #[must_use]
    pub fn get(path: &str) -> Self {
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        Self { path: path.to_owned(), query: query.to_owned(), ..Self::default() }
    }

    /// Header `name` (any case), if sent.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Route parameter `name`, if the route has it.
    #[must_use]
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    /// Query parameter `name`, percent-decoded, if present.
    #[must_use]
    pub fn query_param(&self, name: &str) -> Option<String> {
        self.query
            .split('&')
            .filter_map(|pair| pair.split_once('=').or(Some((pair, ""))))
            .find_map(|(key, value)| (decode(key) == name).then(|| decode(value)))
    }

    /// What the host allows this request.
    #[must_use]
    pub const fn limits(&self) -> &HostLimits {
        &self.limits
    }

    /// The query string as `T` (`C13-2`): typed, validated by `T`'s
    /// deserialization, with `T`'s defaults for what is missing
    /// (`#[serde(default)]`). [`query_string`] writes the same shape back.
    ///
    /// # Errors
    ///
    /// A parameter is missing or does not parse as its field's type.
    pub fn query_as<T: DeserializeOwned>(&self) -> Result<T, String> {
        let pairs: Vec<(String, String)> = self
            .query
            .split('&')
            .filter(|pair| !pair.is_empty())
            .map(|pair| {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                (decode(key), decode(value))
            })
            .collect();
        from_pairs(pairs)
    }
}

/// `T` from decoded `application/x-www-form-urlencoded` pairs (a query
/// string, a form body): each value is read as its field's type wants it —
/// a number, a boolean, text, or an `Option` of one — and a repeated name
/// is a sequence.
///
/// # Errors
///
/// A field is missing, or a value does not parse as its type.
pub fn from_pairs<T: DeserializeOwned>(pairs: Vec<(String, String)>) -> Result<T, String> {
    let mut fields: Vec<(String, Vec<String>)> = Vec::new();
    for (key, value) in pairs {
        match fields.iter_mut().find(|(name, _)| *name == key) {
            Some((_, values)) => values.push(value),
            None => fields.push((key, vec![value])),
        }
    }
    let map = serde::de::value::MapDeserializer::new(
        fields.into_iter().map(|(key, values)| (key, Text(values))),
    );
    T::deserialize(map).map_err(|error: serde::de::value::Error| error.to_string())
}

/// A query value, read as whatever its field asks for.
struct Text(Vec<String>);

impl Text {
    fn last(&self) -> String {
        self.0.last().cloned().unwrap_or_default()
    }
}

impl serde::de::IntoDeserializer<'_, serde::de::value::Error> for Text {
    type Deserializer = Self;
    fn into_deserializer(self) -> Self {
        self
    }
}

macro_rules! parse_as {
    ($($method:ident => $visit:ident),* $(,)?) => {$(
        fn $method<V: serde::de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            let text = self.last();
            match text.parse() {
                Ok(value) => visitor.$visit(value),
                Err(_) => Err(serde::de::Error::invalid_value(serde::de::Unexpected::Str(&text), &visitor)),
            }
        }
    )*};
}

impl<'de> serde::Deserializer<'de> for Text {
    type Error = serde::de::value::Error;

    fn deserialize_any<V: serde::de::Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_string(self.last())
    }

    parse_as! {
        deserialize_bool => visit_bool,
        deserialize_i8 => visit_i8,
        deserialize_i16 => visit_i16,
        deserialize_i32 => visit_i32,
        deserialize_i64 => visit_i64,
        deserialize_u8 => visit_u8,
        deserialize_u16 => visit_u16,
        deserialize_u32 => visit_u32,
        deserialize_u64 => visit_u64,
        deserialize_f32 => visit_f32,
        deserialize_f64 => visit_f64,
        deserialize_char => visit_char,
    }

    fn deserialize_option<V: serde::de::Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if self.0.iter().all(String::is_empty) {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_seq<V: serde::de::Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let items = self.0.into_iter().map(|value| Text(vec![value]));
        visitor.visit_seq(serde::de::value::SeqDeserializer::new(items))
    }

    fn deserialize_enum<V: serde::de::Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let text: serde::de::value::StringDeserializer<Self::Error> =
            serde::de::IntoDeserializer::into_deserializer(self.last());
        visitor.visit_enum(text)
    }

    serde::forward_to_deserialize_any! {
        i128 u128 str string bytes byte_buf unit unit_struct newtype_struct tuple
        tuple_struct map struct identifier ignored_any
    }
}

/// `value`'s fields as a query string (without `?`), sorted by name,
/// leaving out `None`: the inverse of [`RequestInfo::query_as`], so a
/// link built from a typed value reads back as the same value.
#[must_use]
pub fn query_string(value: &impl Serialize) -> String {
    let Ok(Value::Object(fields)) = serde_json::to_value(value) else { return String::new() };
    let mut out = String::new();
    for (key, value) in fields {
        let text = match value {
            Value::Null => continue,
            Value::String(text) => text,
            other => other.to_string(),
        };
        if !out.is_empty() {
            out.push('&');
        }
        encode_into(&mut out, &key);
        out.push('=');
        encode_into(&mut out, &text);
    }
    out
}

fn encode_into(out: &mut String, text: &str) {
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            b' ' => out.push('+'),
            other => {
                let _ = write!(out, "%{other:02X}");
            }
        }
    }
}

/// `application/x-www-form-urlencoded` decoding: `+` is a space, `%XX` a
/// byte; malformed escapes stay as written.
#[must_use]
pub fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => out.push(b' '),
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3])
                    .ok()
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok());
                if let Some(byte) = hex {
                    out.push(byte);
                    index += 3;
                    continue;
                }
                out.push(b'%');
            }
            byte => out.push(byte),
        }
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The host's limits for one request, as capabilities an application can
/// ask about (`W-SL-2`): a function runtime's deadline, an edge sandbox's
/// memory and response ceilings. `None` is "no stated limit".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostLimits {
    /// How long the request may take, from when it arrived.
    pub deadline: Option<Duration>,
    /// Memory available to the request's work, in bytes.
    pub memory: Option<u64>,
    /// Whether a writable file system is available.
    pub filesystem: bool,
    /// The largest response body, in bytes.
    pub response_bytes: Option<u64>,
    /// The largest request body, in bytes.
    pub payload_bytes: Option<u64>,
}

/// The request the render answers, or `None` when there is none — a static
/// export, or the shell of a partially prerendered page. Asking marks the
/// component dynamic.
#[must_use]
pub fn request<M: Send + 'static>(context: &ComponentContext<'_, M>) -> Option<Arc<RequestInfo>> {
    if let Some(render) = context.services().extension::<ServerRender>() {
        render.note_dynamic(context.id());
    }
    context.services().extension::<RequestInfo>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_query_round_trips_through_the_address() {
        #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        struct Search {
            q: String,
            page: u32,
            #[serde(default)]
            exact: bool,
            tag: Option<String>,
        }
        let search = Search { q: "café & co".into(), page: 2, exact: true, tag: None };
        let query = query_string(&search);
        assert_eq!(query, "exact=true&page=2&q=caf%C3%A9+%26+co");
        let request = RequestInfo::get(&format!("/search?{query}"));
        assert_eq!(request.query_as::<Search>().unwrap(), search);
        // Defaults for what is missing; a wrong type is an error.
        let request = RequestInfo::get("/search?q=x&page=1");
        assert_eq!(
            request.query_as::<Search>().unwrap(),
            Search { q: "x".into(), page: 1, exact: false, tag: None }
        );
        assert!(RequestInfo::get("/search?q=x&page=first").query_as::<Search>().is_err());
        // A number-looking value where text is wanted is still text.
        let request = RequestInfo::get("/search?q=42&page=1&tag=7");
        assert_eq!(request.query_as::<Search>().unwrap().q, "42");
    }

    #[test]
    fn query_parameters_are_decoded() {
        let request = RequestInfo::get("/search?q=caf%C3%A9+au+lait&empty&bad=%zz");
        assert_eq!(request.path, "/search");
        assert_eq!(request.query_param("q").as_deref(), Some("café au lait"));
        assert_eq!(request.query_param("empty").as_deref(), Some(""));
        assert_eq!(request.query_param("bad").as_deref(), Some("%zz"));
        assert_eq!(request.query_param("missing"), None);
    }
}
