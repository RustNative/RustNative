//! Validation and forms (`PLAN.md` Milestone 47, `C36`): one schema, one
//! set of error messages, shared by the client and the server
//! (Milestone 49's server validates with the same [`Schema`]).
//!
//! - **Typed fields.** A [`Field<T>`] is a constant naming a field and its
//!   type; reading a validated value through it is checked by the compiler.
//! - **A changeset** holds the raw input, as typed, apart from the
//!   validated output: [`Changeset::validate`] casts every field and checks
//!   every rule, and returns either a [`Valid`] (typed values) or
//!   [`FieldErrors`] (per field).
//! - **Dirty tracking** against the initial values.
//! - **Storage constraint violations** reported by a database map back to
//!   the field they concern ([`Schema::constraint`]).
//! - **A submission lifecycle** ([`Form`], [`Submission`]).
//!
//! ```
//! use rustnative_data::{Changeset, Field, Rule, Schema};
//!
//! const EMAIL: Field<String> = Field::text("email");
//! const AGE: Field<i64> = Field::integer("age");
//!
//! let schema = Schema::new()
//!     .field(EMAIL, [Rule::Required, Rule::Email])
//!     .field(AGE, [Rule::Range(13, 130)])
//!     .constraint("users_email_key", EMAIL, "is already registered");
//!
//! let mut changes = Changeset::new(&schema);
//! changes.set(EMAIL, "ada@example.com");
//! changes.set(AGE, "12");
//! let errors = changes.validate().unwrap_err();
//! assert_eq!(errors.get("age").unwrap()[0].message, "must be between 13 and 130");
//!
//! changes.set(AGE, "36");
//! let valid = changes.validate().unwrap();
//! assert_eq!(valid.get(AGE), 36);
//! ```

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

/// The type a field casts its raw text to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Any text.
    Text,
    /// A whole number.
    Integer,
    /// `true` or `false` (a checkbox).
    Boolean,
}

/// A value a field can hold, cast from its raw text.
pub trait FieldValue: Sized {
    /// The kind of input.
    const KIND: Kind;
    /// Casts raw input; `None` when it is not a value of this type.
    fn cast(raw: &str) -> Option<Self>;
}

impl FieldValue for String {
    const KIND: Kind = Kind::Text;
    fn cast(raw: &str) -> Option<Self> {
        Some(raw.trim().to_owned())
    }
}

impl FieldValue for i64 {
    const KIND: Kind = Kind::Integer;
    fn cast(raw: &str) -> Option<Self> {
        raw.trim().parse().ok()
    }
}

impl FieldValue for bool {
    const KIND: Kind = Kind::Boolean;
    fn cast(raw: &str) -> Option<Self> {
        match raw.trim() {
            "true" | "on" | "1" => Some(true),
            "" | "false" | "off" | "0" => Some(false),
            _ => None,
        }
    }
}

/// A typed field: its name, and the type its value casts to.
pub struct Field<T> {
    name: &'static str,
    _value: PhantomData<fn() -> T>,
}

impl<T> Clone for Field<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Field<T> {}

impl<T> fmt::Debug for Field<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Field({})", self.name)
    }
}

impl<T> Field<T> {
    /// Its name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.name
    }
}

impl Field<String> {
    /// A text field.
    #[must_use]
    pub const fn text(name: &'static str) -> Self {
        Self { name, _value: PhantomData }
    }
}

impl Field<i64> {
    /// A whole-number field.
    #[must_use]
    pub const fn integer(name: &'static str) -> Self {
        Self { name, _value: PhantomData }
    }
}

impl Field<bool> {
    /// A yes-or-no field.
    #[must_use]
    pub const fn boolean(name: &'static str) -> Self {
        Self { name, _value: PhantomData }
    }
}

/// A rule a field's value must satisfy.
#[derive(Clone)]
pub enum Rule {
    /// It must not be empty.
    Required,
    /// Text at least this many characters long.
    MinLength(usize),
    /// Text at most this many characters long.
    MaxLength(usize),
    /// Text that looks like an email address.
    Email,
    /// A number within this inclusive range.
    Range(i64, i64),
    /// Text the function accepts; otherwise the message is the error.
    Custom(fn(&str) -> bool, &'static str),
}

impl fmt::Debug for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Required => f.write_str("Required"),
            Self::MinLength(n) => write!(f, "MinLength({n})"),
            Self::MaxLength(n) => write!(f, "MaxLength({n})"),
            Self::Email => f.write_str("Email"),
            Self::Range(low, high) => write!(f, "Range({low}, {high})"),
            Self::Custom(_, message) => write!(f, "Custom({message:?})"),
        }
    }
}

/// One problem with one field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldError {
    /// A stable code, for translating the message (`form-required`, …).
    pub code: String,
    /// The message, in the schema's language.
    pub message: String,
}

impl FieldError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_owned(), message: message.into() }
    }
}

