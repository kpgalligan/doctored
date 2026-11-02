# A published build updates itself through Sparkle (ADR)

- **Status:** Accepted — shipped 2026-11-01 in one batch; the install of an update is not yet proven (Open)
- **Source:** a port of Frame Job's integration, `kpgalligan/framejob 117687d6` (the updater, its checks and the workflow) and `kpgalligan/framejob a9bba16f` (its public key), adapted to Doctored's build, gate and launch model. Every Frame Job commit or run here carries the `kpgalligan/framejob` prefix; a bare hash would read as one from the private archive.
- **Related:** [agent-socket.md](agent-socket.md) MC-4 (the helper's launch marker); `.github/workflows/publish.yml`, whose header lists the secrets.

## What ships

- **Doctored ▸ Check for Updates…**, in every build, enabled only in a launch that has an updater.
- In a published build, Sparkle 2.10.1 with its own defaults: it asks on the second launch whether to check automatically, checks daily when allowed, and never installs without asking unless the user ticks its automatic-update box.
- The feed `https://github.com/kpgalligan/doctored/releases/latest/download/appcast.xml`: one item, the release's DMG, signed with EdDSA over the stapled DMG. The publish workflow writes it (`scripts/appcast.py`) and uploads it beside `Doctored.dmg`.
- 0.4.1 and 0.4.2 have no updater; their users replace the app by hand once (owner task 3).

## Decisions (UP-1..UP-10)

- **UP-1 — A third configuration, `Distribution: release`, is what ships.** Release is what runs locally (the gate's `launch`, the Release-only drives, `make run`, Xcode's Run), signed ad hoc with the hardened runtime, and dyld refuses an ad-hoc framework in such a build: "mapping process and mapped file (non-platform) have different Team IDs" (measured in Frame Job, 2026-10-27). So Release never loads Sparkle. CI-only build settings on the workflow's `xcodebuild archive` line were rejected: they would be a second copy of the shipping configuration that drifts from any local reproduction. Distribution sets only `DR_UPDATER`, the feed and the public key, so it is Release plus the updater; the workflow archives it and `make dist` builds it. It is never launched as built: `scripts/app_under_test.py` refuses its path whatever `DR_ALLOW_STALE_APP` says. The one local launch is `scripts/updater_check.py` D9's scratch copy (Verification).
- **UP-2 — `#if DR_UPDATER`, not `#if canImport(Sparkle)`.** The package is linked per target, so `canImport` would be true in Debug and Release Xcode builds and bring the load command back; `make typecheck` and `scripts/probe_build.py` compile `app/Sources` with no Sparkle module. No Sparkle name appears outside `#if DR_UPDATER` in `AppUpdater.swift`, `DR_UPDATER` is never in a `ProbeSet.defines`, and `updater_check.py`'s selftest T1 fails a `project.yml` that sets the feed, the key or `DR_UPDATER` (as a word or a `-DDR_UPDATER` flag) in any `settings:` block but Distribution's, or a Distribution key other than `updater_check.PUBLIC_KEY`.
- **UP-3 — `-Xlinker -dead_strip_dylibs` in the base `OTHER_LDFLAGS`** (Frame Job sets it per configuration). The framework is embedded in every configuration; the flag drops its load command wherever nothing references it, and keeps Distribution's loads exactly Release's plus `@rpath/Sparkle.framework/Versions/B/Sparkle` (D6). Measured at the batch: `otool -L` of the Release and Debug binaries was identical before and after the change, 39 entries each, so **no load command was restored**. If an entry ever disappears it is a stop, restored with `-Xlinker -needed_framework -Xlinker <Name>` or `-Xlinker -needed-l<name>` and recorded here, never "proven unused": a Swift overlay that only supplies conformances can have no referenced symbol and still be needed by a dynamic cast.
- **UP-4 — The menu item is in every build** (`AppDelegate+Updates.swift`, validated by `validateUpdatesItem`), so the probes, `RegistryDump`, the Shortcuts tab and the command palette see one menu tree. It sits on `AppDelegate` because it must work with no document open. It has no key equivalent, so `expectedDefaultChords` and the audit table in [v1-shortcuts-fade.md](v1-shortcuts-fade.md) are unchanged.
- **UP-5 — `enum AppUpdater` with static state**, not a stored property on `AppDelegate`, which keeps no stored state and which probes construct directly.
- **UP-6 — One pure function, `AppUpdater.availability`, decides each launch.**
  - `.absent`: no `DR_UPDATER`, a `SUFeedURL` that is not `https://`, a `SUPublicEDKey` that is not strict base64 of 32 bytes, or a `CFBundleVersion` that is not a positive integer (local builds read `"0"`, which would make every release look newer).
  - `.automation`: `AutomationLaunch.isAutomation` (any non-empty `RZ_*`, `DR_BACKGROUND` or `DR_AUTOMATION_ALERTS`, or a `DR_AGENT_SOCKET` other than the default; lifted unchanged from `DataMigration`). The updater is never constructed, so Sparkle writes nothing into `com.kgalligan.Doctored`, never reaches the network, and never shows a window: `AutomationAlerts` answers `NSAlert`s, not Sparkle's windows.
  - `.deferred`: argv holds `-DRMCPHelperLaunch`, which `doctored-mcp` appends to every launch it makes. It starts straight away if the app is already active, else at the first `didBecomeActiveNotification` or from the menu item. Why: Sparkle's standard driver may hold scheduled alerts in an inactive app, but nothing holds the second-launch permission prompt, which would otherwise appear in an instance nobody opened.
  - `.started` otherwise, in `applicationDidFinishLaunching` after `bindSocket()`.
  - Distribution writes one `[AppUpdater] <case>` stderr line per decision, plus `[AppUpdater] started` when a deferred one starts; D9 reads it. Debug and Release write nothing.
- **UP-7 — No MCP mirror.** The same exemption as the view preferences (app/CLAUDE.md): app-wide, so a `document_id` would be meaningless; no undo step; nothing a `save_copy` carries; and an install quits the app, which no agent may start. The catalog, the parity check and `mcp-tools.md` are unchanged.
- **UP-8 — Fixed forever in every shipped copy:** the repository, `releases/latest/download/appcast.xml`, the asset names `Doctored.dmg` and `appcast.xml`, and the name of `publish.yml`. Its run counter is `CFBundleVersion`, which Sparkle compares: renaming or recreating the file restarts the counter and stops updates, so a forced recreation adds an offset to `BUILD_NUMBER`. Latest is always the highest run and carries an appcast: a run that creates a release older than the current Latest creates it with `--latest=false`, and a re-run replaces both assets and leaves Latest alone. No delta updates; release notes only as `sparkle:fullReleaseNotesLink`.
- **UP-9 — Sparkle's defaults.** No `SUEnableAutomaticChecks`, `SUAutomaticallyUpdate`, `SUVerifyUpdateBeforeExtraction` or `SURequireSignedFeed`: the last two exist in 2.10.1 but are unproven in Frame Job, and a misconfiguration would strand installed copies. No Settings ▸ Updates tab (owner decision): the controls are the permission prompt, the update alert's automatic-update checkbox and `defaults write com.kgalligan.Doctored SUEnableAutomaticChecks -bool NO`.
- **UP-10 — The key check Frame Job lacks.** Before anything is signed, `appcast.py write` refuses a key file whose Ed25519 public half is not the exported app's `SUPublicEDKey`: a secret and a committed public key from different keys would make every installed copy reject every update, and only a manual download would recover. The derivation is pure Python per RFC 8032 (stdlib `hashlib`), selftested against its test vectors. The file's ASCII whitespace is stripped, then it must decode to what `generate_keys -x` exports: the 32-byte seed, or, for a key made in Sparkle's older format, 96 bytes (orlp ed25519's 64-byte expanded private key, then the public key), accepted only when its clamped scalar makes its own public half. Nobody has checked which format the reused key is stored in, `sign_update` reads both (`common_cli/Secret.swift`), and an older key's seed cannot be recovered, so refusing that format would block every publish. The key is read into memory only, never printed and never in argv; a refusal names only the two public keys. `write` also refuses a `CFBundleVersion` that is not a positive integer and a `--url` that is not `https://` or lacks `/v<short>/`. `appcast.py check-key`, the workflow's step straight after checkout, makes the same match from the secret against Distribution's `DR_SPARKLE_PUBLIC_KEY` in `project.yml`, so a wrong secret fails before the archive and notarization; the sign step keeps the match against the exported app. `updater_check.PUBLIC_KEY` pins the decided key in T1 and D4 as well, so a rotation changes it and `project.yml` in one reviewed commit; a key changed together with its secret alone would pass every other check and strand every installed copy.

