# benix-mdns-advertiser

BenixOS's box-side mDNS advertiser: publishes `_benixos._tcp` on the LAN so
Courier can discover a box and start the existing `TPair`/`RPair`/
`DPairClaim`/`DPairResult` pairing flow without an account/hub round-trip
first. This is the box-side half of messaging-architect's ruling in
`context/projects/benixos.md` §9c (in the `dlockamy/vault` workspace);
Courier's browse-side half is tracked separately as Task #30.

## Why this exists, and why here

Full ruling and reasoning: `context/projects/benixos.md` §9c/§9e. Short
version: mDNS is a discovery layer strictly *beneath* Hearth's presence/
pairing wire contract — it answers only "a BenixOS box is reachable here,"
never claim state or identity authoritatively. It replaces the QR-code
rendezvous step the (now-paused) compositor screen would have provided,
so headless boxes still have a way for Courier to find them.

**Placement**: its own repo, not vendored into `slash-builder/core`'s
`meta-benixos`. Reasoning, following the same precedent
`benix-compositor`/`benix-base` already set (DEV-9,
`devicenix-legacy/docs/build-model.md`'s two-tier model):

- This crate's only real dependencies (`mdns-sd`, `uuid`, `hostname`) are
  pure Rust plus plain libc socket syscalls — no dynamic linking against a
  target-image-specific native library stack. That's the *opposite* of
  `benix-compositor`'s situation (which does need an in-tree Yocto build
  against the sysroot's exact Mesa/libinput/libseat ABI) and matches
  `qr-gateway`/`qr-cli`'s already-proven shape: a clean static/musl
  cross-compile, verified in this project to take ~75s with zero code
  changes and produce a static-PIE binary that runs under pure-musl Alpine
  with no glibc at all (`benixos.md` §6i).
- A from-scratch Yocto/BitBake recipe building Rust source in-tree would
  also hit the same stock-toolchain wall the compositor did (oe-core
  kirkstone's `rust_1.59.0.bb` vs. a real dependency's MSRV, §6f) for no
  benefit — this crate builds trivially against a modern `rustup` toolchain,
  so there's no reason to fight the old one.
- Conclusion: build a musl release artifact in *this* repo's own CI (this
  repo), and give `meta-benixos` a thin recipe that fetches the prebuilt
  binary + a dinit unit — the same shape `qr-gateway`'s recipe already
  uses, not a vendored/local recipe source. **That recipe does not exist
  yet** — out of scope for this pass, see "Not done" below.

Small enough that a whole repo might look like overkill for ~150 lines of
code — noted, but going against the studio's own established two-tier
precedent to save one `git init` would be the inconsistent choice, not the
minimal one.

