//! `query!`: SQL checked against the schema at compile time (`PLAN.md`
//! Milestone 49).
//!
//! At expansion the macro applies the crate's `migrations/*.up.sql` to an
//! in-memory SQLite database and prepares the statement there. A misspelt
//! table or column, a syntax error, or the wrong number of parameters is a
//! compile error with SQLite's own message; the row type is inferred from
//! the result columns.

use std::collections::HashMap;
use std::path::PathBuf;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Expr, LitStr, Token};

struct Input {
    sql: LitStr,
    params: Vec<Expr>,
}

impl Parse for Input {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let sql: LitStr = input.parse()?;
        let params = if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            Punctuated::<Expr, Token![,]>::parse_terminated(input)?.into_iter().collect()
        } else {
            Vec::new()
        };
        Ok(Self { sql, params })
    }
}

/// A query checked against the crate's migrations; see the crate docs.
///
/// ```ignore
/// let users = query!("SELECT id, name FROM users WHERE id = ?", id).fetch_all(&connection)?;
/// println!("{}", users[0].name);
/// ```
#[proc_macro]
pub fn query(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as Input);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn migrations() -> Result<Vec<PathBuf>, String> {
    let root = std::env::var("CARGO_MANIFEST_DIR").map_err(|error| error.to_string())?;
    let directory = PathBuf::from(root).join("migrations");
    let mut files = std::fs::read_dir(&directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.to_string_lossy().ends_with(".up.sql"))
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

/// `NOT NULL` for each column name, across tables (a name in several
/// tables is nullable unless it is `NOT NULL` in all of them).
fn not_null(connection: &rusqlite::Connection) -> rusqlite::Result<HashMap<String, bool>> {
    let mut tables = connection.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?;
    let names = tables
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut columns: HashMap<String, bool> = HashMap::new();
    for table in names {
        let mut info = connection.prepare(&format!("PRAGMA table_info(\"{table}\")"))?;
        let rows = info.query_map([], |row| {
            let primary: i64 = row.get(5)?;
            let kind: String = row.get(2)?;
            let integer_key = primary > 0 && kind.eq_ignore_ascii_case("INTEGER");
            Ok((row.get::<_, String>(1)?, row.get::<_, i64>(3)? != 0 || integer_key))
        })?;
        for row in rows {
            let (name, required) = row?;
            columns.entry(name).and_modify(|all| *all = *all && required).or_insert(required);
        }
    }
    Ok(columns)
}

fn rust_type(declared: Option<&str>) -> proc_macro2::TokenStream {
    let declared = declared.unwrap_or("").to_ascii_uppercase();
    if declared.contains("INT") {
        quote!(i64)
    } else if declared.contains("CHAR") || declared.contains("TEXT") || declared.contains("CLOB") {
        quote!(::std::string::String)
    } else if declared.contains("REAL") || declared.contains("FLOA") || declared.contains("DOUB") {
        quote!(f64)
    } else if declared.contains("BOOL") {
        quote!(bool)
    } else if declared.contains("BLOB") {
        quote!(::std::vec::Vec<u8>)
    } else {
        // An expression (`COUNT(*)`, `MAX(x)`) has no declared type.
        quote!(::rustnative_server::db::rusqlite::types::Value)
    }
}

fn expand(input: &Input) -> syn::Result<proc_macro2::TokenStream> {
    let span = input.sql.span();
    let fail = |message: String| syn::Error::new(span, message);
    let sql = input.sql.value();
    let files = migrations()
        .map_err(|error| fail(format!("query!: cannot read the migrations: {error}")))?;
    let connection =
        rusqlite::Connection::open_in_memory().map_err(|error| fail(error.to_string()))?;
    for file in &files {
        let text = std::fs::read_to_string(file).map_err(|error| fail(error.to_string()))?;
        connection.execute_batch(&text).map_err(|error| {
            fail(format!("query!: migration {} fails: {error}", file.display()))
        })?;
    }
    let statement = connection.prepare(&sql).map_err(|error| fail(format!("query!: {error}")))?;
    let expected = statement.parameter_count();
    if expected != input.params.len() {
        return Err(fail(format!(
            "query!: the statement takes {expected} parameter(s), {} given",
            input.params.len()
        )));
    }
    let required = not_null(&connection).map_err(|error| fail(error.to_string()))?;
    let columns = statement.columns();

    // Rebuild when a migration changes.
    let tracked = files.iter().map(|file| {
        let path = file.to_string_lossy().into_owned();
        quote!(
            const _: &str = include_str!(#path);
        )
    });
    let params = input.params.iter().map(|param| quote!(::rustnative_server::db::param(&(#param))));

    if columns.is_empty() {
        return Ok(quote! {{
            #(#tracked)*
            ::rustnative_server::db::Query::<()>::new(#sql, vec![#(#params),*], |_| Ok(()))
        }});
    }

    let mut fields = Vec::new();
    let mut reads = Vec::new();
    for (index, column) in columns.iter().enumerate() {
        let name = column.name();
        let field = format_ident!("{}", sanitize(name), span = Span::call_site());
        let base = rust_type(column.decl_type());
        let is_value = column.decl_type().is_none();
        let kind = if is_value || required.get(name).copied().unwrap_or(false) {
            base
        } else {
            quote!(::std::option::Option<#base>)
        };
        fields.push(quote!(pub #field: #kind));
        reads.push(quote!(#field: row.get(#index)?));
    }
    Ok(quote! {{
        #(#tracked)*
        #[derive(Debug, Clone, PartialEq, ::rustnative_server::serde::Serialize)]
        #[serde(crate = "::rustnative_server::serde")]
        struct Row { #(#fields),* }
        ::rustnative_server::db::Query::<Row>::new(#sql, vec![#(#params),*], |row| Ok(Row { #(#reads),* }))
    }})
}

fn sanitize(name: &str) -> String {
    let cleaned: String =
        name.chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() { character.to_ascii_lowercase() } else { '_' }
            })
            .collect();
    if cleaned.chars().next().is_none_or(|first| first.is_ascii_digit()) {
        format!("c_{cleaned}")
    } else {
        cleaned
    }
}
