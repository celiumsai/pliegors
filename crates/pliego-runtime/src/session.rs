// SPDX-License-Identifier: Apache-2.0

use crate::{SameSitePolicy, SessionCookie, SessionCookiePolicy, SessionError, SessionToken};
use http::HeaderMap;
use http::header::{COOKIE, SET_COOKIE};

pub fn session_cookie_header(
    session: &SessionCookie,
) -> Result<(http::HeaderName, http::HeaderValue), SessionError> {
    let policy = session.policy();
    let seconds = i64::try_from(session.max_age().as_secs())
        .map_err(|_| SessionError::InvalidPolicy("cookie max age is too large".to_owned()))?;
    let value = header_value(policy, session.token().as_cookie_value(), seconds)?;
    Ok((SET_COOKIE, value))
}

pub fn expire_session_cookie_header(
    policy: &SessionCookiePolicy,
) -> Result<(http::HeaderName, http::HeaderValue), SessionError> {
    let value = header_value(policy, "", 0)?;
    Ok((SET_COOKIE, value))
}

pub fn read_session_token(
    headers: &HeaderMap,
    policy: &SessionCookiePolicy,
) -> Result<Option<SessionToken>, SessionError> {
    let mut found = None;
    for header in headers.get_all(COOKIE) {
        let header = header.to_str().map_err(|_| SessionError::InvalidToken)?;
        for item in header.split(';') {
            let (name, value) = parse_cookie_pair(item)?;
            if name != policy.name() {
                continue;
            }
            if found.is_some() {
                return Err(SessionError::InvalidToken);
            }
            found = Some(SessionToken::parse(value)?);
        }
    }
    Ok(found)
}

fn header_value(
    policy: &SessionCookiePolicy,
    token: &str,
    max_age_seconds: i64,
) -> Result<http::HeaderValue, SessionError> {
    let mut value = format!(
        "{}={token}; Path={}; Max-Age={max_age_seconds}; SameSite={}",
        policy.name(),
        policy.path_value(),
        same_site(policy.same_site_value())
    );
    if policy.is_secure() {
        value.push_str("; Secure");
    }
    if policy.is_http_only() {
        value.push_str("; HttpOnly");
    }
    http::HeaderValue::from_str(&value)
        .map_err(|_| SessionError::InvalidPolicy("cookie header is invalid".to_owned()))
}

fn parse_cookie_pair(input: &str) -> Result<(&str, &str), SessionError> {
    let input = input.trim();
    let (name, value) = input.split_once('=').ok_or(SessionError::InvalidToken)?;
    if name.is_empty()
        || !name.bytes().all(is_cookie_name_byte)
        || value
            .bytes()
            .any(|byte| byte <= 0x20 || byte == b';' || byte == 0x7f)
    {
        return Err(SessionError::InvalidToken);
    }
    Ok((name, value))
}

fn is_cookie_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn same_site(policy: SameSitePolicy) -> &'static str {
    match policy {
        SameSitePolicy::Strict => "Strict",
        SameSitePolicy::Lax => "Lax",
        SameSitePolicy::None => "None",
    }
}
