use super::*;

// Explicit browser smoke test. Open the deployed return route with the synthetic
// code/state below after the readiness message; no real bank authorization is used.
#[tokio::test]
#[ignore = "requires a browser on this Mac and the deployed HTTPS return route"]
async fn browser_return_probe() {
    let mut listener = CallbackListener::bind("c".repeat(43)).await.unwrap();
    let (_cancel, receiver) = watch::channel(0);
    println!("Synthetic browser probe ready on reserved callback port 1431.");
    let code = tokio::time::timeout(std::time::Duration::from_secs(120), listener.wait(receiver))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(code, "synthetic-browser-probe");
}

// Run explicitly with the public site's existing dev server on assigned port 3001.
// This exercises both repositories with synthetic values, never provider credentials.
#[tokio::test]
#[ignore = "requires source-public-site on localhost:3001"]
async fn public_return_round_trip_with_synthetic_data() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let nonce = "b".repeat(43);
    let code = "synthetic bridge +&=/#<> code";
    let mut listener = CallbackListener::bind(nonce.clone()).await.unwrap();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap();
    let mut public = reqwest::Url::parse("http://localhost:3001/banking/callback").unwrap();
    public
        .query_pairs_mut()
        .append_pair("state", &nonce)
        .append_pair("code", code);
    let returned = client.get(public).send().await.unwrap();
    assert_eq!(returned.status(), StatusCode::SEE_OTHER);
    assert_eq!(returned.headers()["referrer-policy"], "no-referrer");
    assert!(returned.headers()["cache-control"]
        .to_str()
        .unwrap()
        .contains("no-store"));
    let destination =
        reqwest::Url::parse(returned.headers()["location"].to_str().unwrap()).unwrap();
    assert_eq!(
        destination.origin().ascii_serialization(),
        "http://127.0.0.1:1431"
    );
    assert_eq!(destination.path(), "/banking/callback");
    let received = client.get(destination).send().await.unwrap();
    assert_eq!(received.status(), StatusCode::OK);
    let page = received.text().await.unwrap();
    assert!(page.contains("history.replaceState"));
    assert!(!page.contains(code) && !page.contains(&nonce));
    let (_cancel, receiver) = watch::channel(0);
    assert_eq!(listener.wait(receiver).await.unwrap(), code);
}

async fn attempt(state: CallbackState, nonce: &str, host: &str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::HOST, host.parse().unwrap());
    callback(
        State(state),
        headers,
        Ok(Query(CallbackQuery {
            state: nonce.into(),
            code: Some("synthetic-code".into()),
            error: None,
        })),
    )
    .await
}

#[tokio::test]
async fn wrong_state_or_host_does_not_consume_callback() {
    let (tx, mut rx) = oneshot::channel();
    let state = CallbackState {
        expected_state: "nonce".into(),
        reply: Arc::new(Mutex::new(Some(tx))),
    };
    assert_eq!(
        attempt(state.clone(), "wrong", "127.0.0.1:1431")
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        attempt(state.clone(), "nonce", "evil.invalid")
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert!(rx.try_recv().is_err());
    assert_eq!(
        attempt(state.clone(), "nonce", "127.0.0.1:1431")
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(rx.await.unwrap().unwrap(), "synthetic-code");
    assert_eq!(
        attempt(state, "nonce", "127.0.0.1:1431").await.status(),
        StatusCode::GONE
    );
}

#[tokio::test]
async fn http_return_validates_input_and_accepts_a_code_only_once() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let nonce = "a".repeat(43);
    let code = "synthetic +&=/#<> code";
    // Port 1431 is reserved for this callback; never stop another listener.
    let mut listener = CallbackListener::bind(nonce.clone()).await.unwrap();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let mut url = reqwest::Url::parse(crate::LOCAL_CALLBACK_URL).unwrap();
    url.query_pairs_mut()
        .append_pair("state", &nonce)
        .append_pair("code", code);

    let wrong_host = client
        .get(url.clone())
        .header("Host", "evil.invalid")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_host.status(), StatusCode::BAD_REQUEST);
    let mut wrong_state = url.clone();
    wrong_state.set_query(Some("state=wrong&code=synthetic"));
    assert_eq!(
        client.get(wrong_state).send().await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    for extra in ["state", "code", "unexpected"] {
        let mut malformed = url.clone();
        malformed
            .query_pairs_mut()
            .append_pair(extra, "synthetic-private-value");
        let response = client.get(malformed).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
        let html = response.text().await.unwrap();
        assert!(!html.contains(code) && !html.contains("synthetic-private-value"));
    }

    let response = client.get(url.clone()).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store, max-age=0");
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    let csp = response.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(csp.contains("script-src 'sha256-") && !csp.contains("unsafe-inline"));
    let html = response.text().await.unwrap();
    assert!(html.contains("history.replaceState(null, \"\", \"/banking/callback\")"));
    assert!(!html.contains(code) && !html.contains(&nonce));
    let (_cancel, receiver) = watch::channel(0);
    assert_eq!(listener.wait(receiver).await.unwrap(), code);
    assert_eq!(
        client.get(url).send().await.unwrap().status(),
        StatusCode::GONE
    );
}

#[tokio::test]
async fn bank_refusal_is_delivered_as_cancellation_without_echoing_it() {
    let (tx, rx) = oneshot::channel();
    let state = CallbackState {
        expected_state: "synthetic-state".into(),
        reply: Arc::new(Mutex::new(Some(tx))),
    };
    let mut headers = HeaderMap::new();
    headers.insert(header::HOST, "127.0.0.1:1431".parse().unwrap());
    let response = callback(
        State(state),
        headers,
        Ok(Query(CallbackQuery {
            state: "synthetic-state".into(),
            code: None,
            error: Some("synthetic-private-bank-error".into()),
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(matches!(rx.await.unwrap(), Err(Error::Cancelled)));
    let body = axum::body::to_bytes(response.into_body(), 16384)
        .await
        .unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("synthetic-private-bank-error"));
}
