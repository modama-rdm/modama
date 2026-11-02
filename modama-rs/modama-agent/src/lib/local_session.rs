//! The caller's sign-in with the local API, as a browser-style session cookie.
//!
//! The cookie holds the caller's [`UpstreamSession`] (mgmtinterface-api's own session cookie,
//! percent-encoded), so the agent keeps no state per user: it forwards what the caller brings,
//! and hands back what the management API re-issues. The GUI and the HTML pages use the same
//! cookie.
//!
//! The cookie is `HttpOnly` and `SameSite=Strict`, and has no expiry, so it ends with the
//! browser or GUI. It can't be `Secure`: the agent serves plain http on localhost.

use axum::{
    extract::FromRequestParts,
    http::{HeaderValue, header, request::Parts},
    response::{IntoResponse, Response},
};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

use crate::{local_http_service::LocalAPIError, mgmt_api::UpstreamSession};

pub const SESSION_COOKIE: &str = "modama_agent_session";

/// The caller's session, from the `modama_agent_session` cookie; without one, the request is
/// rejected with [`LocalAPIError::NoSession`]
pub struct CallerSession(pub UpstreamSession);

impl<S: Send + Sync> FromRequestParts<S> for CallerSession {
    type Rejection = LocalAPIError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        session_from_headers(&parts.headers)
            .map(CallerSession)
            .ok_or(LocalAPIError::NoSession)
    }
}

/// The caller's session, if the request carries one
pub struct MaybeCallerSession(pub Option<UpstreamSession>);

impl<S: Send + Sync> FromRequestParts<S> for MaybeCallerSession {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(MaybeCallerSession(session_from_headers(&parts.headers)))
    }
}

fn session_from_headers(headers: &axum::http::HeaderMap) -> Option<UpstreamSession> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|cookie| cookie.trim().split_once('='))
        .find(|(name, _)| *name == SESSION_COOKIE)
        .and_then(|(_, value)| percent_decode_str(value).decode_utf8().ok())
        .filter(|token| !token.is_empty())
        .map(|token| UpstreamSession::from_token(&token))
}

/// `Set-Cookie` value that stores `session`
pub fn session_cookie(session: &UpstreamSession) -> HeaderValue {
    let value = utf8_percent_encode(session.token(), NON_ALPHANUMERIC);
    // only ASCII letters, digits and `%XX` escapes, so this is a valid header value
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}={value}; HttpOnly; SameSite=Strict; Path=/"
    ))
    .unwrap_or_else(|_| expired_session_cookie())
}

/// `Set-Cookie` value that removes the session
pub fn expired_session_cookie() -> HeaderValue {
    HeaderValue::from_static("modama_agent_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
}

/// `response`, storing the re-issued session if the management API re-issued it
pub fn with_refreshed_session(
    refreshed: Option<UpstreamSession>,
    response: impl IntoResponse,
) -> Response {
    let mut response = response.into_response();
    if let Some(session) = refreshed {
        response
            .headers_mut()
            .append(header::SET_COOKIE, session_cookie(&session));
    }
    response
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, header};

    use super::{SESSION_COOKIE, expired_session_cookie, session_cookie, session_from_headers};
    use crate::mgmt_api::UpstreamSession;

    #[test]
    fn the_cookie_round_trips_an_upstream_session() {
        let upstream = UpstreamSession::from_token("__Host-modama_session=a.b-c_d; other=x");
        let set_cookie = session_cookie(&upstream);
        let set_cookie = set_cookie.to_str().unwrap();
        assert!(set_cookie.ends_with("; HttpOnly; SameSite=Strict; Path=/"));

        let pair = set_cookie.split(';').next().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("unrelated=1; {pair}")).unwrap(),
        );
        assert_eq!(session_from_headers(&headers), Some(upstream));
    }

    #[test]
    fn no_or_an_emptied_cookie_is_no_session() {
        let mut headers = HeaderMap::new();
        assert_eq!(session_from_headers(&headers), None);
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("{SESSION_COOKIE}=")).unwrap(),
        );
        assert_eq!(session_from_headers(&headers), None);
        assert!(
            expired_session_cookie()
                .to_str()
                .unwrap()
                .contains("Max-Age=0")
        );
    }
}
