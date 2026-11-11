//! Load script for the deployed trial function.
//!
//! ```text
//! TRIAL_URL=https://<deployment> TRIAL_TOKEN=<secret> \
//!   cargo run --release --example load -- <mode> [--option value]...
//! ```
//!
//! Modes: `health`, `same-base`, `many-notes`, `sequential`, `latency`,
//! `once`, `poll`, `body`, `body-limit`, `blob-limit`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::io::Write as _;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::{Client, Method, StatusCode};
use uuid::Uuid;
use vercel_neon_trial::{PullPage, PushReply};

type AnyError = Box<dyn std::error::Error + Send + Sync>;

#[tokio::main]
async fn main() -> Result<(), AnyError> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().ok_or("missing mode")?;
    let rest: Vec<String> = args.collect();
    let mut options = HashMap::new();
    for pair in rest.chunks(2) {
        let [key, value] = pair else {
            return Err(format!("option {} has no value", pair[0]).into());
        };
        options.insert(key.trim_start_matches("--").to_owned(), value.clone());
    }
    let options = Options(options);

    let base = std::env::var("TRIAL_URL").map_err(|_| "TRIAL_URL is not set")?;
    let token = std::env::var("TRIAL_TOKEN").map_err(|_| "TRIAL_TOKEN is not set")?;
    let trial = Trial {
        client: Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?,
        base: base.trim_end_matches('/').to_owned(),
        token,
    };

    match mode.as_str() {
        "health" => health(&trial).await,
        "same-base" => same_base(&trial, options.number("count", 50)?).await,
        "many-notes" => many_notes(&trial, options.number("count", 500)?).await,
        "sequential" => {
            sequential(
                &trial,
                options.number("count", 200)?,
                options.number("interval-ms", 0)?,
            )
            .await
        }
        "latency" => latency(&trial, options.number("count", 100)?).await,
        "once" => once(&trial).await,
        "poll" => {
            poll(
                &trial,
                options.number("minutes", 120)?,
                options.number("interval-secs", 30)?,
                options.text("out", "poll.csv"),
            )
            .await
        }
        "body" => body(&trial, options.number("bytes", 4_600_000)?).await,
        "body-limit" => body_limit(&trial).await,
        "blob-limit" => blob_limit(&trial).await,
        other => Err(format!("unknown mode {other}").into()),
    }
}

struct Options(HashMap<String, String>);

impl Options {
    fn number(&self, key: &str, default: usize) -> Result<usize, AnyError> {
        match self.0.get(key) {
            Some(text) => Ok(text.parse()?),
            None => Ok(default),
        }
    }

    fn text(&self, key: &str, default: &str) -> String {
        self.0
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.to_owned())
    }
}

#[derive(Clone)]
struct Trial {
    client: Client,
    base: String,
    token: String,
}

/// One finished request. A request that never got a response has status 0
/// and the client error as its body.
struct Reply {
    status: u16,
    total_ms: f64,
    db_ms: Option<f64>,
    deployment: Option<String>,
    instance: Option<String>,
    body: Vec<u8>,
}

impl Reply {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    fn push(&self) -> Option<PushReply> {
        serde_json::from_slice(&self.body).ok()
    }
}

