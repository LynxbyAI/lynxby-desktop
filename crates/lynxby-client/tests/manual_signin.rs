//! The full sign-in, with a person at the browser.
//!
//! ```sh
//! LYNXBY_TEST_BASE_URL=https://projectsovereign.ddev.site \
//!   cargo test -p lynxby-client --test manual_signin -- --ignored --nocapture
//! ```
//!
//! It prints the authorize URL and blocks. Open it, approve, and the test
//! prints the token it received — the same code path the window runs, minus
//! the window.

use lynxby_client::api::ApiClient;
use lynxby_client::signin::SignIn;
use lynxby_client::store::Store;

#[test]
#[ignore = "manual: needs a person in a browser"]
fn sign_in_and_read_the_landing_data() {
    let base = std::env::var("LYNXBY_TEST_BASE_URL")
        .expect("set LYNXBY_TEST_BASE_URL to a running deployment");

    let store = Store::new(std::env::temp_dir().join("lynxby-manual-signin"));
    let _ = store.clear();

    let pending = SignIn::begin(&base, 47844, &store).expect("begin");

    println!("AUTHORIZE_URL={}", pending.authorize_url);
    println!("CLIENT_ID={}", pending.client_id());

    let session = pending
        .wait(&std::sync::atomic::AtomicBool::new(false))
        .expect("the browser came back");

    println!("TOKEN={}", session.access_token);

    let api = ApiClient::new(&session.base_url, &session.access_token);
    let me = api.me().expect("probe");
    let assistants = api.assistants().expect("assistants");
    let tools = api.tools().expect("tools");

    println!(
        "ME={} / {} / scopes {:?}",
        me.display_name(),
        me.organisation_name(),
        me.scopes
    );
    println!(
        "ASSISTANTS={}",
        assistants
            .iter()
            .map(|assistant| assistant.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "TOOLS={}",
        tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
}
