# Trial 3: Clerk sign-in from native clients

- **Date:** 2026-11-11
- **Verdict:** not finished. The desktop and CLI flow passes with an email
  code. Google sign-in and the Android flow have not been run
- **Code:** `trials/clerk-signin/` on the `trials/step-1` branch
- **Plan:** `docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md`, Step 1
- **Tracking issue:** #237

## Question

Can the desktop app, the CLI and the Android app sign in through Clerk in the
system browser, and can our server verify the result with standard libraries
(decision 9)?

## Status

| Criterion | Result |
|---|---|
| Desktop and CLI complete sign-in twice, on two random ports | Passed with an email code, on ports 62700 and 65328 |
| The verifier accepts the real ID token | Passed |
| The verifier rejects tampered, expired, wrong-audience and wrong-nonce tokens | Passed, along with the other bad cases listed below |
| The user ID is identical across two sign-ins | Passed |
| Sign-in with Google | Not run. It needs your Google account |
| Android completes sign-in, including when the app is killed while the browser is open | Not run |
| The user ID is identical across desktop and Android | Not run |

Nothing found so far argues against decision 9.

## Method

- **Clerk application:** an OAuth application named "Dirt trial (native)" in
  the development instance, created through Clerk's Backend API. It is a
  public client with PKCE required and the consent screen on. Redirect targets
  are `http://127.0.0.1/callback` and `dev.dirt.app://callback`.
- **The client:** a terminal program, `src/bin/desktop.rs`. It reads the
  discovery document, listens on `127.0.0.1` on a port the system picks, makes
  a PKCE verifier, a state and a nonce, and prints the address to open. When
  the browser returns it checks the state, exchanges the code with no client
  secret, verifies the ID token and calls the user-info endpoint.
- **The verifier:** `src/verify.rs`, the check that will move into `dirt-api`.
  It uses `jsonwebtoken` 11 and nothing specific to Clerk.
- **The user:** a test user created through the Backend API, with an address
  ending in `+clerk_test@example.com`. Clerk development instances accept the
  fixed code `424242` for such addresses and send no email.
- **The browser:** the sign-in pages were driven in a private browser tab, not
  opened with `webbrowser::open`. That call is in the code and was not
  exercised.
- **PKCE and the callback server are written by hand,** about 200 lines, with
  `reqwest`, `sha2` and `base64`. The `openidconnect` and `oauth2` crates were
  not needed.

## Results

### What Clerk publishes

From `{issuer}/.well-known/openid-configuration`:

| | |
|---|---|
| Grant types | authorization code, refresh token, device code |
| PKCE methods | S256 only |
| Token endpoint authentication | client secret, or none |
| ID token signing | RS256 only |
| Scopes | `openid`, `profile`, `email`, `offline_access`, `public_metadata`, `private_metadata` |

### The sign-in

Both runs completed. The second ran on a different port, in the same browser
session, and returned the same user ID.

| | |
|---|---|
| Redirect target | `http://127.0.0.1:<port>/callback` accepted on both ports |
| Authorization code | 48 characters |
| ID token | RS256. The key ID is the Clerk instance ID |
| ID token lifetime | 24 hours |
| ID token claims | `iss`, `sub`, `aud`, `exp`, `iat`, `auth_time`, `nonce`, `at_hash`, `jti`, `rat`, `sid`, `email`, `email_verified`, `name`, `given_name`, `family_name` |
| `sub` | the Clerk user ID, `user_…`. The user-info endpoint returns the same value |
| `aud` | a list holding our client ID |
| Access token | opaque, 36 characters, valid 24 hours |
| Refresh token | issued, although the request did not ask for `offline_access` |

### What the user sees

- **Sign-in page:** "Sign in to Dirt", a Google button, and an email field.
  After the email, a code field.
- **Consent screen, on every sign-in:** "Dirt trial (native) wants to access
  Dirt on behalf of <email>", a list of three permissions (identity, basic
  profile, email address), a warning to "make sure that you trust Dirt trial
  (native) (127.0.0.1)", and Deny and Allow buttons.
- **Second sign-in in the same browser:** no email step. The consent screen
  appears again.
