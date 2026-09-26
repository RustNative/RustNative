//! The interface description (`.ril`): one typed description of a
//! library's surface, with ownership and threading annotated, from which
//! every host language's binding is generated (`PLAN.md` Milestone 40,
//! concept `C66`).
//!
//! ```text
//! /// A counter a host application drives.
//! library counter;
//!
//! service Counter [thread = owner] {
//!     new(start: u32);
//!     fn increment(&mut self) -> u32;
//!     fn value(&self) -> u32 [thread = any];
//!     fn name(&self) -> string;
//!     fn rename(&mut self, name: string);
//!     event changed(value: u32);
//! }
//! ```
//!
//! - **types**: `bool`, `i32`, `u32`, `i64`, `u64`, `f64`, `string` (UTF-8);
//! - **ownership**: a service instance is owned by the host through an
//!   opaque handle it frees; a `string` argument is borrowed for the call;
//!   a returned `string` is owned by the caller, who frees it with the
//!   library's `string_free`;
//! - **threading**: `[thread = owner]` (the default) — callable only on the
//!   thread that created the instance, checked on every call;
//!   `[thread = any]` — callable from any thread (the instance is
//!   serialized by the handle table);
//! - **events**: callbacks the host registers, invoked on the thread that
//!   caused them.

use std::fmt;

/// A parse error, with its 1-based line and column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdlError {
    /// What is wrong.
    pub message: String,
    /// 1-based line.
    pub line: usize,
    /// 1-based column.
    pub column: usize,
}

impl fmt::Display for IdlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for IdlError {}

/// A value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Type {
    /// `bool`.
    Bool,
    /// `i32`.
    I32,
    /// `u32`.
    U32,
    /// `i64`.
    I64,
    /// `u64`.
    U64,
    /// `f64`.
    F64,
    /// `string`: UTF-8.
    String,
}

impl Type {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "bool" => Self::Bool,
            "i32" => Self::I32,
            "u32" => Self::U32,
            "i64" => Self::I64,
            "u64" => Self::U64,
            "f64" => Self::F64,
            "string" => Self::String,
            _ => return None,
        })
    }

    /// Its IDL spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::I32 => "i32",
            Self::U32 => "u32",
            Self::I64 => "i64",
            Self::U64 => "u64",
            Self::F64 => "f64",
            Self::String => "string",
        }
    }
}

/// Which threads may call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Threading {
    /// Only the thread that created the instance.
    #[default]
    Owner,
    /// Any thread.
    Any,
}

/// A named, typed parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// Its name.
    pub name: String,
    /// Its type.
    pub ty: Type,
}

/// How a method receives the instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Receiver {
    /// `&self`.
    Shared,
    /// `&mut self`.
    Exclusive,
}

/// A method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    /// Its name.
    pub name: String,
    /// Its receiver.
    pub receiver: Receiver,
    /// Its parameters.
    pub params: Vec<Param>,
    /// Its return type, if any.
    pub returns: Option<Type>,
    /// Which threads may call it.
    pub threading: Threading,
    /// Its documentation.
    pub doc: Vec<String>,
}

/// An event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Its name.
    pub name: String,
    /// What it carries.
    pub params: Vec<Param>,
    /// Its documentation.
    pub doc: Vec<String>,
}

/// A service: a type the host creates, calls, and frees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    /// Its name.
    pub name: String,
    /// Its default threading.
    pub threading: Threading,
    /// The constructor's parameters.
    pub constructor: Vec<Param>,
    /// Its methods.
    pub methods: Vec<Method>,
    /// Its events.
    pub events: Vec<Event>,
    /// Its documentation.
    pub doc: Vec<String>,
}

/// A whole description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Idl {
    /// The library's name: the prefix of every C symbol.
    pub library: String,
    /// Its services.
    pub services: Vec<Service>,
}

/// Parses a description.
///
/// # Errors
///
/// The first thing that is not the grammar, at its position.
pub fn parse_idl(source: &str) -> Result<Idl, IdlError> {
    Parser::new(source).idl()
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Punct(char),
    Arrow,
    Doc(String),
}

struct Parser {
    tokens: Vec<(Token, usize, usize)>,
    position: usize,
    end: (usize, usize),
}