/// Every field's problems, by field name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldErrors(BTreeMap<String, Vec<FieldError>>);

impl FieldErrors {
    /// `field`'s problems, if it has any.
    #[must_use]
    pub fn get(&self, field: &str) -> Option<&[FieldError]> {
        self.0.get(field).map(Vec::as_slice)
    }

    /// Whether no field has a problem.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Adds a problem to `field`.
    pub fn add(&mut self, field: &str, error: FieldError) {
        self.0.entry(field.to_owned()).or_default().push(error);
    }

    /// Every field with problems.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[FieldError])> {
        self.0.iter().map(|(field, errors)| (field.as_str(), errors.as_slice()))
    }
}

#[derive(Debug, Clone)]
struct Spec {
    name: &'static str,
    kind: Kind,
    rules: Vec<Rule>,
}

/// The fields of a form, their types, and their rules.
#[derive(Debug, Clone, Default)]
pub struct Schema {
    fields: Vec<Spec>,
    constraints: Vec<(String, &'static str, String)>,
}

impl Schema {
    /// A schema with no fields.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `field`, checked by `rules`.
    #[must_use]
    pub fn field<T: FieldValue>(
        mut self,
        field: Field<T>,
        rules: impl IntoIterator<Item = Rule>,
    ) -> Self {
        self.fields.push(Spec {
            name: field.name,
            kind: T::KIND,
            rules: rules.into_iter().collect(),
        });
        self
    }

    /// Maps the storage constraint `name` (a unique index, say), when
    /// violated, to an error on `field` with `message`.
    #[must_use]
    pub fn constraint<T>(
        mut self,
        name: impl Into<String>,
        field: Field<T>,
        message: impl Into<String>,
    ) -> Self {
        self.constraints.push((name.into(), field.name, message.into()));
        self
    }

    /// The field names, in order.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.fields.iter().map(|spec| spec.name)
    }

    /// Checks raw input, by field name — what a server receives.
    ///
    /// # Errors
    ///
    /// Every field's problems.
    pub fn check(&self, raw: &BTreeMap<String, String>) -> Result<Valid, FieldErrors> {
        let mut errors = FieldErrors::default();
        for spec in &self.fields {
            let value = raw.get(spec.name).map_or("", String::as_str);
            for error in check_field(spec, value) {
                errors.add(spec.name, error);
            }
        }
        if errors.is_empty() { Ok(Valid { values: raw.clone() }) } else { Err(errors) }
    }

    /// The field error a violation of storage constraint `name` means, if
    /// the schema maps it.
    #[must_use]
    pub fn constraint_error(&self, name: &str) -> Option<(&'static str, FieldError)> {
        self.constraints.iter().find(|(constraint, _, _)| constraint == name).map(
            |(_, field, message)| (*field, FieldError::new("form-constraint", message.clone())),
        )
    }
}

fn check_field(spec: &Spec, raw: &str) -> Vec<FieldError> {
    let text = raw.trim();
    let mut errors = Vec::new();
    if text.is_empty() {
        if spec.rules.iter().any(|rule| matches!(rule, Rule::Required)) {
            errors.push(FieldError::new("form-required", "is required"));
        }
        return errors;
    }
    let number = match spec.kind {
        Kind::Integer => {
            let Ok(number) = text.parse::<i64>() else {
                errors.push(FieldError::new("form-not-a-number", "must be a whole number"));
                return errors;
            };
            Some(number)
        }
        Kind::Boolean if bool::cast(text).is_none() => {
            errors.push(FieldError::new("form-not-a-choice", "must be yes or no"));
            return errors;
        }
        _ => None,
    };
    for rule in &spec.rules {
        let error = match rule {
            Rule::MinLength(min) if text.chars().count() < *min => Some(FieldError::new(
                "form-too-short",
                format!("must be at least {min} characters"),
            )),
            Rule::MaxLength(max) if text.chars().count() > *max => {
                Some(FieldError::new("form-too-long", format!("must be at most {max} characters")))
            }
            Rule::Email if !looks_like_email(text) => {
                Some(FieldError::new("form-email", "must be an email address"))
            }
            Rule::Range(low, high) if number.is_some_and(|n| n < *low || n > *high) => {
                Some(FieldError::new("form-range", format!("must be between {low} and {high}")))
            }
            Rule::Custom(accepts, message) if !accepts(text) => {
                Some(FieldError::new("form-custom", *message))
            }
            _ => None,
        };
        errors.extend(error);
    }
    errors
}

fn looks_like_email(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else { return false };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !text.contains(char::is_whitespace)
}

/// Validated output: every field cast to its type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Valid {
    values: BTreeMap<String, String>,
}

impl Valid {
    /// `field`'s value. A field not in the schema reads as its type's
    /// empty value.
    #[must_use]
    pub fn get<T: FieldValue + Default>(&self, field: Field<T>) -> T {
        self.values.get(field.name).and_then(|raw| T::cast(raw)).unwrap_or_default()
    }
}

