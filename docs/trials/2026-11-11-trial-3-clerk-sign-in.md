# Trial 3: Clerk sign-in from native clients

- **Date:** 2026-11-11
- **Verdict:** pass, with one gap: Android was tested on the emulator only
- **Code:** `trials/clerk-signin/` and `trials/clerk-signin-android/` on the
  `trials/step-1` branch
- **Plan:** `docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md`, Step 1
- **Tracking issue:** #237

## Question

Can the desktop app, the CLI and the Android app sign in through Clerk in the
system browser, and can our server verify the result with standard libraries
(decision 9)?

## Verdict

Yes. Decision 9 stands: Clerk for identity only, through OAuth with PKCE.

| Criterion | Result |
|---|---|
| Desktop and CLI complete sign-in twice, on two random ports | Passed with an email code on ports 62700 and 65328, and with Google on port 51617 |
| Android completes sign-in | Passed with an email code and with Google |
| Android completes sign-in when the app is killed while the browser is open | Passed |
| The verifier accepts the real ID token | Passed on every run, desktop and Android |
| The verifier rejects tampered, expired, wrong-audience and wrong-nonce tokens | Passed, along with the other bad cases listed below |
| The user ID is identical across two sign-ins | Passed |
| The user ID is identical across desktop and Android | Passed, for the email user and for the Google user |

## Method

- **Clerk application:** an OAuth application named "Dirt trial (native)" in
  the development instance, created through Clerk's Backend API. It is a
  public client with PKCE required and the consent screen on. Redirect targets
  are `http://127.0.0.1/callback` and `dev.dirt.app://callback`.
- **The desktop client:** a terminal program, `src/bin/desktop.rs`. It reads
  the discovery document, listens on `127.0.0.1` on a port the system picks,
  makes a PKCE verifier, a state and a nonce, and opens the browser. When the
  browser returns it checks the state, exchanges the code with no client
  secret, verifies the ID token and calls the user-info endpoint. The desktop
  app and the CLI will both run this flow; the trial did not involve the
  Dioxus desktop app itself.
- **The Android client:** a one-screen Dioxus app,
  `trials/clerk-signin-android/`, sharing the flow and the verifier with the
  desktop program. "Sign in" saves the attempt to a file and opens the
  browser. Clerk redirects to `dev.dirt.app://callback`, Android delivers that
  to the app's activity, and the activity writes the address to a file that
  the Rust code watches.
- **The verifier:** `src/verify.rs`, the check that will move into `dirt-api`.
  It uses `jsonwebtoken` 11 and nothing specific to Clerk.
- **The email user:** a test user with an address ending in
  `+clerk_test@example.com`. Clerk development instances accept the fixed code
  `424242` for such addresses and send no email. These runs were automated.
- **The Google user:** a real Google account, signed in by hand on desktop and
  on the emulator. The output of those runs is not in the repository in full,
  because it contains a real name, email address and account ID.
- **Android device:** the `dirt-trial` emulator, Android 16, x86_64, with
  Chrome as the browser.
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

### The sign-in, on every client

| | |
|---|---|
| Redirect target, desktop | `http://127.0.0.1:<port>/callback`, accepted on all three ports |
| Redirect target, Android | `dev.dirt.app://callback`, delivered to the app by Chrome |
| The callback | carries `code` and `state`, and an `iss` parameter naming the issuer |
| Authorization code | 48 characters |
| ID token | RS256. The key ID is the Clerk instance ID |
| ID token lifetime | 24 hours |
| ID token claims | `iss`, `sub`, `aud`, `exp`, `iat`, `auth_time`, `nonce`, `at_hash`, `jti`, `rat`, `sid`, `email`, `email_verified`, `name`, `given_name`, `family_name`. A Google user also has `picture` |
| `sub` | the Clerk user ID, `user_…`. The user-info endpoint returns the same value |
| `aud` | a list holding our client ID |
| Access token | opaque, 36 characters, valid 24 hours |
| Refresh token | issued, although the request did not ask for `offline_access` |

