# Releasing

How a version of Dirt is built, checked and offered. The owner decided what
a release is for 0.1.0 (`docs/OWNER_DECISIONS.md`): offered to a few people
through GitHub Releases, for **Windows and Android only**, the Windows
installer unsigned, the Android app signed with a key of its own.

## What a release holds

| Asset | Made by | Notes |
|---|---|---|
| `Dirt-<version>-windows-x64-setup.exe` | `dx bundle --release --package-types nsis`, from `crates/dirt-desktop` | The desktop app with voice (the speech model inside), for the person installing it, into `%LOCALAPPDATA%\Programs\Dirt`. Unsigned. The script is `crates/dirt-desktop/installer/dirt.nsi.hbs` |
| `dirt-cli-<version>-windows-x64.zip` | `cargo build -p dirt-cli --release` | `dirt.exe`, `LICENSE` and `THIRD_PARTY_LICENSES.txt` |
| `Dirt-<version>-android-arm64-v8a.apk` | `dx build --platform android --release --target aarch64-linux-android`, signed again with the release key by `apksigner` | `dev.catjam.dirt`, versionCode 1. 64-bit ARM phones only |
| `SHA256SUMS` | The workflow | The SHA-256 of each asset |
| `LICENSE`, `THIRD_PARTY_NOTICES.md`, `THIRD_PARTY_LICENSES.txt` | The repository | What Dirt is under; the works it ships from others, with where each came from; and the full text of each one's licence (the speech model's CC BY 4.0 among them) |

Every build names the production server, `https://api.dirt.catjam.dev`
(`DIRT_API_BASE_URL` and its desktop and mobile variants in the workflow's
environment, which the build scripts read before `.env.client`). The
workflow checks that each program names it and not the trial server.