- **After Allow:** the browser is sent to our local page, which says "Signed in.
  You can close this tab."
- **Wrong redirect target:** Clerk shows a page of raw JSON, `invalid_request`
  with "The 'redirect_uri' parameter does not match any of the OAuth 2.0
  Client's pre-registered redirect urls". The browser never returns to the
  app. `http://localhost:<port>/callback` gets this page; only `127.0.0.1`
  is accepted with any port.

### The verifier

13 tests, with tokens signed by a key made in the test. No network.

| Token | Outcome |
|---|---|
| Valid | accepted; the user ID and the other claims come back |
| Payload changed after signing | rejected |
| Expired | rejected |
| Issued for another client | rejected |
| Carries another attempt's nonce, or none | rejected |
| From another issuer | rejected |
| Missing `exp`, `aud` or `sub` | rejected |
| Signed by another key that uses the same key ID | rejected |
| Names a key the issuer does not publish, or no key | rejected |
| Signed with HS256, using the public key as the secret | rejected before any key is tried |
| Not a token | rejected |

The same function accepted both real ID tokens from Clerk.

## Findings

- **The flow in the design works as written.** Public client, PKCE, loopback
  redirect on a random port, no secret in the client.
- **Clerk's Backend API accepts a custom URL scheme** as a redirect target.
  Whether Android can return through it is the untested part.
- **The server needs only the ID token.** It carries the user ID, the client
  ID, the nonce and the email. The access token and refresh token are not
  needed, because Dirt issues its own device token. The client should discard
  them without storing them.
- **Clerk adds `offline_access` to the application by itself** and issues a
  refresh token even when the request does not ask for one. Whether this can
  be turned off was not checked.
- **`aud` is a list.** `jsonwebtoken` handles both shapes. A hand-written
  comparison with a string would have failed.
- **The consent screen reads oddly for a first-party app.** The user signs in
  to Dirt and is then asked whether an application may "access Dirt" on their
  behalf, naming `127.0.0.1`, on every sign-in. The plan said to leave consent
  on. Whether Clerk allows it off for a public client, and whether that is
  wise for a loopback client, is a decision for step 6.
- **A wrong redirect target strands the user in the browser.** The client
  must time out, say so, and offer to try again. It cannot rely on being
  called back with an error.
- **Tests can replace Clerk with a local key,** as the design planned. The
  verifier takes the key set as a parameter and never fetches it itself.
- **End-to-end sign-in can be automated in a development instance** with the
  `+clerk_test` addresses and the fixed code. No mailbox is needed.
- **Clerk also offers the device-code grant.** A CLI on a machine with no
  browser, such as over SSH, cannot use the loopback redirect. The device-code
  grant would cover that case. It is off by default on the application and was
  not tried.
- **`jsonwebtoken` 11 needs a backend feature.** `rust_crypto` builds with no
  C compiler. Reading a PEM key needs the separate `use_pem` feature; the
  tests pass DER instead.

## Effect on the design

- **Decision 9 stands so far.**
- **Section 5.1:** add that the client keeps only the ID token long enough to
  send it to our server, and that a sign-in which never returns must time out.
- **Open for step 6:** the consent screen, and the device-code grant for the
  CLI without a browser.

## Still to do

1. **Google sign-in on desktop.** Run `cargo run --bin desktop` and choose
   Google. This needs a real Google account, so it needs you.
2. **Android.** Add the flow to the trial 2 app: an intent filter for
   `dev.dirt.app://callback`, the verifier saved before the browser opens,
   and the return handled in `MainActivity`. Run it with an email code and
   with Google, and once with the app killed while the browser is open.
3. **Confirm the user ID matches** between desktop and Android.
4. **Open the system browser from the program** with `webbrowser::open`,
   which the runs so far skipped.

## Left in the Clerk development instance

- The OAuth application "Dirt trial (native)". Its client ID is in
  `.env.trials.local` as `CLERK_OAUTH_CLIENT_ID`. It has no secret.
- The test user `dirt.trial+clerk_test@example.com`.

## How to rerun

```bash
cd trials/clerk-signin
cargo test

# CLERK_ISSUER and CLERK_OAUTH_CLIENT_ID from .env.trials.local
cargo run --bin desktop
```
