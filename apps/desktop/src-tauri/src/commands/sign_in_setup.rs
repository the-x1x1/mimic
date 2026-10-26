//! The user's own registration with Microsoft or Google (0.10.0-alpha.59).
//!
//! Signing in to a mailbox in the browser, rather than with a password, needs
//! an app registered with the provider. A build made with one has its client
//! id compiled in (`oauth::microsoft_client_id`, `google_client_id`). Without
//! one — or to use their own — the user registers Mimic themselves and pastes
//! what the provider shows: Microsoft's Application (client) ID, or Google's
//! client ID and secret. They are kept in the secret store with the
//! mailboxes' own sign-ins, never in settings, and take effect at once
//! (`Credentials::register`). The secret is never handed back to the screen.

use mimic_core::providers::SecretStore;
use mimic_core::sources::imap::Auth;
use mimic_core::sources::oauth::{self, Provider};
use serde::Serialize;
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

const MICROSOFT_CLIENT: &str = "oauth:microsoft:client";
const GOOGLE_CLIENT: &str = "oauth:google:client";
const GOOGLE_SECRET: &str = "oauth:google:secret";

fn saved(secrets: &dyn SecretStore, key: &str) -> Option<String> {
    secrets.get(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// The user's own Microsoft registration, saved here.
fn own_microsoft(secrets: &dyn SecretStore) -> Option<String> {
    saved(secrets, MICROSOFT_CLIENT)
}

/// The user's own Google registration: the client id and its secret, both,
/// since Google's token endpoint takes neither without the other.
fn own_google(secrets: &dyn SecretStore) -> Option<(String, String)> {
    saved(secrets, GOOGLE_CLIENT).zip(saved(secrets, GOOGLE_SECRET))
}

/// What mailboxes signing in with `auth` sign in as: the provider (with
/// Google's secret) and the client id — the user's own registration, else
/// this build's; no client id when neither. None for a password.
pub fn registered(secrets: &dyn SecretStore, auth: Auth) -> Option<(Provider, Option<String>)> {
    match auth {
        Auth::Password => None,
        Auth::Microsoft => Some((
            Provider::microsoft(),
            own_microsoft(secrets).or_else(|| oauth::microsoft_client_id().map(str::to_string)),
        )),
        Auth::Google => Some(match own_google(secrets) {
            Some((client, secret)) => (Provider { client_secret: Some(secret), ..Provider::google() }, Some(client)),
            None => (Provider::google(), oauth::google_client_id().map(str::to_string)),
        }),
    }
}

/// One provider's registration, as Settings shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSetup {
    /// The client id a sign-in uses now: the user's own, or this build's;
    /// none when there is neither. Not a secret: the provider shows it to
    /// anyone signing in.
    pub client_id: Option<String>,
    /// Whether that is the user's own, saved here.
    pub own: bool,
    /// Whether this build has one of its own, to go back to.
    pub built_in: bool,
}

/// What each provider signs mailboxes in as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SignInSetup {
    pub microsoft: ProviderSetup,
    pub google: ProviderSetup,
}

pub fn setup(secrets: &dyn SecretStore) -> SignInSetup {
    let microsoft_own = own_microsoft(secrets);
    let google_own = own_google(secrets).map(|(client, _)| client);
    SignInSetup {
        microsoft: ProviderSetup {
            own: microsoft_own.is_some(),
            built_in: oauth::microsoft_client_id().is_some(),
            client_id: microsoft_own.or_else(|| oauth::microsoft_client_id().map(str::to_string)),
        },
        google: ProviderSetup {
            own: google_own.is_some(),
            built_in: oauth::google_client_id().is_some(),
            client_id: google_own.or_else(|| oauth::google_client_id().map(str::to_string)),
        },
    }
}

/// Why what was pasted can't be used, before anything is kept.
fn checked(with: Auth, client_id: &str, client_secret: Option<&str>) -> CommandResult<()> {
    let invalid = |why: &str| Err(CommandError::new("invalid", why));
    match with {
        Auth::Password => invalid("That isn't a way of signing in."),
        Auth::Microsoft if !oauth::is_microsoft_client_id(client_id) => invalid(
            "That isn't an Application (client) ID. Azure shows it on the app's Overview as five groups of letters and digits, like 0f5c1a2b-3c4d-4e5f-8a9b-0c1d2e3f4a5b.",
        ),
        Auth::Google if !oauth::is_google_client_id(client_id) => {
            invalid("That isn't a Google client ID. Google Cloud shows it ending in .apps.googleusercontent.com.")
        }
        Auth::Google if !client_secret.is_some_and(oauth::is_client_secret) => {
            invalid("Paste the client secret too. Google Cloud shows it beside the client ID.")
        }
        _ => Ok(()),
    }
}

/// Signing in with `with` from now on uses what is registered now.
fn apply(state: &crate::AppState, with: Auth) {
    if let Some((provider, client)) = registered(&*state.secrets, with) {
        state.mail_credentials.register(with, provider, client);
    }
}

fn store_error(e: std::io::Error) -> CommandError {
    CommandError::new("secrets", format!("I couldn't keep it: {e}"))
}

