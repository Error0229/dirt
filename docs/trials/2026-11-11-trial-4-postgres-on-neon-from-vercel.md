# Trial 4: Postgres on Neon from the Vercel Rust runtime

- **Date:** 2026-11-11
- **Verdict:** pass on every criterion. The cost measurement needs a decision
  from you (see "Cost of polling")
- **Code:** `trials/vercel-neon/` on the `trials/step-1` branch
- **Plan:** `docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md`, Step 1
- **Tracking issue:** #237

## Question

The redesign runs the server as one axum function on Vercel, talking to
Postgres on Neon through `sqlx` (decisions 10 and 11). Does that function
reach the database, run the real push transaction correctly when many
requests arrive at once, and stay within acceptable latency and cost?

## Verdict

It works, and it is fast. The free plans cannot carry clients that poll all
day.

| Criterion | Threshold | Measured |
|---|---|---|
| Builds on Vercel with a Rust that `sqlx` accepts | builds | Builds once the project names its toolchain. The builder's own Rust is 1.92.0 and `sqlx` 0.9 needs 1.94 |
| 50 pushes at once on the same base | exactly 1 accepted | 1 accepted, 49 stale |
| 500 pushes at once to different notes | change numbers strictly increasing, no duplicates | all 500 accepted, numbered 1 to 500 |
| Prepared-statement errors over 200 requests spanning a redeploy | none | none, with default settings |
| Warm request, 95th percentile | at most 500 ms | 59 ms for a pull, 61 ms for a push |
| First request after more than six idle minutes | at most 5 s | 0.99 s, twice |

Decisions 10 and 11 stand. No fallback was needed: `sqlx` 0.9 was kept.

## Method

- **Versions:** axum 0.8.9, sqlx 0.9.0, vercel_runtime 2.4.1, Rust 1.99.1,
  Postgres 18.6 on Neon and in the local and CI containers.
- **The function:** one binary with the whole router, as the real server will
  have. It builds a connection pool before it starts serving and connects on
  the first query. Pool settings as planned: at most 3 connections, none kept
  when idle for 10 s, Neon's pooled endpoint, TLS required.
- **Schema and queries:** the plan's two tables. Push is one transaction that
  takes the next change number from the account row and writes the note only
  if its change number still equals the base the client sent. Pull is two
  queries: the account's counter, then the notes changed after a given number.
- **Where it ran:** the `dirt-trial` Vercel project on the Hobby plan, function
  region `pdx1`. The Neon project is in AWS US West 2 on the free plan, with a
  fixed 0.25 CU compute. The live `dirt-api` project was not touched.
- **Access:** the deployment is public, so every route but the health check
  needs a shared secret header.
- **Load:** a script on the development machine. Each response carries the
  time the function spent in the database, so network time and database time
  can be told apart.
- **Tests:** the same queries under `#[sqlx::test]`, against Postgres 18 in
  Docker locally and in a GitHub Actions service container.

## Results

### Build

| | |
|---|---|
| Rust on Vercel's builder | 1.92.0 |
| First build, no cache | 2 min 52 s |
| Rebuild with cache | 44 s |
| Function size | 2.61 MB |
| Build cache | 187 MB of a 1.5 GB limit |
| Build machine | 2 cores, 8 GB, in `iad1` whatever the function region |

The first deployment failed in eight seconds: `sqlx` 0.9 requires Rust 1.94.
Adding a `rust-toolchain.toml` that names 1.99.1 fixed it. The builder's cargo
goes through rustup, which downloads the named toolchain.

### Correctness when requests arrive together

| Run | Accepted | Stale | Errors | Function instances used |
|---|---|---|---|---|
| 50 pushes replacing one note, same base | 1 | 49 | 0 | 44 |
| 50 pushes creating one note | 1 | 49 | 0 | 48 |
| 500 pushes to different notes, one account | 500 | 0 | 0 | 156 |

- The 500 pushes took the change numbers 1 to 500 with none missing or
  repeated, and paging through them with pull returned all 500 in order.
- The 500 pushes finished in 2.9 s, including starting 156 instances.
- A rejected push rolls back, so it does not use up a change number.
- The 12 local tests cover the same cases. Removing the version check makes
  four of them fail, so they do test it.

### Across a redeploy

200 requests, one every 1.5 s, alternating push and pull, while a new build
went out. All 200 returned 200. Request 46 was the first served by the new
deployment. There was no prepared-statement error and no failed request at
the switch.

### Latency

Measured from the development machine. The health check has no database work,
so it shows the network and platform share.

| Request, function warm, database awake (100 each) | Median | 95th percentile | Slowest | Of which in the database (median) |
|---|---|---|---|---|
| Health check | 41 ms | 54 ms | 66 ms | none |
| Pull | 44 ms | 59 ms | 92 ms | 3 ms |
| Push | 48 ms | 61 ms | 91 ms | 4 ms |

| First request after 7 idle minutes | Total | In the database |
|---|---|---|
| Round 1 | 987 ms | 739 ms |
| Round 2 | 985 ms | 698 ms |

The database's start time changed between the rounds, which confirms Neon
had suspended it and started it again.

### Size limits

| | |
|---|---|
| Largest request body accepted | 4,493,163 bytes |
| Smallest request body rejected | 4,494,140 bytes |
| What a rejected request gets | `413`, body `FUNCTION_PAYLOAD_TOO_LARGE`, from Vercel before the function runs |
| Largest response body tried | 16,000,000 bytes, returned in full |

The request limit is 4.5 MB counted in units of 1,000, less about 6 KB that
the headers take. The 4.5 MB limit did not apply to responses in this
runtime.

### Cost of polling

One client polled every 30 seconds, as an open app does. The run was meant to
last two hours. The development machine went to sleep after 76.5 minutes
despite a keep-awake request, so the run has 154 polls instead of 232. It was
not repeated: the result does not depend on the length.