**Repo visibility: public, not private.** This repo started private and was
flipped to public during the `meta-benixos` Yocto-integration pass (2026-09-02)
after a real, reproduced bug: `meta-benixos`'s fetch-recipe uses BitBake's
anonymous `wget` fetcher, which cannot authenticate to a private GitHub
repo's release assets at all (confirmed live: `wget ... failed with exit
code 8, no output` in a real Jenkins build; independently reproduced with a
plain anonymous `curl` against the exact release URL, which 404'd before the
flip and 200'd after). This is the same reason `qr-gateway`'s own release
artifact lives in the *public* `gateway-downloads` repo rather than the
private `gateway` source repo — a prebuilt-artifact fetch-recipe needs a
publicly fetchable URL, full stop. Unlike `gateway`, this crate's source and
its release artifact are the same small repo (per the "small enough that a
whole repo might look like overkill" note above), so the fix here was
simpler: make the one repo public rather than splitting a second
`-downloads` repo. Checked before flipping: no secrets anywhere in this
repo's history, Apache-2.0 licensed, and the org's own public mission
statement (`slash-builder.github.io`) already frames this org as "the
open-source organization behind Hearth and BenixOS" — this crate fits that
mission as-is.

## What it does

One dinit-supervised binary, no subcommands, no config file:

1. Loads (or creates, on first run) a LAN-local id persisted to
   `<state dir>/mdns-id`.
2. Builds a 3-field TXT record (`id`, `claimed`, `pv`) and registers
   `_benixos._tcp` via `mdns-sd`'s `ServiceDaemon`, with the onboarding
   port carried in the SRV record (the `port` argument to
   `ServiceInfo::new`), not in TXT.
3. Blocks on the daemon's own event-monitor channel. If `mdns-sd` ever
   reports a `DaemonEvent::Error` or the channel closes, this process exits
   `1` and relies on dinit's `restart = true` / `smooth-recovery = true`
   (the same posture `qr-gateway`'s and `dockerd`'s dinit units already
   use) to bring it back and re-register, rather than sleeping forever
   having silently gone deaf.

Mechanism is exactly `messaging-architect`'s ruling: `mdns-sd` (crates.io,
pure safe Rust, no C libs, no D-Bus — required by DEV-17's no-D-Bus lock),
one background responder thread, channel-based API, no imposed async
runtime (`default-features = false` on `mdns-sd` drops its optional
`async`/`logging` features — this binary doesn't need either).

### TXT record — exactly three fields, per the ruling, no more

| Field | Value | Status |
|---|---|---|
| `id` | a UUIDv4 generated at first run, persisted to `<state dir>/mdns-id` | **Ruled shape** (`data-architect` Task #29, `benixos.md` §9u): a per-install, opaque, random 128-bit token — a discovery *handle*, not an identity. Decoupled from the fabric `device_id` by rule (broadcasting that in clear would hand every LAN observer the box's permanent global fabric identity; deriving from it is impossible anyway — an unclaimed box has no fabric id yet). Stable for one claim/install, **must be regenerated on factory reset / unclaim** (a contract on those paths, not this crate — `load_or_create` regenerates automatically once `<state dir>/mdns-id` is cleared). See `src/id.rs`'s module doc for the full ruling. |
| `claimed` | hardcoded `"0"` | **Advisory UI hint only, never authoritative.** `benix-claim-agent` (Task #23) doesn't exist as code yet, so there is no real claim state to read. See the `TODO(benix-claim-agent)` in `src/main.rs`. The box's own fail-closed claim-agent state machine, once built, is the sole gate — Courier must never treat this field as security-relevant. |
| `pv` | `"1"` | The actual, real starting protocol/schema version for this TXT shape. Bump it if the field set or meaning changes. |

The onboarding/connect port goes in the SRV record via
`BENIX_MDNS_PORT` (default `8420`) — also a placeholder, since no
onboarding endpoint is finalized yet (`benixos.md` §9d, gateway-pm's
`qr-web` musl feasibility spike is answered in principle but not yet run
on real Linux).

**Hard boundary this crate respects**: no credentials, no key material, no
claim-state authority ever goes into this TXT record or any future field.
That's messaging-architect's explicit line, not a decision made here.

## Configuration (env vars, all optional)

| Var | Default | Meaning |
|---|---|---|
| `BENIX_MDNS_STATE_DIR` | `/var/lib/benixos` | Where the persisted LAN-local id lives. |
| `BENIX_MDNS_PORT` | `8420` | Placeholder onboarding port, carried in the SRV record. |
| `BENIX_MDNS_INSTANCE` | this box's hostname | Override the mDNS instance name. |
| `RUST_LOG` | `info` | Standard `tracing-subscriber` env filter. |

## What was actually verified, and how

This environment is macOS/Darwin with no musl/Linux toolchain or real LAN
to test against — the same limitation on record throughout this project.
What was actually run, not just claimed:

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test` (2 unit tests covering `src/id.rs`'s persist-and-reload and
  empty-file-is-treated-as-missing behavior), and `cargo build --release`
  all green against the **host** target (`aarch64-apple-darwin`).