#[tauri::command]
pub async fn get_sign_in_setup(state: State<'_, SharedState>) -> CommandResult<SignInSetup> {
    Ok(setup(&*state.secrets))
}

/// Keep the user's own registration with Microsoft or Google, and sign in
/// with it from now on. Checked first; nothing is kept that couldn't be one.
#[tauri::command]
pub async fn save_sign_in_setup(
    state: State<'_, SharedState>,
    with: Auth,
    client_id: String,
    client_secret: Option<String>,
) -> CommandResult<SignInSetup> {
    let client_id = client_id.trim().to_string();
    let client_secret = client_secret.map(|s| s.trim().to_string());
    checked(with, &client_id, client_secret.as_deref())?;
    match with {
        Auth::Microsoft => state.secrets.set(MICROSOFT_CLIENT, &client_id).map_err(store_error)?,
        Auth::Google => {
            state.secrets.set(GOOGLE_SECRET, client_secret.as_deref().unwrap_or_default()).map_err(store_error)?;
            state.secrets.set(GOOGLE_CLIENT, &client_id).map_err(store_error)?;
        }
        Auth::Password => {}
    }
    apply(&state, with);
    Ok(setup(&*state.secrets))
}

/// Forget the user's own registration: this build's, if it has one, is used
/// again; otherwise signing in with that provider isn't offered.
#[tauri::command]
pub async fn forget_sign_in_setup(state: State<'_, SharedState>, with: Auth) -> CommandResult<SignInSetup> {
    let keys: &[&str] = match with {
        Auth::Microsoft => &[MICROSOFT_CLIENT],
        Auth::Google => &[GOOGLE_CLIENT, GOOGLE_SECRET],
        Auth::Password => &[],
    };
    for key in keys {
        state.secrets.remove(key).map_err(store_error)?;
    }
    apply(&state, with);
    Ok(setup(&*state.secrets))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimic_core::providers::MemorySecrets;

    fn store(pairs: &[(&str, &str)]) -> MemorySecrets {
        MemorySecrets(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
    }

    #[test]
    fn the_users_own_registration_is_used_over_the_builds_and_google_needs_its_secret() {
        let ms = "0f5c1a2b-3c4d-4e5f-8a9b-0c1d2e3f4a5b";
        let g = "123456789012-abc.apps.googleusercontent.com";
        let own = store(&[(MICROSOFT_CLIENT, ms), (GOOGLE_CLIENT, g), (GOOGLE_SECRET, "GOCSPX-x")]);
        let (provider, client) = registered(&own, Auth::Microsoft).unwrap();
        assert_eq!((provider.key, client.as_deref()), ("microsoft", Some(ms)));
        let (provider, client) = registered(&own, Auth::Google).unwrap();
        assert_eq!((provider.client_secret.as_deref(), client.as_deref()), (Some("GOCSPX-x"), Some(g)));
        assert!(registered(&own, Auth::Password).is_none());
        let shown = setup(&own);
        assert!(shown.microsoft.own && shown.google.own);
        assert_eq!(shown.google.client_id.as_deref(), Some(g));

        // A Google client id without its secret isn't a registration: this
        // build's is used, as if nothing were saved.
        let half = store(&[(GOOGLE_CLIENT, g)]);
        let (provider, client) = registered(&half, Auth::Google).unwrap();
        assert_eq!(client.as_deref(), oauth::google_client_id());
        assert_eq!(provider.client_secret, Provider::google().client_secret);
        assert!(!setup(&half).google.own);
        let nothing = setup(&store(&[]));
        assert_eq!(nothing.microsoft.client_id.as_deref(), oauth::microsoft_client_id());
        assert_eq!(nothing.microsoft.built_in, oauth::microsoft_client_id().is_some());
    }

    #[test]
    fn what_is_pasted_is_checked_before_anything_is_kept() {
        let ms = "0f5c1a2b-3c4d-4e5f-8a9b-0c1d2e3f4a5b";
        let g = "123456789012-abc.apps.googleusercontent.com";
        assert!(checked(Auth::Microsoft, ms, None).is_ok());
        assert!(checked(Auth::Microsoft, "my app", None).is_err());
        assert!(checked(Auth::Google, g, Some("GOCSPX-x")).is_ok());
        assert!(checked(Auth::Google, g, None).is_err(), "Google's needs its secret");
        assert!(checked(Auth::Google, ms, Some("GOCSPX-x")).is_err(), "a Microsoft id isn't Google's");
        assert!(checked(Auth::Password, ms, None).is_err());
    }

    #[test]
    fn what_settings_is_shown_matches_its_fixture() {
        let shown = SignInSetup {
            microsoft: ProviderSetup {
                client_id: Some("0f5c1a2b-3c4d-4e5f-8a9b-0c1d2e3f4a5b".into()),
                own: true,
                built_in: false,
            },
            google: ProviderSetup { client_id: None, own: false, built_in: false },
        };
        let json = serde_json::to_value(&shown).unwrap();
        assert!(json.to_string().contains("\"builtIn\""), "{json}");
        crate::fixtures::check_fixture("sign_in_setup.json", &json);
    }
}