impl Trial {
    async fn send(&self, method: Method, path_and_query: &str, body: Option<Vec<u8>>) -> Reply {
        let mut request = self
            .client
            .request(method, format!("{}{path_and_query}", self.base))
            .header("x-trial-token", &self.token);
        if let Some(body) = body {
            request = request.body(body);
        }
        let started = Instant::now();
        let outcome = async {
            let response = request.send().await?;
            let status = response.status();
            let header = |name: &str| {
                response
                    .headers()
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned)
            };
            let db_ms = header("x-db-ms").and_then(|text| text.parse().ok());
            let deployment = header("x-trial-deployment");
            let instance = header("x-trial-instance");
            let body = response.bytes().await?.to_vec();
            Ok::<_, reqwest::Error>((status, db_ms, deployment, instance, body))
        }
        .await;
        let total_ms = started.elapsed().as_secs_f64() * 1000.0;
        match outcome {
            Ok((status, db_ms, deployment, instance, body)) => Reply {
                status: status.as_u16(),
                total_ms,
                db_ms,
                deployment,
                instance,
                body,
            },
            Err(error) => Reply {
                status: 0,
                total_ms,
                db_ms: None,
                deployment: None,
                instance: None,
                body: format!("client error: {error:?}").into_bytes(),
            },
        }
    }

    async fn new_account(&self) -> Result<Uuid, AnyError> {
        let account = Uuid::new_v4();
        let reply = self
            .send(Method::POST, &format!("/account?id={account}"), None)
            .await;
        if reply.status != StatusCode::NO_CONTENT.as_u16() {
            return Err(format!("account: {} {}", reply.status, reply.text()).into());
        }
        Ok(account)
    }

    async fn push(&self, account: Uuid, note: Uuid, base: i64, body: Vec<u8>) -> Reply {
        self.send(
            Method::POST,
            &format!("/push?account={account}&note={note}&base={base}"),
            Some(body),
        )
        .await
    }

    async fn pull(&self, account: Uuid, after: i64, limit: usize) -> Reply {
        self.send(
            Method::GET,
            &format!("/pull?account={account}&after={after}&limit={limit}"),
            None,
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    // Nearest rank: the smallest value with at least `fraction` of the
    // samples at or below it.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let rank = (fraction * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn summary(label: &str, mut values: Vec<f64>) -> String {
    values.sort_by(f64::total_cmp);
    format!(
        "{label}: n={} min={:.0} p50={:.0} p95={:.0} p99={:.0} max={:.0} ms",
        values.len(),
        values.first().copied().unwrap_or(f64::NAN),
        percentile(&values, 0.50),
        percentile(&values, 0.95),
        percentile(&values, 0.99),
        values.last().copied().unwrap_or(f64::NAN),
    )
}

fn status_counts(replies: &[Reply]) -> BTreeMap<u16, usize> {
    let mut counts = BTreeMap::new();
    for reply in replies {
        *counts.entry(reply.status).or_insert(0) += 1;
    }
    counts
}

/// Bodies of replies that are neither accepted nor stale, grouped, so a
/// prepared-statement error or a platform error page is visible.
fn unexpected(replies: &[Reply]) -> BTreeMap<String, usize> {
    let mut groups = BTreeMap::new();
    for reply in replies {
        if matches!(reply.status, 200 | 204 | 409) {
            continue;
        }
        let mut text = reply.text();
        text.truncate(300);
        *groups
            .entry(format!("{} {text}", reply.status))
            .or_insert(0) += 1;
    }
    groups
}

fn prepared_statement_errors(replies: &[Reply]) -> usize {
    replies
        .iter()
        .filter(|reply| reply.status != 200 && reply.text().contains("prepared statement"))
        .count()
}

fn report(replies: &[Reply]) {
    println!("statuses: {:?}", status_counts(replies));
    println!(
        "prepared-statement errors: {}",
        prepared_statement_errors(replies)
    );
    for (text, count) in unexpected(replies) {
        println!("unexpected x{count}: {text}");
    }
    println!(
        "{}",
        summary("total", replies.iter().map(|r| r.total_ms).collect())
    );
    println!(
        "{}",
        summary("database", replies.iter().filter_map(|r| r.db_ms).collect())
    );
    let mut deployments: BTreeMap<String, usize> = BTreeMap::new();
    for reply in replies {
        if let Some(deployment) = &reply.deployment {
            *deployments.entry(deployment.clone()).or_insert(0) += 1;
        }
    }
    println!("deployments: {deployments:?}");
    let instances: BTreeSet<&String> = replies.iter().filter_map(|r| r.instance.as_ref()).collect();
    println!("function instances: {}", instances.len());
}

async fn join_all(tasks: Vec<tokio::task::JoinHandle<Reply>>) -> Result<Vec<Reply>, AnyError> {
    let mut replies = Vec::with_capacity(tasks.len());
    for task in tasks {
        replies.push(task.await?);
    }
    Ok(replies)
}

// ---------------------------------------------------------------------------
// Modes
// ---------------------------------------------------------------------------

async fn health(trial: &Trial) -> Result<(), AnyError> {
    let reply = trial.send(Method::GET, "/healthz", None).await;
    println!(
        "{} {} ({:.0} ms)",
        reply.status,
        reply.text(),
        reply.total_ms
    );
    Ok(())
}

/// `count` pushes at once to one note, all naming the same base. Exactly one
/// may be accepted. Run once for an existing note and once for a new one.
async fn same_base(trial: &Trial, count: usize) -> Result<(), AnyError> {
    let account = trial.new_account().await?;

    let existing = Uuid::new_v4();
    let first = trial.push(account, existing, 0, b"v1".to_vec()).await;
    let base = first
        .push()
        .and_then(|reply| reply.change_number)
        .ok_or_else(|| format!("first push: {} {}", first.status, first.text()))?;

    for (label, note, base) in [
        ("replace an existing note", existing, base),
        ("create a new note", Uuid::new_v4(), 0),
    ] {
        let started = Instant::now();
        let tasks = (0..count)
            .map(|index| {
                let trial = trial.clone();
                tokio::spawn(async move {
                    trial
                        .push(account, note, base, format!("writer {index}").into_bytes())
                        .await
                })
            })
            .collect();
        let replies = join_all(tasks).await?;
        println!("== {count} concurrent pushes, same base: {label}");
        println!("wall: {:.0} ms", started.elapsed().as_secs_f64() * 1000.0);
        report(&replies);
        let accepted = replies.iter().filter(|r| r.status == 200).count();
        let stale = replies.iter().filter(|r| r.status == 409).count();
        println!(
            "accepted={accepted} stale={stale} -> {}",
            if accepted == 1 && stale == count - 1 {
                "PASS"
            } else {
                "FAIL"
            }
        );
    }

    let page = trial.pull(account, 0, 10).await;
    println!("final state: {}", page.text());
    Ok(())
}

/// `count` pushes at once to different notes in one account. Every push must
/// be accepted and the change numbers must be exactly 1..=count.
async fn many_notes(trial: &Trial, count: usize) -> Result<(), AnyError> {
    let account = trial.new_account().await?;
    let started = Instant::now();
    let tasks = (0..count)
        .map(|index| {
            let trial = trial.clone();
            tokio::spawn(async move {
                trial
                    .push(
                        account,
                        Uuid::new_v4(),
                        0,
                        format!("note {index}").into_bytes(),
                    )
                    .await
            })
        })
        .collect();
    let replies = join_all(tasks).await?;
    println!("== {count} concurrent pushes to different notes");
    println!("wall: {:.0} ms", started.elapsed().as_secs_f64() * 1000.0);
    report(&replies);

    let numbers: Vec<i64> = replies
        .iter()
        .filter_map(|reply| reply.push().and_then(|push| push.change_number))
        .collect();
    let distinct: BTreeSet<i64> = numbers.iter().copied().collect();
    let expected: BTreeSet<i64> = (1..=i64::try_from(count)?).collect();
    println!(
        "accepted={} distinct={} range={:?}..={:?}",
        numbers.len(),
        distinct.len(),
        distinct.first(),
        distinct.last()
    );

    // Read everything back through paged pulls, as a client would.
    let mut pulled = Vec::new();
    let mut after = 0;
    loop {
        let reply = trial.pull(account, after, 200).await;
        let page: PullPage = serde_json::from_slice(&reply.body)
            .map_err(|error| format!("pull: {} {} ({error})", reply.status, reply.text()))?;
        let Some(last) = page.notes.last() else { break };
        after = last.change_number;
        pulled.extend(page.notes.iter().map(|note| note.change_number));
    }
    let increasing = pulled.windows(2).all(|pair| pair[0] < pair[1]);
    println!("pulled={} strictly increasing={increasing}", pulled.len());

    let pass =
        numbers.len() == count && distinct == expected && pulled.len() == count && increasing;
    println!("-> {}", if pass { "PASS" } else { "FAIL" });
    Ok(())
}

/// `count` requests one after another, alternating a push and a pull, with an
/// optional pause between them. Run it across a redeploy.
async fn sequential(trial: &Trial, count: usize, interval_ms: usize) -> Result<(), AnyError> {
    let account = trial.new_account().await?;
    let mut replies = Vec::with_capacity(count);
    let mut order: Vec<String> = Vec::new();
    for index in 0..count {
        let reply = if index % 2 == 0 {
            trial
                .push(
                    account,
                    Uuid::new_v4(),
                    0,
                    format!("seq {index}").into_bytes(),
                )
                .await
        } else {
            trial.pull(account, 0, 5).await
        };
        if let Some(deployment) = &reply.deployment {
            if order.last() != Some(deployment) {
                println!("request {index}: deployment {deployment}");
                order.push(deployment.clone());
            }
        }
        if !matches!(reply.status, 200) {
            println!("request {index}: {} {}", reply.status, reply.text());
        }
        replies.push(reply);
        if interval_ms > 0 {
            tokio::time::sleep(Duration::from_millis(interval_ms as u64)).await;
        }
    }
    println!("== {count} sequential requests");
    report(&replies);

    let page = trial.pull(account, 0, 1).await;
    let page: PullPage = serde_json::from_slice(&page.body)?;
    println!(
        "account counter={:?}, pushes sent={}",
        page.change_counter,
        count.div_ceil(2)
    );
    Ok(())
}

/// Sequential pulls on a warm function and an awake database.
async fn latency(trial: &Trial, count: usize) -> Result<(), AnyError> {
    let account = trial.new_account().await?;
    for index in 0..20 {
        trial
            .push(account, Uuid::new_v4(), 0, vec![7; 2048 + index])
            .await;
    }

    let mut health = Vec::new();
    let mut pulls = Vec::new();
    let mut pushes = Vec::new();
    for index in 0..count {
        health.push(trial.send(Method::GET, "/healthz", None).await);
        pulls.push(trial.pull(account, 0, 100).await);
        pushes.push(
            trial
                .push(account, Uuid::new_v4(), 0, vec![7; 2048 + index])
                .await,
        );
    }
    for (label, replies) in [
        ("healthz (no database)", &health),
        ("pull (two queries)", &pulls),
        ("push (one transaction)", &pushes),
    ] {
        println!("== {label}");
        report(replies);
    }
    Ok(())
}

/// One pull, for timing the first request after an idle period.
async fn once(trial: &Trial) -> Result<(), AnyError> {
    let reply = trial.pull(Uuid::new_v4(), 0, 1).await;
    println!(
        "status={} total={:.0} ms database={:?} ms deployment={:?} body={}",
        reply.status,
        reply.total_ms,
        reply.db_ms,
        reply.deployment,
        reply.text()
    );
    Ok(())
}

/// One pull every `interval_secs` for `minutes`, as a client left open does.
/// Each line of the CSV holds the database start time, which changes if Neon
/// suspended the compute between two polls.
async fn poll(
    trial: &Trial,
    minutes: usize,
    interval_secs: usize,
    out: String,
) -> Result<(), AnyError> {
    let account = trial.new_account().await?;
    let mut file = std::fs::File::create(&out)?;
    writeln!(
        file,
        "unix_time,status,total_ms,db_ms,database_started_at,deployment"
    )?;
    let polls = minutes * 60 / interval_secs;
    let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs as u64));
    let mut starts: BTreeSet<String> = BTreeSet::new();
    let mut replies = Vec::with_capacity(polls);
    for _ in 0..polls {
        ticker.tick().await;
        let reply = trial.pull(account, 0, 100).await;
        let started_at = serde_json::from_slice::<PullPage>(&reply.body)
            .map(|page| format!("{:.3}", page.database_started_at))
            .unwrap_or_default();
        if !started_at.is_empty() {
            starts.insert(started_at.clone());
        }
        let mut line = String::new();
        write!(
            line,
            "{},{},{:.1},{},{},{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            reply.status,
            reply.total_ms,
            reply.db_ms.map(|ms| format!("{ms:.2}")).unwrap_or_default(),
            started_at,
            reply.deployment.clone().unwrap_or_default(),
        )?;
        writeln!(file, "{line}")?;
        file.flush()?;
        replies.push(reply);
    }
    println!("== {polls} polls, one every {interval_secs} s, written to {out}");
    report(&replies);
    println!("distinct database start times: {}", starts.len());
    Ok(())
}