/// Raw input to a [`Schema`], as the person types it, with its errors and
/// what changed; see the [module documentation](self).
#[derive(Debug, Clone)]
pub struct Changeset<'a> {
    schema: &'a Schema,
    initial: BTreeMap<String, String>,
    raw: BTreeMap<String, String>,
    errors: FieldErrors,
}

impl<'a> Changeset<'a> {
    /// An empty changeset over `schema`.
    #[must_use]
    pub fn new(schema: &'a Schema) -> Self {
        Self {
            schema,
            initial: BTreeMap::new(),
            raw: BTreeMap::new(),
            errors: FieldErrors::default(),
        }
    }

    /// A changeset whose fields start as `initial` — an edit form.
    #[must_use]
    pub fn with_initial(schema: &'a Schema, initial: BTreeMap<String, String>) -> Self {
        Self { schema, raw: initial.clone(), initial, errors: FieldErrors::default() }
    }

    /// Sets `field`'s raw input — what a text field's change event carries.
    pub fn set<T>(&mut self, field: Field<T>, raw: impl Into<String>) {
        self.raw.insert(field.name.to_owned(), raw.into());
    }

    /// `field`'s raw input — what the text field shows (two-way binding).
    #[must_use]
    pub fn raw<T>(&self, field: Field<T>) -> &str {
        self.raw.get(field.name).map_or("", String::as_str)
    }

    /// Whether any field differs from its initial value.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.schema.names().any(|name| self.raw.get(name) != self.initial.get(name))
    }

    /// Whether `field` differs from its initial value.
    #[must_use]
    pub fn field_dirty<T>(&self, field: Field<T>) -> bool {
        self.raw.get(field.name) != self.initial.get(field.name)
    }

    /// Checks every field.
    ///
    /// # Errors
    ///
    /// Every field's problems, also kept for [`Self::errors`].
    pub fn validate(&mut self) -> Result<Valid, FieldErrors> {
        let outcome = self.schema.check(&self.raw);
        self.errors = outcome.clone().err().unwrap_or_default();
        outcome
    }

    /// The problems the last validation found (and any storage violations
    /// added since).
    #[must_use]
    pub fn errors(&self) -> &FieldErrors {
        &self.errors
    }

    /// Records a storage constraint violation reported on saving, as an
    /// error on the field the schema maps it to. Returns whether it did.
    pub fn constraint_violated(&mut self, constraint: &str) -> bool {
        match self.schema.constraint_error(constraint) {
            Some((field, error)) => {
                self.errors.add(field, error);
                true
            }
            None => false,
        }
    }

    /// The raw input, by field name — what a client sends to a server that
    /// checks it with the same schema.
    #[must_use]
    pub fn raw_values(&self) -> &BTreeMap<String, String> {
        &self.raw
    }
}

/// Where a form's submission is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Submission {
    /// Not submitted since the last edit.
    #[default]
    Idle,
    /// Sent; waiting for the answer.
    Submitting,
    /// Accepted.
    Succeeded,
    /// Not accepted, for this reason.
    Failed(String),
}

/// A changeset with a submission lifecycle.
#[derive(Debug, Clone)]
pub struct Form<'a> {
    /// The input.
    pub changes: Changeset<'a>,
    submission: Submission,
}

impl<'a> Form<'a> {
    /// A form over `schema`.
    #[must_use]
    pub fn new(schema: &'a Schema) -> Self {
        Self { changes: Changeset::new(schema), submission: Submission::Idle }
    }

    /// Where the submission is.
    #[must_use]
    pub fn submission(&self) -> &Submission {
        &self.submission
    }

    /// Validates and, when valid, marks the form submitting and returns the
    /// output to send. `None` while a submission is already under way.
    pub fn submit(&mut self) -> Option<Result<Valid, FieldErrors>> {
        if self.submission == Submission::Submitting {
            return None;
        }
        let outcome = self.changes.validate();
        if outcome.is_ok() {
            self.submission = Submission::Submitting;
        }
        Some(outcome)
    }

    /// Records the server's answer. A constraint violation it names lands on
    /// its field.
    pub fn finish(&mut self, outcome: Result<(), SubmitError>) {
        self.submission = match outcome {
            Ok(()) => Submission::Succeeded,
            Err(SubmitError::Constraint(name)) => {
                self.changes.constraint_violated(&name);
                Submission::Failed(format!("{name} violated"))
            }
            Err(SubmitError::Fields(errors)) => {
                self.changes.errors = errors;
                Submission::Failed("the server rejected some fields".into())
            }
            Err(SubmitError::Other(message)) => Submission::Failed(message),
        };
    }
}

/// Why a submission failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubmitError {
    /// The server's storage refused it by this constraint.
    Constraint(String),
    /// The server's check of the same schema found these problems.
    Fields(FieldErrors),
    /// Anything else.
    Other(String),
}