fn is_ident(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Parser {
    fn new(source: &str) -> Self {
        let mut tokens = Vec::new();
        let mut line = 1;
        for text in source.lines() {
            let mut chars = text.char_indices().peekable();
            while let Some((index, c)) = chars.next() {
                let column = text[..index].chars().count() + 1;
                if c.is_whitespace() {
                    continue;
                }
                if text[index..].starts_with("///") {
                    tokens.push((Token::Doc(text[index + 3..].trim().to_owned()), line, column));
                    break;
                }
                if text[index..].starts_with("//") {
                    break;
                }
                if text[index..].starts_with("->") {
                    chars.next();
                    tokens.push((Token::Arrow, line, column));
                    continue;
                }
                if c.is_ascii_alphanumeric() || c == '_' {
                    let mut end = index + c.len_utf8();
                    while let Some(&(next, n)) = chars.peek() {
                        if n.is_ascii_alphanumeric() || n == '_' {
                            end = next + n.len_utf8();
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    tokens.push((Token::Ident(text[index..end].to_owned()), line, column));
                    continue;
                }
                tokens.push((Token::Punct(c), line, column));
            }
            line += 1;
        }
        Self { tokens, position: 0, end: (line, 1) }
    }

    fn here(&self) -> (usize, usize) {
        self.tokens.get(self.position).map_or(self.end, |(_, line, column)| (*line, *column))
    }

    fn error<T>(&self, message: impl Into<String>) -> Result<T, IdlError> {
        let (line, column) = self.here();
        Err(IdlError { message: message.into(), line, column })
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position).map(|(token, _, _)| token)
    }

    fn docs(&mut self) -> Vec<String> {
        let mut docs = Vec::new();
        while let Some(Token::Doc(text)) = self.peek() {
            docs.push(text.clone());
            self.position += 1;
        }
        docs
    }

    fn ident(&mut self, what: &str) -> Result<String, IdlError> {
        match self.peek() {
            Some(Token::Ident(name)) if is_ident(name) => {
                let name = name.clone();
                self.position += 1;
                Ok(name)
            }
            _ => self.error(format!("expected {what}")),
        }
    }

    fn keyword(&mut self, keyword: &str) -> bool {
        if matches!(self.peek(), Some(Token::Ident(name)) if name == keyword) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn punct(&mut self, c: char) -> Result<(), IdlError> {
        if self.peek() == Some(&Token::Punct(c)) {
            self.position += 1;
            Ok(())
        } else {
            self.error(format!("expected `{c}`"))
        }
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(&Token::Punct(c)) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn idl(mut self) -> Result<Idl, IdlError> {
        self.docs();
        if !self.keyword("library") {
            return self.error("a description starts with `library <name>;`");
        }
        let library = self.ident("the library name")?;
        self.punct(';')?;
        let mut services = Vec::new();
        loop {
            let doc = self.docs();
            if self.peek().is_none() {
                break;
            }
            if !self.keyword("service") {
                return self.error("expected `service`");
            }
            services.push(self.service(doc)?);
        }
        let mut names: Vec<&str> = services.iter().map(|service| service.name.as_str()).collect();
        names.sort_unstable();
        if names.windows(2).any(|pair| pair[0] == pair[1]) {
            return self.error("two services have the same name");
        }
        Ok(Idl { library, services })
    }

    fn threading(&mut self) -> Result<Option<Threading>, IdlError> {
        if !self.eat('[') {
            return Ok(None);
        }
        if !self.keyword("thread") {
            return self.error("the only annotation is `[thread = owner|any]`");
        }
        self.punct('=')?;
        let threading = match self.ident("`owner` or `any`")?.as_str() {
            "owner" => Threading::Owner,
            "any" => Threading::Any,
            _ => return self.error("expected `owner` or `any`"),
        };
        self.punct(']')?;
        Ok(Some(threading))
    }

    fn params(&mut self) -> Result<Vec<Param>, IdlError> {
        let mut params = Vec::new();
        while !self.eat(')') {
            if !params.is_empty() {
                self.punct(',')?;
                if self.eat(')') {
                    break;
                }
            }
            let name = self.ident("a parameter name")?;
            self.punct(':')?;
            let ty = self.ty()?;
            if params.iter().any(|param: &Param| param.name == name) {
                return self.error(format!("`{name}` is declared twice"));
            }
            params.push(Param { name, ty });
        }
        Ok(params)
    }

    fn ty(&mut self) -> Result<Type, IdlError> {
        let name = self.ident("a type")?;
        if let Some(ty) = Type::parse(&name) {
            return Ok(ty);
        }
        self.position -= 1;
        self.error(format!("`{name}` is not a type: bool, i32, u32, i64, u64, f64, string"))
    }

    fn service(&mut self, doc: Vec<String>) -> Result<Service, IdlError> {
        let name = self.ident("the service name")?;
        let threading = self.threading()?.unwrap_or_default();
        self.punct('{')?;
        let mut service = Service {
            name,
            threading,
            constructor: Vec::new(),
            methods: Vec::new(),
            events: Vec::new(),
            doc,
        };
        let mut has_constructor = false;
        loop {
            let doc = self.docs();
            if self.eat('}') {
                break;
            }
            if self.keyword("new") {
                if has_constructor {
                    return self.error("a service has one constructor");
                }
                has_constructor = true;
                self.punct('(')?;
                service.constructor = self.params()?;
                self.punct(';')?;
            } else if self.keyword("fn") {
                let name = self.ident("a method name")?;
                if name == "free" || name.starts_with("on_") {
                    return self.error(format!("`{name}` is reserved for the generated binding"));
                }
                self.punct('(')?;
                self.punct('&')?;
                let receiver =
                    if self.keyword("mut") { Receiver::Exclusive } else { Receiver::Shared };
                if !self.keyword("self") {
                    return self.error("a method takes `&self` or `&mut self` first");
                }
                let params = if self.eat(',') {
                    self.params()?
                } else {
                    self.punct(')')?;
                    Vec::new()
                };
                let returns = if self.peek() == Some(&Token::Arrow) {
                    self.position += 1;
                    Some(self.ty()?)
                } else {
                    None
                };
                let threading = self.threading()?.unwrap_or(service.threading);
                self.punct(';')?;
                if service.methods.iter().any(|method| method.name == name) {
                    return self.error(format!("`{name}` is declared twice"));
                }
                service.methods.push(Method { name, receiver, params, returns, threading, doc });
            } else if self.keyword("event") {
                let name = self.ident("an event name")?;
                self.punct('(')?;
                let params = self.params()?;
                self.punct(';')?;
                service.events.push(Event { name, params, doc });
            } else {
                return self.error("expected `new`, `fn`, `event`, or `}`");
            }
        }
        if !has_constructor {
            return self.error(format!("service `{}` has no `new(…)` constructor", service.name));
        }
        Ok(service)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const COUNTER: &str = "/// A counter.\nlibrary counter;\n\n/// Counts.\nservice Counter {\n    new(start: u32);\n    /// Adds one.\n    fn increment(&mut self) -> u32;\n    fn value(&self) -> u32 [thread = any];\n    fn name(&self) -> string;\n    fn rename(&mut self, name: string);\n    event changed(value: u32);\n}\n";

    #[test]
    fn a_description_parses() {
        let idl = parse_idl(COUNTER).unwrap();
        assert_eq!(idl.library, "counter");
        let counter = &idl.services[0];
        assert_eq!(counter.constructor, vec![Param { name: "start".into(), ty: Type::U32 }]);
        assert_eq!(counter.methods.len(), 4);
        assert_eq!(counter.methods[0].receiver, Receiver::Exclusive);
        assert_eq!(counter.methods[0].doc, vec!["Adds one."]);
        assert_eq!(counter.methods[1].threading, Threading::Any);
        assert_eq!(counter.methods[2].returns, Some(Type::String));
        assert_eq!(counter.events[0].name, "changed");
    }

    #[test]
    fn mistakes_are_reported_where_they_are() {
        let error =
            parse_idl("library x;\nservice S {\n    new();\n    fn f(&self) -> float;\n}\n")
                .unwrap_err();
        assert_eq!((error.line, error.column), (4, 20));
        assert!(error.message.contains("not a type"), "{error}");
        let error = parse_idl("library x;\nservice S {\n    fn f(&self);\n}\n").unwrap_err();
        assert!(error.message.contains("no `new(…)`"), "{error}");
    }
}