- `cargo build --target x86_64-unknown-linux-musl` (std for that target was
  already installed via `rustup`): every crate in the dependency graph,
  including this one, **compiled and codegen'd clean** for the musl
  target. Linking then failed, but only because this Darwin host has no
  musl/GNU cross-linker available (`cc`/`ld` here is Apple's, which
  doesn't understand the GNU linker flags rustc emits for an ELF target) —
  an expected host-tooling gap, not a code problem. This is stronger
  evidence than "untested" but it is **not** a real musl binary in hand.
- The GitHub Actions CI in this repo (`.github/workflows/ci.yml`) runs
  `fmt`/`clippy`/`test`/`build` **and** a real `cargo build --release
  --target x86_64-unknown-linux-musl` on `ubuntu-latest` (which does have a
  working musl cross-linker via `musl-tools`). **Confirmed green, both
  jobs, run `33577081795`**: the musl job produced a real
  `static-pie linked` ELF (verified via `file(1)`'s own ELF classification,
  not `ldd` — `ldd` misidentified the first attempt's genuinely-static
  binary as dynamic, a bug in the check script that was found and fixed in
  a follow-up commit, not a build problem; see the repo's commit history).
  This is a real compiled musl binary from this exact source, not an
  assertion — see the repo's own Actions tab for the current state; don't
  take this README's word over that if they ever disagree.

**Not verified, not claimed as done**:

- No real LAN broadcast test. No `avahi-browse`/`dns-sd`-from-another-
  machine confirmation that a running instance is actually visible on a
  real network, or that the wire-format TXT/SRV records parse the way
  Courier's future browse side expects.
- No dinit-unit / Yocto (`meta-benixos`) integration — this binary is not
  on any image yet. Deliberately out of scope for this pass (see below);
  it's a real, separate next step, not folded in. **Update**: this is now
  done in `slash-builder/core` (`meta-benixos/recipes-connectivity/
  benix-mdns-advertiser`) — see that repo for the recipe and dinit unit.
- No musl release artifact has ever been published anywhere (GitHub
  Releases or otherwise) — only compiled in CI, not packaged/tagged.
  **Update**: fixed by `.github/workflows/release.yml` (see "Known gaps"
  below) — check the repo's Releases tab for the actual current tag
  `meta-benixos`'s recipe is pinned to.

## Known gaps / explicitly deferred, not silently dropped

- **No dinit unit, no `meta-benixos` recipe.** Once one exists, it should
  be a plain `process`-type unit with no isolation wrapper, matching
  `qr-gateway`'s and `dockerd`'s existing units (`slash-builder/core` PR
  #19) — no reason for this one to be different. Ordering: after whatever
  brings up the network interface(s) `mdns-sd`'s auto-address-detection
  would enumerate; this crate doesn't gate on that today (`mdns-sd`'s
  auto-detect just won't find any usable interface yet if run too early —
  worth a real ordering decision once the recipe is written, not decided
  here).
- **No GitHub Release / tag-triggered publish job.** ~~`qr-gateway`/`qr-cli`
  went through the same two-step sequence (dinit units landed first
  against an existing artifact, a musl release target added second, PR
  #44) — this crate is at the first step's equivalent (code + CI proving
  the musl build works), not yet the second.~~ **Fixed** — `.github/
  workflows/release.yml` builds the musl release artifact on any `v*.*.*`
  tag push and publishes a GitHub Release (`benix-mdns-advertiser-linux-x64.tar.gz`
  + `SHA256SUMS`), same tarball+checksum shape as `gateway-downloads`'
  existing releases. See the repo's Releases tab for the actual published
  artifact `meta-benixos`'s recipe fetches — don't take this README's word
  over that if they disagree.
- **`id` is now the ruled shape** (data-architect Task #29, `benixos.md`
  §9u), not a placeholder — its generation/persistence here is correct; the
  only remaining half is a rotation-on-reset/unclaim contract on the
  reset/unclaim paths (which don't exist yet). **`claimed` remains an
  explicitly-named placeholder** (advisory hint, `benix-claim-agent` Task #23
  not yet reading real state), not a quietly-shipped final decision.

## Open, routed rather than decided here

- ~~`data-architect` — the real shape of the `id` TXT field (Task #29).~~
  **RULED** 2026-09-02 — `benixos.md` §9u (see the `id` table row above).
- `benix-claim-agent` (`software-developer`) — wire the unclaim path to
  delete `<state dir>/mdns-id`, and ensure any future factory-reset flow
  wipes it (or the whole state dir), so a re-claimed box advertises a fresh
  `id` per §9u's rotation-on-ownership-boundary rule.
- `benixos-pm` — sequencing this into the Stage 1 headless backlog and the
  `meta-benixos` recipe/dinit-unit work.
- `devops-engineer` — the dinit unit itself, its ordering against network
  bring-up, and triggering a real musl release build once a recipe exists.