/// One push with a body of exactly `bytes` bytes.
async fn body(trial: &Trial, bytes: usize) -> Result<(), AnyError> {
    let account = trial.new_account().await?;
    let reply = trial.push(account, Uuid::new_v4(), 0, vec![7; bytes]).await;
    let mut text = reply.text();
    text.truncate(400);
    println!(
        "bytes={bytes} status={} {:.0} ms: {text}",
        reply.status, reply.total_ms
    );
    Ok(())
}

/// Finds the largest request body the platform passes to the function, to
/// within 1 KiB. The same note is replaced each time, so the database holds
/// one copy.
async fn body_limit(trial: &Trial) -> Result<(), AnyError> {
    let account = trial.new_account().await?;
    let note = Uuid::new_v4();
    let mut base = 0;
    let mut accepted = 4_000_000;
    let mut rejected = 5_000_000;
    let mut rejection = String::new();

    for bytes in [accepted, rejected] {
        let reply = trial.push(account, note, base, vec![7; bytes]).await;
        println!(
            "bytes={bytes} status={} {:.0} ms",
            reply.status, reply.total_ms
        );
        match (bytes == accepted, reply.status) {
            (true, 200) => base = reply.push().and_then(|r| r.change_number).unwrap_or(base),
            (false, status) if status != 200 => rejection = reply.text(),
            _ => return Err("the starting bounds do not bracket the limit".into()),
        }
    }
    while rejected - accepted > 1024 {
        let bytes = accepted + (rejected - accepted) / 2;
        let reply = trial.push(account, note, base, vec![7; bytes]).await;
        println!(
            "bytes={bytes} status={} {:.0} ms",
            reply.status, reply.total_ms
        );
        if reply.status == 200 {
            accepted = bytes;
            base = reply.push().and_then(|r| r.change_number).unwrap_or(base);
        } else {
            rejected = bytes;
            rejection = format!("{} {}", reply.status, reply.text());
        }
    }
    rejection.truncate(400);
    println!("largest accepted body: {accepted} bytes");
    println!("smallest rejected body: {rejected} bytes");
    println!("rejection: {rejection}");
    // Leave a small row behind.
    trial.push(account, note, base, b"done".to_vec()).await;
    Ok(())
}