| | |
|---|---|
| Polls | 154 over 76.5 minutes, all answered 200 |
| Times Neon suspended the database during the run | none. The database start time was the same in every poll |
| Poll latency, median and 95th percentile | 136 ms and 216 ms |
| Of which in the database | 78 ms and 144 ms |
| Compute size | 0.25 CU, fixed |
| Neon console, compute used this billing period, read the next morning | 0.46 CU-hours |

The console figure covers the whole evening, not only the polling run. It
matches the database being awake for about 1.8 hours at 0.25 CU: the polling
run, the five idle minutes after it, and the earlier tests.

What follows from it:

- **A polling client keeps the database awake for as long as it is open.**
  Neon suspends after five idle minutes, and polls are 30 seconds apart.
- **One client left open all month uses 182.5 CU-hours**: 730 hours at 0.25
  CU. The free plan includes 100.
- **The free plan covers 400 awake hours a month, about 13 a day.** That
  budget is shared by all users together, because the database is awake
  whenever any one client anywhere is open.
- **On Neon's Launch plan an always-awake database of this size costs about
  $19 a month**, at $0.106 per CU-hour.
- **Polling a little less often does not help.** Any interval under five
  minutes keeps the database awake all the time. One always-open client fits
  the free plan only with an interval of nine minutes or more.
- **Vercel counts too.** One always-open client makes 86,400 requests a
  month. The Hobby plan includes 1,000,000 function invocations, which is
  about 11 such clients. Hobby is also barred from commercial use (amendment
  A1).

#### Decision needed

The design says a client polls on a 30-second timer (decision 4). The options:

| | Option | Effect |
|---|---|---|
| A | Keep the timer running whenever the app is open | Simplest. Needs the paid Neon plan from the first users who leave the app open |
| B | Run the timer only while the app is in use: stop it after a few minutes without input or when the window is hidden, and sync at once on return | Awake time follows real use. The free plan lasts until use across all users passes about 13 hours a day. No change to the protocol or the server |
| C | Lengthen the timer to ten minutes | Fits the free plan, but a second device can be ten minutes behind |

I recommend B. Sync on startup and after each local change stay as they are,
so a person who is typing still sees their other device's changes within 30
seconds. A paid plan is still expected once there are real users (amendment
A2); B decides how soon.

## Findings

- **The project must name its Rust toolchain.** Vercel's builder ships 1.92.0.
  A `rust-toolchain.toml` in the function's root folder is enough. The
  repository's own pin (pull request #247) will have this effect on the live
  `dirt-api` project the next time it deploys.
- **Vercel starts close to one instance per request in a burst.** 500
  requests at once ran on 156 instances, and 50 on 44 or more, although Fluid
  compute is on for the project. Each instance has its own pool, so the pool's
  maximum of 3 limits little. What limits connections is the number of
  instances. Neon's pooled endpoint absorbed this. The direct endpoint allows
  112 connections at this compute size and would not have.
- **The pooled endpoint works with `sqlx` defaults.** No prepared-statement
  error appeared in about 2,000 requests, so the `statement-cache-capacity=0`
  setting is not needed.
- **Migrations ran through the direct endpoint.** Running them through the
  pooled endpoint was not tried.
- **The plan's push statement had a gap.** It inserted a missing note
  whatever base the client sent. The trial's statement inserts only when the
  base is 0 or the note exists, so a non-zero base for a note the server does
  not have is stale. A test covers it. Step 4 should use this statement in
  place of the plan's.
- **One account's pushes run one at a time, as intended.** The account row
  lock orders them. At about 4 ms each, one account can take roughly 250
  pushes a second, far more than a person produces.
- **A 30-second poll reconnects every time.** The pool drops connections idle
  for 10 s, and polls are 30 s apart, so each poll opens a new connection.
  That costs about 75 ms: the database share of a poll was 78 ms at the median
  against 3 ms for back-to-back requests. It is not a problem at this scale.
  Step 4 can choose between leaving it and keeping one connection for longer.
- **The response limit is not the request limit.** Pull pages are still capped
  by bytes (amendment A3), which keeps pulls and pushes symmetric and
  independent of this host.

## Effect on the design

- **Decisions 10 and 11 stand:** Postgres on Neon, axum on Vercel.
- **Section 7.5:** add that the deployed project must carry a
  `rust-toolchain.toml`, and that Vercel runs about one instance per
  concurrent request.
- **Section 7.4:** state that the server always uses the pooled endpoint and
  migrations always use the direct one, and record the measured wake-up time
  and compute use.
- **Section 7.6:** the request limit is confirmed at 4.5 MB. The 3 MB batch
  cap from amendment A3 leaves a third of it spare.
- **Polling:** see the decision under "Cost of polling".

## Not covered

- **A full two hours of polling.** The run ended at 76.5 minutes, as
  described under "Cost of polling".
- **Migrations through the pooled endpoint**, as noted above.
- **Work after the response.** The report that Vercel suspends a Rust
  instance right after it responds was not tested. The trial does nothing
  after responding, and the real server must not either.
- **Other regions.** Latency was measured from one machine. A client far from
  the US west coast adds its own network time to every number here.

## How to rerun

```bash
cd trials/vercel-neon

# Local tests
docker compose up -d
DATABASE_URL=postgres://postgres:trial@127.0.0.1:54329/postgres cargo test

# Against the deployment (TRIAL_URL and TRIAL_TOKEN from .env.trials.local)
cargo run --release --example load -- same-base
cargo run --release --example load -- many-notes
cargo run --release --example load -- latency
cargo run --release --example load -- body-limit
pwsh -File poll.ps1
```

Raw output of each run is in `trials/vercel-neon/results/`.