### Android

Three runs, all ending in a verified ID token.

| Run | How the app got the redirect | User ID |
|---|---|---|
| Email code | the app was still running; the activity received a new intent | same as desktop |
| Email code, app killed while Chrome was in front | Android started the app again for the redirect; the new process loaded the saved attempt and finished it | same as desktop |
| Google | the app was still running | same as the desktop Google run |

In the second run the attempt was saved by process 3971 and finished by
process 5127. The reports and screenshots are in
`trials/clerk-signin-android/results/`.

### Sign-in with Google

- Clerk created a user for the Google account on first sign-in, with no
  separate sign-up step.
- The client and the verifier needed no change. The provider is chosen on
  Clerk's page.

### What the user sees

- **Sign-in page:** "Sign in to Dirt", a Google button, and an email field.
  After the email, a code field.
- **Consent screen, on every sign-in:** "Dirt trial (native) wants to access
  Dirt on behalf of <email>", a list of three permissions (identity, basic
  profile, email address), a warning to "make sure that you trust Dirt trial
  (native)", and Deny and Allow buttons. On desktop it names `127.0.0.1`. On
  Android it offers "View full URL" in place of a host name.
- **Second sign-in in the same browser:** no email step. The consent screen
  appears again.
- **After Allow, desktop:** the browser is sent to our local page, which says
  "Signed in. You can close this tab."
- **After Allow, Android:** Chrome hands over to the app, which comes to the
  front.
- **Wrong redirect target:** Clerk shows a page of raw JSON, `invalid_request`
  with "The 'redirect_uri' parameter does not match any of the OAuth 2.0
  Client's pre-registered redirect urls". The browser never returns to the
  app. `http://localhost:<port>/callback` gets this page; only `127.0.0.1`
  is accepted with any port.
- **A late return, desktop:** the first Google attempt was finished after the
  program's ten-minute wait had ended. Clerk redirected to a port nobody was
  listening on, and the browser showed a connection error with a valid code
  in the address bar. The code could not be used: its PKCE verifier had gone
  with the program.

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

The same function accepted every real ID token from Clerk.

## Findings

### The flow

- **The flow in the design works as written** on all three clients: public
  client, PKCE, no secret in the client, loopback redirect on desktop and a
  custom scheme on Android.
- **The server needs only the ID token.** It carries the user ID, the client
  ID, the nonce and the email. The access token and refresh token are not
  needed, because Dirt issues its own device token. The client should discard
  them without storing them.
- **Clerk adds `offline_access` to the application by itself** and issues a
  refresh token even when the request does not ask for one. Whether this can
  be turned off was not checked.
- **`aud` is a list.** `jsonwebtoken` handles both shapes. A hand-written
  comparison with a string would have failed.
- **The callback names the issuer.** The client should check the `iss`
  parameter against the issuer it started with. The Android client does; the
  desktop program does not yet.
- **Tests can replace Clerk with a local key,** as the design planned. The
  verifier takes the key set as a parameter and never fetches it itself.
- **End-to-end sign-in can be automated in a development instance** with the
  `+clerk_test` addresses and the fixed code, on desktop and on the emulator.

### When sign-in does not come back

- **A wrong redirect target strands the user in the browser.** The client
  cannot rely on being called back with an error.
- **A late return strands the user too, on desktop.** If the client has
  stopped listening, the browser ends on a connection error and nothing says
  why. The desktop client should listen until the person cancels, not for a
  fixed time.
- **Android has no such limit.** The attempt is a file, so the redirect can
  arrive much later, in a new process.

### Android

- **The attempt must be saved before the browser opens.** With that, a killed
  app recovers with no extra code path: the cold start finds the redirect and
  the saved attempt, exactly as a warm return does.
- **An attempt is used once.** The app deletes the saved attempt before it
  exchanges the code, so a redirect delivered twice cannot be finished twice.
