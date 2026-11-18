//! OAuth 2.0 in an environment: a configuration read, its requests made, the
//! answers read, PKCE as RFC 7636 says.

use super::*;

fn security() -> Value {
    serde_json::json!({"Auth": {
        "main": {
            "Type": "OAuth2", "Grant Type": "Authorization Code",
            "Auth URL": "https://auth.e.com/authorize", "Token URL": "https://auth.e.com/token",
            "Redirect URL": "http://localhost:12345/cb", "Client ID": "app", "Client Secret": "{{secret}}",
            "Scope": "read write", "PKCE": true,
            "Custom Request Parameters": {"audience": {"Value": "api", "Use": "In Auth Request"}}
        },
        "machine": {"Type": "OAuth2", "Grant Type": "Client Credentials", "Token URL": "https://auth.e.com/token",
            "Client ID": "m", "Client Secret": "s", "Client Credentials": "in body"},
        "fake": {"Type": "Mock", "Token": "t0k"}
    }})
}

fn text(t: &str) -> String {
    t.replace("{{secret}}", "sh")
}

#[test]
fn a_configuration_is_read_with_its_variables() {
    let main = config(&security(), "main", &text).unwrap();
    assert_eq!(
        (main.grant, main.client_secret.as_str(), main.pkce),
        (Grant::AuthorizationCode, "sh", true)
    );
    assert_eq!(main.custom, [("audience".to_owned(), "api".to_owned(), Some(true))]);
    assert_eq!(config(&security(), "fake", &text).unwrap().mock, Some(("t0k".into(), None)));
    assert!(config(&security(), "nope", &text).is_none());
}

#[test]
fn its_requests_are_made_as_the_grant_wants() {
    let main = config(&security(), "main", &text).unwrap();
    let url = main.authorize_url("http://localhost:12345/cb", "st", "v");
    assert!(url.starts_with("https://auth.e.com/authorize?response_type=code&client_id=app&redirect_uri=http%3A%2F%2Flocalhost%3A12345%2Fcb&state=st&scope=read+write"), "{url}");
    assert!(url.contains("code_challenge_method=S256") && url.contains("audience=api"));
    let exchange = main.exchange("c0de", "http://localhost:12345/cb", "v");
    assert!(
        exchange.body().contains("grant_type=authorization_code&code=c0de")
            && exchange.body().contains("code_verifier=v")
    );
    assert!(
        exchange.headers.iter().any(|(n, v)| n == "Authorization" && v.starts_with("Basic ")),
        "client said by Basic"
    );
    assert!(!exchange.body().contains("audience"), "an auth request's own parameter");
    let machine = config(&security(), "machine", &text).unwrap();
    let body = machine.client_credentials().body();
    assert_eq!(body, "grant_type=client_credentials&client_id=m&client_secret=s");
}

#[test]
fn pkce_and_answers_as_the_rfcs_say() {
    // RFC 7636, appendix B.
    assert_eq!(
        challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    let token =
        read_token(r#"{"access_token": "a", "refresh_token": "r", "expires_in": 3600}"#, 1000)
            .unwrap();
    assert_eq!(
        (token.access.as_str(), token.refresh.as_deref(), token.expires),
        ("a", Some("r"), Some(4600))
    );
    assert!(token.fresh(1000) && !token.fresh(4580));
    assert_eq!(
        read_token(r#"{"error": "invalid_client", "error_description": "bad"}"#, 0),
        Err("invalid_client bad".into())
    );
    let device = read_device(r#"{"device_code": "d", "user_code": "ABCD", "verification_uri": "https://e.com/device", "interval": 2}"#).unwrap();
    assert_eq!((device.user_code.as_str(), device.interval), ("ABCD", 2));
    assert_eq!(
        uses("Bearer {{$auth.token(\"main\")}} {{$auth.idToken('main')}}"),
        [("main".into(), false), ("main".into(), true)]
    );
    assert_eq!(
        redirect_code("GET /cb?code=a%2Bb&state=st HTTP/1.1"),
        Some((Some("a+b".into()), Some("st".into()), None))
    );
}

#[test]
fn an_implicit_sign_in_s_fragment_comes_back_as_a_query() {
    assert_eq!(redirect_fields("GET /callback HTTP/1.1"), None, "first: still in the fragment");
    let line = "GET /callback?access_token=a%2Bb&token_type=bearer&expires_in=60&state=s1 HTTP/1.1";
    let fields = redirect_fields(line).unwrap();
    let token = implicit_token(&fields, "s1", 1000).unwrap().unwrap();
    assert_eq!((token.access.as_str(), token.expires), ("a+b", Some(1060)));
    assert_eq!(implicit_token(&fields, "other", 1000), None, "another sign-in's");
    let refused = redirect_fields("GET /cb?error=access_denied&state=s1 HTTP/1.1").unwrap();
    assert_eq!(implicit_token(&refused, "s1", 0), Some(Err("access_denied".to_owned())));
    assert!(IMPLICIT_PAGE.contains("location.replace"));
}
