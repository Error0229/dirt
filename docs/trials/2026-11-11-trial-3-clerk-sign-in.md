# Trial 3: Clerk sign-in from native clients

- **Date:** 2026-11-11
- **Verdict:** not finished. The desktop and CLI flow passes with an email
  code and with Google. The Android flow has not been run
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
| Sign-in with Google, desktop | Passed, on port 51617, through the system browser |
| Sign-in with Google, Android | Not run |
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
- **The browser:** for the email-code runs the sign-in pages were driven in a
  private browser tab. The Google run opened the system browser with
  `webbrowser::open` and was completed by hand with a real Google account.
  Its output is not in the repository, because it contains a real name and
  email address.
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

### Sign-in with Google

One run, from the same program, completed and verified.

| | |
|---|---|
| Account | Clerk created a new user for the Google account on first sign-in. No separate sign-up step |
| ID token | same shape as with an email code, plus a `picture` claim. `email_verified` is true |
| Tokens | as before: opaque access token, refresh token issued, 24-hour lifetimes |
| The callback | carries `code` and `state`, and also an `iss` parameter naming the issuer |

A first attempt failed in an instructive way. The program waits ten minutes
for the browser. The sign-in was finished after that, so Clerk redirected to
a port nobody was listening on, and the browser showed a connection error
with a valid code in the address bar. The code could not be used: its PKCE
verifier had gone with the program.

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
- **A late return strands the user too.** If the client has stopped waiting,
  the browser ends on a connection error at `127.0.0.1` and nothing says
  why. The client should keep listening until the person cancels, not for a
  fixed time, and its own message should say to start again.
- **The callback names the issuer.** Clerk adds an `iss` parameter to the
  redirect. The client should check it against the issuer it started with,
  which costs one comparison. The trial's client ignores it.
- **Google needs no extra code.** The client and the verifier were unchanged.
  The provider choice happens on Clerk's page.
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
  send it to our server, that it listens until the person cancels, and that
  it checks the `iss` parameter of the callback.
- **Open for step 6:** the consent screen, and the device-code grant for the
  CLI without a browser.

## Still to do

1. **Android.** Add the flow to the trial 2 app: an intent filter for
   `dev.dirt.app://callback`, the verifier saved before the browser opens,
   and the return handled in `MainActivity`. Run it with an email code and
   with Google, and once with the app killed while the browser is open.
2. **Confirm the user ID matches** between desktop and Android.

## Left in the Clerk development instance

- The OAuth application "Dirt trial (native)". Its client ID is in
  `.env.trials.local` as `CLERK_OAUTH_CLIENT_ID`. It has no secret.
- The test user `dirt.trial+clerk_test@example.com`.
- A user for the Google account used in the Google run.

## How to rerun

```bash
cd trials/clerk-signin
cargo test

# CLERK_ISSUER and CLERK_OAUTH_CLIENT_ID from .env.trials.local
cargo run --bin desktop
```