## The key: reused from Frame Job

- **One key, two apps** (owner decision, 2026-11-01). Distribution's `DR_SPARKLE_PUBLIC_KEY` in `project.yml` is Frame Job's public key, `v0qZeH0YUq+E1zznO2RjbaKZcIMuKSWM1hfEAuz7otI=` (`kpgalligan/framejob a9bba16f`), so the batch landed with the real key and there was never an empty-key state. The format the private key is stored in (the 32-byte seed or the older 96 bytes) is unrecorded; UP-10 accepts either. Sparkle's own help says one key can serve many apps.
- **Accepted risk:** the same private key sits in both repositories' `publish` secrets, so a leak of either lets someone sign updates that both apps accept, and a rotation changes both apps at once.
- **Custody:** the private key is the owner's alone: the login Keychain item "Private key for signing Sparkle updates" (the one `generate_keys -p` reads with no `--account`), a backup copy, and the `SPARKLE_ED_PRIVATE_KEY` secret in the `publish` environment. It is never in the tree. No agent generates, reads, exports or prints it, or touches the Keychain item. Losing it means installed copies can never be updated again; a new key needs every user to download once by hand.
- Publishing refuses until the secret is set: the workflow's first step, "Check the update key", fails before checkout, and "Match the update key" fails straight after it when the secret's public half is not Distribution's key.