The licence texts (`THIRD_PARTY_LICENSES.txt`) also travel inside each
app: `dirt-voice`'s build copies the file into the voice folder beside
`NOTICE.txt`, so the installed desktop app has it under `assets\voice\` and
the APK under `assets/voice/`; the workflow checks both. Every Rust crate
the programs are built from is under its own licence, as each declares; a
generated list of their texts (`cargo about`, shipped with each program) is
later work, not done for 0.1.0.

The release notes are `docs/releases/<version>.md`, written for the people
who install it: how to install, what is in it, and what has not been tried
by hand.

## The workflow

`.github/workflows/release.yml` builds the three assets in three jobs, each
checking what it made (see the workflow's steps). None of the three sees
the release key: the APK comes out signed with a key made for that build
alone. Then one of two jobs ends the run:

- **A pushed tag `v<version>`**: `publish`, in the environment `release`,
  signs the APK again with the release key, checks that it is signed by
  that key alone, and makes a **draft** GitHub Release named
  "Dirt `<version>`" with the assets and the release notes as its text. A
  draft is seen only by the repository's owners. It is the only job with
  the key's secrets, and the only one that may write to the repository.
- **Run by hand** (Actions → Release → Run workflow), or **a pull request**
  that changes what the builds are made from (the workflow, the
  `Dioxus.toml` files and build scripts, the icons, the installer's
  script, the Android app's Rust, Kotlin, manifest, resources and R8 rules,
  the voice folder's notice, the licence texts, the release notes): a dry
  run. It builds and checks everything, never uses the release key, and
  publishes nothing. Its APK is named `…-NOT-RELEASE-SIGNED.apk`, and no
  installed Dirt takes it as an update. Its summary lists each asset's size
  and SHA-256, and what the APK holds (checklist A63).

Every run keeps what it built as workflow artifacts for one day only.

The tag must name the version in the root `Cargo.toml`
(`[workspace.package]`), and `docs/releases/<version>.md` must exist, or
nothing is built.

## Making a release

1. On `main`, raise `version` in the root `Cargo.toml` (every crate takes
   it; Android's versionName is it), write `docs/releases/<version>.md`, and
   merge both. The release's pull request runs the dry run: wait for it to
   pass.
2. Android's versionCode is written by `dx` as 1, and Android installs an
   update over an app only with a higher one. Before a second Android
   release, make the build give a higher versionCode (dx 0.7 has no
   setting for it): this is to be settled before 0.2.0.
3. Tag the merge and push the tag:

   ```bash
   git tag -a v0.1.0 -m "Dirt 0.1.0"
   git push origin v0.1.0
   ```

4. When the workflow has finished, open the draft under Releases. Check:
   - every job passed, and the Draft release job's summary says the APK
     was signed with **the release key** (its certificate's SHA-256 is
     there, the same each release);
   - the assets are all there, and `SHA256SUMS` lists each;
   - try the release builds by hand (`docs/MANUAL_CHECKLIST.md`, "Release
     builds", R1 and R2): the APK on a phone, the installer on Windows.
5. **Publish** the draft. This is the owner's step; nothing publishes a
   release by itself.

A failed run can be run again for the same tag: it replaces the draft's
assets and text. A published release is never changed by the workflow; a
mistake in one is mended by a new version.

## The Android release build is shrunk

A release APK is minified by R8 (`dx` turns it on), which takes away and
renames what nothing it can see uses. What only the app's Rust calls
through JNI is invisible to it: `crates/dirt-mobile/android/proguard-rules.pro`
keeps it, and `Dioxus.toml` names that file: AndroidX Security's classes,
`MainActivity`'s static methods, and the classes of `dev.dirt.capture`
whose native methods Rust provides (wry's own classes are kept by the
rules dx writes beside them). Without it, AndroidX Security
was gone from the APK and no session could be opened ("the device's secure
storage failed"), which only a release build shows. A new class or method
that Rust reaches by name goes into that file, and is tried in a release
build.

## The Android signing key

The release key is a PKCS12 keystore with one key, alias `dirt`, whose key
password is the store's. The alias is not secret and is written in the
workflow (`DIRT_ANDROID_KEY_ALIAS: dirt`): as a secret, GitHub masked the
word "dirt" in every log line. The keystore and its password are secrets of
the GitHub **environment `release`**, which only the `publish` job uses:

| Secret, in the environment `release` | What |
|---|---|
| `DIRT_ANDROID_KEYSTORE_B64` | The keystore, in base64 |
| `DIRT_ANDROID_KEYSTORE_PASSWORD` | Its password, the key's too |

Setting it up, once (the owner's, in the repository's Settings):

1. Settings → Environments → **New environment**, named exactly `release`.
2. Under **Deployment branches and tags**, choose **Selected branches and
   tags** and add one rule: type **Tag**, pattern `v*`. Nothing but a pushed
   `v*` tag can then run a job in it.
3. Add the two secrets above to the environment.
4. Delete the repository-level secrets `DIRT_ANDROID_KEYSTORE_B64`,
   `DIRT_ANDROID_KEYSTORE_PASSWORD` and `DIRT_ANDROID_KEY_ALIAS` (the first
   version of the workflow read them there; it reads none of them now), so
   that no other job can read the key.

Until the environment exists, GitHub makes an empty one the first time
`publish` runs, with no rule and no secrets: the job then stops, saying the
secrets are missing, and nothing is published.

The owner keeps the original keystore and its password off the repository
and off GitHub, where they can be found again. The workflow decodes the
keystore into the runner's temporary folder for the one step that signs,
and deletes it after; the password reaches `apksigner` and `keytool` from
the environment, never on a command line or in a file. `dx` itself is given
a key made for that build alone (it writes its key's password into the
Gradle project it makes), and the APK is then signed again with the release
key.

No dry run uses the release key, whoever starts it: a change that has not
been reviewed, merged and tagged can never come out as a valid update to
`dev.catjam.dirt`. A tag is never published without the release key.

**Losing the key means Android cannot update Dirt in place.** Android
installs an update only when it is signed by the same key as the app
installed. Without the key, a new APK would have to be a new app: everybody
would sync, uninstall Dirt and install it again, as the people who had a
development build do for 0.1.0. Keep the original safe, and replace the
secrets only with that same keystore.

## The icon

`assets/brand/cairn.svg` is the drawing. `node tools/icons/make-icons.mjs`
makes from it the Windows icon (`crates/dirt-desktop/icons/dirt.ico`, which
`dx` puts in the program and the installer, and which the window and the
taskbar take from the program) and Android's launcher icons (the native
module's `res/mipmap-*/dirt_launcher*` and their adaptive layers). Change
the drawing, run it, and commit both. It draws in the same headless Edge or
Chrome as the motion check.

## Builds for the owner's computer

A release build can be made locally the same way, with the environment the
workflow sets. A release build ignores the stand-ins for trying the app off
the screen (`docs/MANUAL_CHECKLIST.md`): its window, hotkey and browser are
real, so it is tried by a person.
