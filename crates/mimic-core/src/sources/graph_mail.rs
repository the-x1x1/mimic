//! Sending a reply through Microsoft Graph (0.10.0-alpha.94), for a Microsoft
//! 365 mailbox whose organisation turned off sending from apps by SMTP — the
//! security defaults most new organisations have turn it off — so its server
//! refuses the sign-in (`smtp::turned_off_for_apps`). The message is the one
//! SMTP would have carried, byte for byte (`outgoing`), handed to Graph's
//! `sendMail` as MIME (base64, `Content-Type: text/plain`); Exchange keeps it
//! in Sent Items as it keeps one sent by SMTP. Graph answers 202 with nothing
//! that says which message it made, so the Message-ID it went under is known
//! only once its copy is read back from the Sent folder
//! (`Db::own_sends_unread`).

use std::time::Duration;

use serde_json::Value;

use super::transcript::note;

/// Where a message is handed to Graph to send as the signed-in user.
pub const SEND_MAIL: &str = "https://graph.microsoft.com/v1.0/me/sendMail";
/// What a reply sent this way is marked as having gone through
/// (`outbox.via`).
pub const VIA: &str = "graph";

const TIMEOUT: Duration = Duration::from_secs(60);

/// Why a message didn't go through Graph — or might have.
#[derive(Debug, thiserror::Error)]
pub enum GraphSendError {
    #[error("Microsoft turned down the sign-in for sending through Microsoft 365 ({0}). Sign in to this mailbox again under Your mail.")]
    SignIn(String),
    #[error(
        "Microsoft 365 wouldn't let me send as you ({0}). Your organization may not allow apps to send mail for you."
    )]
    Forbidden(String),
    #[error("Microsoft 365 didn't send it: {0}")]
    Refused(String),
    #[error("Microsoft 365 asked me to slow down, so it wasn't sent. Try again in a minute.")]
    RateLimited,
    #[error("I couldn't reach Microsoft 365 to send, so it wasn't sent: {0}")]
    Network(String),
    #[error("I lost Microsoft 365's answer as it was being sent ({0}), so it may or may not have gone. Look in your Sent folder before sending it again.")]
    Unsure(String),
}

impl GraphSendError {
    /// Whether the message may have reached Microsoft 365. Only then is it
    /// unsafe to send it again.
    pub fn may_have_gone(&self) -> bool {
        matches!(self, GraphSendError::Unsure(_))
    }
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect()
}

/// Send `message` — a whole message, headers and body, as SMTP would carry
/// it — through Graph at `url` as the user `token` was issued for.
pub fn send(url: &str, token: &str, message: &[u8]) -> Result<(), GraphSendError> {
    let path = url.find("/me/").map_or(url, |i| &url[i..]);
    note("graph >", &format!("POST {path} [a message, {} bytes]", message.len()));
    let client = reqwest::blocking::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| GraphSendError::Network(one_line(&e.to_string())))?;
    let response = client
        .post(url)
        .bearer_auth(token)
        .header(reqwest::header::CONTENT_TYPE, "text/plain")
        .body(super::oauth::base64_standard(message))
        .send();
    let response = match response {
        Ok(r) => r,
        // Never reached: nothing was handed over.
        Err(e) if e.is_connect() || e.is_builder() => {
            note("graph <", "no connection");
            return Err(GraphSendError::Network(one_line(&e.to_string())));
        }
        // Asked, and no answer: it may have gone.
        Err(e) => {
            note("graph <", "no answer");
            return Err(GraphSendError::Unsure(one_line(&e.to_string())));
        }
    };
    let status = response.status().as_u16();
    if (200..300).contains(&status) {
        note("graph <", &status.to_string());
        return Ok(());
    }
    // A server's failure may have come after it took the message.
    if status >= 500 {
        note("graph <", &status.to_string());
        return Err(GraphSendError::Unsure(format!("Microsoft 365 answered {status}")));
    }
    let value: Value = response.json().unwrap_or(Value::Null);
    let error = value.get("error");
    let code = error.and_then(|e| e.get("code")).and_then(Value::as_str).unwrap_or("").to_string();
    let message = error.and_then(|e| e.get("message")).and_then(Value::as_str).map(one_line).unwrap_or_default();
    let said = match (code.is_empty(), message.is_empty()) {
        (_, true) if code.is_empty() => status.to_string(),
        (_, true) => format!("{status} {code}"),
        (true, false) => message,
        (false, false) => format!("{code}: {message}"),
    };
    note("graph <", &format!("{status} {said}"));
    Err(match status {
        401 => GraphSendError::SignIn(said),
        403 => GraphSendError::Forbidden(said),
        429 => GraphSendError::RateLimited,
        _ => GraphSendError::Refused(said),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::teams::fake::{graph as server, TOKEN};

    #[test]
    fn a_message_goes_whole_as_mime_and_graphs_refusals_say_what_to_do() {
        let (base, seen) = server(Box::new(|path, body| {
            match path {
            "/me/sendMail" if body == crate::sources::oauth::base64_standard(b"Subject: Hi\r\n\r\nSure.\r\n") => {
                (202, String::new())
            }
            "/me/sendMail" => (400, r#"{"error":{"code":"ErrorMimeContentInvalidBase64String","message":"Invalid base64 string for MIME content."}}"#.into()),
            _ => (404, "{}".into()),
        }
        }));
        let url = format!("{base}/me/sendMail");
        let ((), said) = {
            let (sent, said) =
                crate::sources::transcript::recording(|| send(&url, TOKEN, b"Subject: Hi\r\n\r\nSure.\r\n"));
            (sent.unwrap(), said)
        };
        assert!(said.iter().any(|l| l.contains("[a message, 22 bytes]")), "the size, not the words: {said:?}");
        assert!(said.iter().all(|l| !l.contains("Sure.")), "{said:?}");
        assert!(seen.lock().unwrap().iter().any(|p| p.starts_with("/me/sendMail?")));
        let e = send(&url, TOKEN, b"something else").unwrap_err();
        assert!(matches!(e, GraphSendError::Refused(ref why) if why.contains("Invalid base64")), "{e}");
        assert!(!e.may_have_gone());
    }

    #[test]
    fn a_sign_in_turned_down_a_refusal_and_a_lost_answer_are_told_apart() {
        for (status, body, gone) in [
            (401, r#"{"error":{"code":"InvalidAuthenticationToken","message":"Access token has expired."}}"#, false),
            (403, r#"{"error":{"code":"ErrorAccessDenied","message":"Access is denied."}}"#, false),
            (429, "{}", false),
            (503, "", true),
            // No answer at all, once asked.
            (0, "", true),
        ] {
            let (base, _) = server(Box::new(move |_, _| (status, body.to_string())));
            let e = send(&format!("{base}/me/sendMail"), TOKEN, b"x").unwrap_err();
            assert_eq!(e.may_have_gone(), gone, "{status}: {e}");
            match status {
                401 => assert!(matches!(e, GraphSendError::SignIn(_)), "{e}"),
                403 => {
                    assert!(matches!(e, GraphSendError::Forbidden(ref why) if why.contains("ErrorAccessDenied")), "{e}")
                }
                429 => assert!(matches!(e, GraphSendError::RateLimited)),
                _ => assert!(matches!(e, GraphSendError::Unsure(_)), "{e}"),
            }
        }
        // Nothing there to hand it to: not sent.
        let e = send("http://127.0.0.1:9/me/sendMail", TOKEN, b"x").unwrap_err();
        assert!(matches!(e, GraphSendError::Network(_)) && !e.may_have_gone(), "{e}");
    }
}