## Launch matrix

- Debug and Release (`make app-debug`, `make app`, `make run`, the gate's `launch`, every drive, Xcode's Run), `make typecheck` and every probe: no Sparkle code compiled, no Sparkle load command, no updater. The gate's rows R1–R5 hold the Release bundle to it.
- Distribution as built (`make dist`): compiled and loaded, never launched.
- `updater_check.py` D9's scratch copy of Distribution: compiled and loaded, `.automation`, nothing constructed.
- Write Agent Catalog (every configuration, the CI archive): runs the executable re-signed without the runtime with `-DRWriteCatalog`, which exits in `main.swift`'s first statement.
- `DR_REGISTRY_DUMP` (Debug): exits in `applicationWillFinishLaunching`.
- A published copy: `.started` on an ordinary launch; `.automation` under `DR_BACKGROUND`, `DR_AUTOMATION_ALERTS`, a scratch `DR_AGENT_SOCKET` or `RZ_*`, which includes a `doctored-mcp` launch on a scratch socket; `.deferred` when `doctored-mcp` opens it on the person's socket, until the first activation or Check for Updates….

## Proof status

Frame Job's publish runs since `kpgalligan/framejob a9bba16f` prove that its workflow publishes, notarizes and writes a signed appcast (its live feed reads `sparkle:version` 22 with an 88-character `edSignature`). They do not prove that an installed copy updates itself: Frame Job's manual check REL-4 has no recorded result, and the `kpgalligan/framejob 117687d6` message lists it as owed. Installing N+1 over N, Sparkle's code-signature match and the relaunch are unproven in both apps until owner task 4.

## Measured pitfalls