- **The activity is declared `singleTask`,** so that the redirect returns to
  the running activity. The trial did not test what happens without it.
- **The redirect reaches Rust through a file.** The activity writes the
  address and renames the file into place; the Rust side polls for it. It is
  crude and it worked. Step 7 can replace it with a JNI call.
- **`webbrowser::open` opens Chrome as a full tab,** not as an in-app custom
  tab. A custom tab would look more like part of the app; it needs a few
  lines of Kotlin.
- **Any app can register the same custom scheme.** A hostile app could
  receive the redirect. PKCE is what makes that harmless: the code is useless
  without the verifier, which never leaves our app. An https app link on a
  domain we own would remove the ambiguity and can be added when the
  production domain exists.
- **TLS on Android needs a deliberate choice.** `reqwest` 0.13 with its own
  rustls set-up checks certificates through a platform component that needs
  Kotlin code and a start-up call. Its native-TLS option needs OpenSSL built
  for Android. The trial gave `reqwest` a ready rustls configuration instead:
  `ring` for the cryptography and Mozilla's root list compiled in. That built
  with no extra tooling and worked. The cost is that the app trusts a root
  list fixed at build time and ignores certificates the user installed.

### Left open

- **The consent screen reads oddly for a first-party app.** The user signs in
  to Dirt and is then asked whether an application may "access Dirt" on their
  behalf, on every sign-in. The plan said to leave consent on. Whether Clerk
  allows it off for a public client, and whether that is wise, is a decision
  for step 6.
- **Clerk also offers the device-code grant.** A CLI on a machine with no
  browser, such as over SSH, cannot use the loopback redirect. The device-code
  grant would cover that case. It is off by default on the application and was
  not tried.
- **`jsonwebtoken` 11 needs a backend feature.** `rust_crypto` builds with no
  C compiler, on Windows and for Android. Reading a PEM key needs the separate
  `use_pem` feature; the tests pass DER instead.

## Effect on the design

- **Decision 9 stands.**
- **Section 5.1:** the client keeps the ID token only long enough to send it
  to our server and discards Clerk's other tokens. It checks the `iss`
  parameter of the callback. On desktop it listens until the person cancels.
  On Android it saves the attempt before opening the browser, uses it once,
  and returns through the custom scheme, which Clerk accepts.
- **Step 5** must choose the TLS set-up for Android deliberately.
- **Open for step 6:** the consent screen, and the device-code grant for the
  CLI without a browser.

## Not covered

- **A physical Android device, and browsers other than Chrome.** Whether
  another default browser hands the custom scheme to the app the same way is
  untested.
- **The Dioxus desktop app.** The flow ran from a terminal program.
- **A production Clerk instance.** Its session and consent behaviour may
  differ from a development instance, and it needs a domain we own (amendment
  A5).
- **Google sign-in with the app killed.** The kill run used an email code.

## Left in the Clerk development instance

- The OAuth application "Dirt trial (native)". Its client ID is in
  `.env.trials.local` as `CLERK_OAUTH_CLIENT_ID` and in the Android app's
  source. It has no secret.
- The test user `dirt.trial+clerk_test@example.com`.
- A user for the Google account used in the Google runs.

## How to rerun

```bash
cd trials/clerk-signin
cargo test

# Desktop. CLERK_ISSUER and CLERK_OAUTH_CLIENT_ID from .env.trials.local
cargo run --bin desktop

# Android (emulator running; ANDROID_HOME and NDK_HOME set)
cd ../clerk-signin-android
JAVA_HOME="$DIRT_ANDROID_JAVA_HOME" dx build --android \
  --target x86_64-linux-android --release
adb install -r target/dx/clerk-signin-android-trial/release/android/app/app/build/outputs/apk/debug/app-debug.apk
# Tap "Sign in" in the app, then:
adb shell run-as dev.dirt.trial.signin cat files/report.txt
```
