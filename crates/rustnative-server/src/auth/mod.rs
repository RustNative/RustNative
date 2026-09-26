//! Authentication and authorization.
//!
//! - [`session`]: sessions in an encrypted, authenticated cookie.
//! - [`password`]: Argon2id password hashes.
//! - [`token`]: signed bearer tokens.
//! - [`passkey`]: `WebAuthn` passkey registration and sign-in (`C52-2`).
//! - [`oauth`]: provider federation, authorization code with PKCE.
//! - Policies: who may use a route is part of the route's type
//!   ([`crate::MethodRouter::public`], [`crate::MethodRouter::signed_in`],
//!   [`crate::MethodRouter::authorized`]); a route that says nothing does
//!   not compile.

pub mod oauth;
pub mod passkey;
pub mod password;
pub mod session;
pub mod token;

use std::marker::PhantomData;
use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::handler::{Guarded, MethodRouter, Unguarded};
use crate::request::{FromRequest, RequestContext};
use crate::response::ServerError;

/// Who is making the request, as the application models people.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal<P>(pub P);

impl<P: Clone + Send + Sync + 'static> FromRequest for Principal<P> {
    fn from_request(request: &RequestContext) -> Result<Self, ServerError> {
        request.value::<Self>().cloned().ok_or_else(ServerError::unauthorized)
    }
}

/// A rule for who may do something, checked before the handler runs.
pub trait Policy<P>: 'static {
    /// Its name, in the route listing and the API schema.
    const NAME: &'static str;

    /// Whether `principal` may make `request`.
    fn allows(principal: &P, request: &RequestContext) -> bool;
}

/// Sets up authentication: finds the principal of each request — from the
/// session's signed-in value, or from a bearer token — and attaches it as
/// [`Principal<P>`].
pub struct Authentication<P> {
    tokens: Option<token::TokenSigner>,
    principal: PhantomData<fn() -> P>,
}

/// The session key the signed-in principal is kept under.
pub const PRINCIPAL_KEY: &str = "principal";

impl<P: Serialize + DeserializeOwned + Clone + Send + Sync + 'static> Authentication<P> {
    /// Principals from the session.
    #[must_use]
    pub const fn sessions() -> Self {
        Self { tokens: None, principal: PhantomData }
    }

    /// Also accepts bearer tokens `signer` issued.
    #[must_use]
    pub fn tokens(mut self, signer: token::TokenSigner) -> Self {
        self.tokens = Some(signer);
        self
    }

    /// The middleware (see `ServerApp::before`); install it after the
    /// session middleware.
    #[must_use]
    pub fn middleware(self) -> crate::app::Before {
        Arc::new(move |request: &mut RequestContext| {
            let from_token = request
                .header("authorization")
                .and_then(|value| value.strip_prefix("Bearer "))
                .map(str::to_owned);
            let principal = match (from_token, &self.tokens) {
                (Some(token), Some(signer)) => {
                    let claims = signer.verify(&token).ok_or_else(ServerError::unauthorized)?;
                    Some(
                        serde_json::from_str::<P>(&claims.subject)
                            .map_err(|_| ServerError::unauthorized())?,
                    )
                }
                _ => request
                    .value::<session::Session>()
                    .and_then(|session| session.get::<P>(PRINCIPAL_KEY)),
            };
            if let Some(principal) = principal {
                request.insert(Principal(principal));
            }
            Ok(())
        })
    }
}

impl MethodRouter<Unguarded> {
    /// Only signed-in principals of type `P` may use this route (`401`
    /// otherwise).
    #[must_use]
    pub fn signed_in<P: Clone + Send + Sync + 'static>(self) -> MethodRouter<Guarded> {
        let gate: crate::handler::Gate = Arc::new(|request: &mut RequestContext| {
            request.value::<Principal<P>>().map(|_| ()).ok_or_else(ServerError::unauthorized)
        });
        self.guarded(Some(gate), "signed in")
    }

    /// Only principals `Pol` allows may use this route: `401` when no one
    /// is signed in, `403` when the policy refuses.
    #[must_use]
    pub fn authorized<P: Clone + Send + Sync + 'static, Pol: Policy<P>>(
        self,
    ) -> MethodRouter<Guarded> {
        let gate: crate::handler::Gate = Arc::new(|request: &mut RequestContext| {
            let principal =
                request.value::<Principal<P>>().cloned().ok_or_else(ServerError::unauthorized)?;
            if Pol::allows(&principal.0, request) { Ok(()) } else { Err(ServerError::forbidden()) }
        });
        self.guarded(Some(gate), Pol::NAME)
    }
}