- An ad-hoc hardened build cannot load an ad-hoc framework ("different Team IDs"; Frame Job, 2026-10-27): only Distribution references Sparkle, dead-strip everywhere else, R1–R2 in the gate, Distribution never launched as built.
- String literals of 15 bytes or fewer are stored inline in `-O` code (Frame Job): the needle R4 and D5 search for is the 28-byte class name `SPUStandardUpdaterController`, which no literal outside `#if DR_UPDATER` may hold.
- The archive strips symbols, so `nm` is useless (Frame Job: 2021 symbols against the Distribution build's 89275): no `nm` row.
- A plist edit breaks the seal (Frame Job): the mutants and D9 re-seal after every edit.
- `sign_update` without `--ed-key-file` reads the Keychain (the tool's help): `appcast.py` always passes the file, and its selftest uses a stand-in.
- Xcode skips CodeSign on a no-op build (Frame Job's B88): Doctored's Write Agent Catalog already re-seals.
- `hdiutil -srcfolder`'s size estimate can come out short ("No space left on device", `kpgalligan/framejob run 40205362450`, fixed in `kpgalligan/framejob 00381860`): the DMG step passes an explicit `-size`, the staged folder's plus a quarter and 64 MB.
- A stored secret may carry a trailing newline: UP-10 strips ASCII whitespace, and the selftest covers `"\n"` and `"\r\n"`.

## Verification

- `make release-check`, also the gate's `release` step (no build, no network, no key): `appcast selftest: 7/7`, `updater_check selftest: 4/4`.
- `make gate`'s `launch` step holds the Release bundle to R1–R5: Sparkle.framework embedded (R1), no Sparkle load command (R2), the feed and key present and empty (R3), no `SPUStandardUpdaterController` (R4), the seal verifies (R5).
- `make dist`, after `make gate` passes and never while it runs: the Distribution build, then `updater_check.py dist` (D1–D9 over Distribution, R1–R5 over Release) and `mutants` (eight cases, each failing exactly its declared rows). D9 `ditto`s Distribution to a scratch folder, sets `CFBundleVersion` 1 and the bundle id `com.kgalligan.doctored.updater-check`, re-signs it without the runtime, launches it with `verification_isolation.owned_environment`, and requires `tools/list` within 60 s, `[AppUpdater] automation`, no `[AppUpdater] started` and no `SU*` key in that bundle id's defaults domain, which it deletes with `defaults delete` before the launch and after (then unlinking the empty plist cfprefsd leaves); it is the one local proof that the binary with Sparkle loaded runs. At the batch: `updater_check dist: 14/14`, `updater_check mutants: 8/8`.
- `python3 scripts/updater-probe/verify.py`: the `availability` matrix, `isPublicKey`, the menu item, `.absent` in a build without `DR_UPDATER`, and `AppUpdater.begin`'s transitions against the stand-in `start()` such a build counts: a deferred updater waits, starts once at the first activation or from Check for Updates…, and stops listening; `.automation` and `.absent` start nothing. `make mcp-test` covers the launch marker.
- CI only, every publish run: `appcast.py check-key` after checkout, `updater_check.py published` after the export (D1 and D2 with a real team, D3–D5, D7, D8, P1, P2), notarization with Sparkle inside, and `sign_update` with the real key behind UP-10's match. After the first Sparkle run, `curl -sL https://github.com/kpgalligan/doctored/releases/latest/download/appcast.xml` shows `sparkle:version` equal to the run number and an 88-character `edSignature`.

## Owner's one-time tasks (exact commands)

1. **Confirm the reused key and store it as the secret.** `generate_keys` is the Sparkle package's own 2.10.1 tool, in `build/SourcePackages` after the first build (Frame Job's copy at `~/devel/framejob/build/SourcePackages/artifacts/sparkle/Sparkle/bin/generate_keys` is the same). No `--account`: Frame Job's documented recipe made its key in Sparkle's default account, and `-p` confirms it before anything is exported.
   ```sh
   GK=~/devel/doctored/build/SourcePackages/artifacts/sparkle/Sparkle/bin/generate_keys
   "$GK" -p          # must print v0qZeH0YUq+E1zznO2RjbaKZcIMuKSWM1hfEAuz7otI=; stop if it prints anything else
   DIR="$(mktemp -d)"
   "$GK" -x "$DIR/key"
   gh secret set SPARKLE_ED_PRIVATE_KEY --env publish --repo kpgalligan/doctored < "$DIR/key"
   rm -rf "$DIR"
   gh secret list --env publish --repo kpgalligan/doctored   # SPARKLE_ED_PRIVATE_KEY is listed
   ```
2. **Keep the Keychain item** "Private key for signing Sparkle updates" and a backup copy, such as a `-x` export in a password manager.
3. **After the first Sparkle release's publish run**, prepend one line for 0.4.1 and 0.4.2 users (replace `v0.4.N` with that release's tag):
   ```sh
   TAG=v0.4.N
   NOTES="$(mktemp)"
   { printf '%s\n\n' 'This is the first build that updates itself (Doctored ▸ Check for Updates…). If you run 0.4.1 or 0.4.2, download this DMG once and replace the app; later releases arrive through the updater.'
     gh release view "$TAG" --repo kpgalligan/doctored --json body -q .body; } > "$NOTES"
   gh release edit "$TAG" --repo kpgalligan/doctored --notes-file "$NOTES"
   rm -f "$NOTES"
   ```
4. **After the second Sparkle release**, the update itself: install build N from its DMG into `/Applications`; launch it twice (the second launch shows Sparkle's permission prompt); publish N+1; choose Doctored ▸ Check for Updates…, then Install Update; after the relaunch, About shows N+1, and Settings, the API key file and the MCP line are intact.

## Accepted risks

- **The shared key** (above).
- **An MCP call during an install.** Sparkle quits the app; a call that arrives before the relaunch makes the helper run `open -n -g` mid-install, which can leave a hidden instance holding the socket, or a 60 s timeout that strands that session.
- **Helpers from 0.4.1 or 0.4.2 still running.** A `doctored-mcp` an MCP client started before the person replaced the app keeps its old code. It resolves its bundle again at each launch (`own_bundle`), so it launches the new app on the person's socket without `-DRMCPHelperLaunch`, and that instance is `.started` until the client restarts the helper.
- **The upload gap on a re-run.** A re-run uploads the DMG, then the appcast; a check in between fails signature verification, a safe failure.
- **Latest set by hand.** Marking an older release, or one without an appcast, as Latest breaks the feed for every copy until it is fixed.
- **Running from the DMG.** Sparkle cannot update a copy run from the disk image or a translocated copy; users drag it to `/Applications`.
- **A deferred instance that nobody activates never checks.**
- **Network on the first build.** The first build in every lane fetches Sparkle into `build/SourcePackages`; whether resolution works offline after `make project` regenerates the project is unmeasured.

## Open

- OPEN — Confirm a published build updates itself (owner task 4: install N, publish N+1, Check for Updates…, Install Update): [#24](https://github.com/kpgalligan/doctored/issues/24).

## Where it lives

`project.yml` (the Distribution configuration, the Sparkle package pin, `-dead_strip_dylibs`, the two Info.plist keys, the scheme's archive configuration); `app/Sources/AppUpdater.swift`, `AppDelegate+Updates.swift`, `AutomationLaunch.swift`, the Doctored menu in `AppDelegate+AppMenu.swift`; `app/Resources/Acknowledgements.txt` and `Credits.html` (Sparkle's licence and notices, in every build); `mcp-proxy/src/launch.rs` (`HELPER_LAUNCH_ARGUMENT`); `.github/workflows/publish.yml`; `scripts/appcast.py`, `scripts/updater_check.py`, `scripts/updater-probe/`, `verification_isolation.owned_environment`, the gate's `release` step and its `launch` rows; the Makefile's `dist` and `release-check`.