/// Finds the largest response body the platform returns, to within 1 KiB.
async fn blob_limit(trial: &Trial) -> Result<(), AnyError> {
    let fetch = |bytes: usize| async move {
        let reply = trial
            .send(Method::GET, &format!("/blob?bytes={bytes}"), None)
            .await;
        let complete = reply.status == 200 && reply.body.len() == bytes;
        println!(
            "bytes={bytes} status={} received={} {:.0} ms",
            reply.status,
            reply.body.len(),
            reply.total_ms
        );
        (complete, reply)
    };

    let mut returned = 4_000_000;
    let mut refused = 8_000_000;
    if !fetch(returned).await.0 {
        return Err("4,000,000 bytes was not returned".into());
    }
    let (complete, _) = fetch(refused).await;
    if complete {
        println!("8,000,000 bytes was returned; the response limit is higher than that");
        return Ok(());
    }
    let mut refusal = String::new();
    while refused - returned > 1024 {
        let bytes = returned + (refused - returned) / 2;
        let (complete, reply) = fetch(bytes).await;
        if complete {
            returned = bytes;
        } else {
            refused = bytes;
            refusal = format!("{} {}", reply.status, reply.text());
        }
    }
    refusal.truncate(400);
    println!("largest returned body: {returned} bytes");
    println!("smallest refused body: {refused} bytes");
    println!("refusal: {refusal}");
    Ok(())
}
