//! Data-layer authorization (`C39`): row policies declared next to the
//! schema, enforced for every read through them — queries and
//! subscriptions alike — and tested against fixtures.
//!
//! A policy gives, for a principal, the SQL condition a row of its table
//! must meet. [`RowPolicies::select`] wraps a query of the table in that
//! condition, so a read cannot forget it; a table with no policy is not
//! readable through them at all.
//!
//! ```
//! use rustnative_server::db::policy::RowPolicies;
//!
//! struct User { id: i64, admin: bool }
//!
//! let policies = RowPolicies::<User>::new()
//!     .table("notes", |user| {
//!         if user.admin { ("1 = 1".into(), vec![]) } else { ("owner = ?".into(), vec![user.id.into()]) }
//!     });
//! let (sql, params) = policies.select(&User { id: 7, admin: false }, "notes", "id, title").unwrap();
//! assert_eq!(sql, "SELECT id, title FROM notes WHERE (owner = ?)");
//! assert_eq!(params.len(), 1);
//! assert!(policies.select(&User { id: 7, admin: false }, "secrets", "*").is_none());
//! ```

use std::collections::BTreeMap;

use rusqlite::types::Value;

type Rule<P> = Box<dyn Fn(&P) -> (String, Vec<Value>) + Send + Sync>;

/// Row policies for principals of type `P`.
pub struct RowPolicies<P> {
    rules: BTreeMap<String, Rule<P>>,
}

impl<P> Default for RowPolicies<P> {
    fn default() -> Self {
        Self { rules: BTreeMap::new() }
    }
}

impl<P> RowPolicies<P> {
    /// No tables readable yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The rule for `table`: the condition, with its parameters.
    #[must_use]
    pub fn table(
        mut self,
        table: &str,
        rule: impl Fn(&P) -> (String, Vec<Value>) + Send + Sync + 'static,
    ) -> Self {
        self.rules.insert(table.to_owned(), Box::new(rule));
        self
    }

    /// `SELECT columns FROM table` limited to the rows `principal` may
    /// read, or `None` for a table with no policy.
    #[must_use]
    pub fn select(
        &self,
        principal: &P,
        table: &str,
        columns: &str,
    ) -> Option<(String, Vec<Value>)> {
        let (condition, params) = self.rules.get(table)?(principal);
        Some((format!("SELECT {columns} FROM {table} WHERE ({condition})"), params))
    }

    /// Whether `principal` may read the row of `table` with `rowid` — the
    /// check a subscription makes before sending a changed row.
    ///
    /// # Errors
    ///
    /// SQLite refused.
    pub fn may_read(
        &self,
        connection: &rusqlite::Connection,
        principal: &P,
        table: &str,
        rowid: i64,
    ) -> rusqlite::Result<bool> {
        let Some((sql, mut params)) = self.select(principal, table, "1") else { return Ok(false) };
        params.push(Value::Integer(rowid));
        connection
            .prepare(&format!("{sql} AND rowid = ?"))?
            .exists(rusqlite::params_from_iter(params.iter()))
    }

    /// The fixture harness: runs `principal`'s read of `table` against
    /// `connection` (loaded with fixture rows) and returns the rows'
    /// `column` values, for a test to compare with what it should see.
    ///
    /// # Errors
    ///
    /// SQLite refused.
    pub fn visible(
        &self,
        connection: &rusqlite::Connection,
        principal: &P,
        table: &str,
        column: &str,
    ) -> rusqlite::Result<Vec<Value>> {
        let Some((sql, params)) = self.select(principal, table, column) else {
            return Ok(Vec::new());
        };
        let mut statement = connection.prepare(&format!("{sql} ORDER BY rowid"))?;
        let rows =
            statement.query_map(rusqlite::params_from_iter(params.iter()), |row| row.get(0))?;
        rows.collect()
    }
}
