//! The sign-in that a request to the agent's local HTTP API carries, as a cookie. The HTTP
//! client that sent the request may be a browser showing the agent's pages, the agent GUI or
//! another program; all use the same cookie.
//!
//! The cookie holds the client's [`SignIn`] (mgmtinterface-api's own session cookie,
//! percent-encoded), so the agent keeps no state per user: it forwards what the client brings,
//! and hands back what the management API renews.
//!
//! The cookie is `HttpOnly` and `SameSite=Strict`, and has no expiry, so it ends with the
//! browser or GUI. It can't be `Secure`: the agent serves plain http on localhost.

use axum::{
    extract::FromRequestParts,
    http::{HeaderValue, header, request::Parts},
    response::{IntoResponse, Response},
};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

use crate::{local_http_service::LocalAPIError, mgmt_api::SignIn};

pub const SIGN_IN_COOKIE: &str = "modama_agent_session";

/// The sign-in the request carries, in the `modama_agent_session` cookie; without one, the
/// request is rejected with [`LocalAPIError::NotSignedIn`]
pub struct ClientSignIn(pub SignIn);

impl<S: Send + Sync> FromRequestParts<S> for ClientSignIn {
    type Rejection = LocalAPIError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        sign_in_from_headers(&parts.headers)
            .map(ClientSignIn)
            .ok_or(LocalAPIError::NotSignedIn)
    }
}

/// The sign-in the request carries, if any
pub struct MaybeClientSignIn(pub Option<SignIn>);

impl<S: Send + Sync> FromRequestParts<S> for MaybeClientSignIn {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(MaybeClientSignIn(sign_in_from_headers(&parts.headers)))
    }
}

fn sign_in_from_headers(headers: &axum::http::HeaderMap) -> Option<SignIn> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|cookie| cookie.trim().split_once('='))
        .find(|(name, _)| *name == SIGN_IN_COOKIE)
        .and_then(|(_, value)| percent_decode_str(value).decode_utf8().ok())
        .filter(|cookie| !cookie.is_empty())
        .map(|cookie| SignIn::from_cookie(&cookie))
}

/// `Set-Cookie` value that stores `sign_in`
pub fn sign_in_cookie(sign_in: &SignIn) -> HeaderValue {
    let value = utf8_percent_encode(sign_in.cookie(), NON_ALPHANUMERIC);
    // only ASCII letters, digits and `%XX` escapes, so this is a valid header value
    HeaderValue::from_str(&format!(
        "{SIGN_IN_COOKIE}={value}; HttpOnly; SameSite=Strict; Path=/"
    ))
    .unwrap_or_else(|_| expired_sign_in_cookie())
}

/// `Set-Cookie` value that removes the sign-in
pub fn expired_sign_in_cookie() -> HeaderValue {
    HeaderValue::from_static("modama_agent_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
}

impl SignIn {
    /// `response`, with the cookie that stores this sign-in if the client doesn't have it yet
    pub fn respond(&self, response: impl IntoResponse) -> Response {
        let mut response = response.into_response();
        if self.changed() {
            response
                .headers_mut()
                .append(header::SET_COOKIE, sign_in_cookie(self));
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, header};

    use super::{SIGN_IN_COOKIE, expired_sign_in_cookie, sign_in_cookie, sign_in_from_headers};
    use crate::mgmt_api::SignIn;

    #[test]
    fn the_cookie_round_trips_a_sign_in() {
        let sign_in = SignIn::from_cookie("__Host-modama_session=a.b-c_d; other=x");
        let set_cookie = sign_in_cookie(&sign_in);
        let set_cookie = set_cookie.to_str().unwrap();
        assert!(set_cookie.ends_with("; HttpOnly; SameSite=Strict; Path=/"));

        let pair = set_cookie.split(';').next().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("unrelated=1; {pair}")).unwrap(),
        );
        assert_eq!(sign_in_from_headers(&headers), Some(sign_in));
    }

    #[test]
    fn no_or_an_emptied_cookie_is_no_sign_in() {
        let mut headers = HeaderMap::new();
        assert_eq!(sign_in_from_headers(&headers), None);
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("{SIGN_IN_COOKIE}=")).unwrap(),
        );
        assert_eq!(sign_in_from_headers(&headers), None);
        assert!(
            expired_sign_in_cookie()
                .to_str()
                .unwrap()
                .contains("Max-Age=0")
        );
    }
}
