//! Running an approval exchange for a screen.
//!
//! The approval screen is the one place in Dirt that polls: each device is
//! waiting for the other. An [`Exchange`] does that polling for an
//! interface. It looks every few seconds, publishes where the exchange
//! stands, and takes the person's answer. It stops when it is dropped,
//! which is what closing the screen does, and after a few minutes in any
//! case, so a screen left open does not poll for ever.

use std::time::Duration;

use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use super::devices::{Approval, ExchangeStep, Join};
use super::SessionError;
use crate::enrolment::CancelReason;

/// What the person says about the codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// The two codes are the same.
    Match,
    /// The two codes differ.
    Mismatch,
    /// The person does not want to go on.
    Decline,
}

/// What a screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeView {
    /// Where the exchange stands.
    Step(ExchangeStep),
    /// The exchange failed. The text says why, and whether someone may be
    /// interfering.
    Failed(String),
    /// Nothing happened for too long, and polling has stopped. The person
    /// can start again.
    TimedOut,
}

impl ExchangeView {
    /// Whether the exchange is over, one way or another.
    #[must_use]
    pub const fn is_over(&self) -> bool {
        !matches!(
            self,
            Self::Step(
                ExchangeStep::Waiting | ExchangeStep::Compare { .. } | ExchangeStep::Finishing
            )
        )
    }
}

/// How often an exchange looks, and for how long.
#[derive(Debug, Clone, Copy)]
pub struct Pace {
    /// The pause between two looks.
    pub every: Duration,
    /// How long the exchange goes on looking with no step forward.
    pub give_up_after: Duration,
}

impl Default for Pace {
    fn default() -> Self {
        Self {
            every: Duration::from_secs(2),
            give_up_after: Duration::from_secs(300),
        }
    }
}

enum Side {
    Join(Join),
    Approval(Approval),
}

impl Side {
    async fn poll(&mut self) -> Result<ExchangeStep, SessionError> {
        match self {
            Self::Join(join) => join.poll().await,
            Self::Approval(approval) => approval.poll().await,
        }
    }

    async fn confirm(&mut self) -> Result<ExchangeStep, SessionError> {
        match self {
            Self::Join(join) => join.confirm().await,
            Self::Approval(approval) => approval.confirm().await,
        }
    }

    async fn reject(&mut self, reason: CancelReason) -> Result<ExchangeStep, SessionError> {
        match self {
            Self::Join(join) => join.reject(reason).await?,
            Self::Approval(approval) => approval.reject(reason).await,
        }
        Ok(ExchangeStep::Cancelled(reason))
    }
}

/// One side of an approval exchange, running for a screen. Dropping it
/// stops the polling.
#[derive(Debug)]
pub struct Exchange {
    answers: mpsc::UnboundedSender<Answer>,
    view: watch::Receiver<ExchangeView>,
    task: JoinHandle<()>,
}

impl Exchange {
    /// Runs the new device's side.
    #[must_use]
    pub fn join(join: Join, pace: Pace) -> Self {
        Self::run(Side::Join(join), pace)
    }

    /// Runs the approving device's side.
    #[must_use]
    pub fn approve(approval: Approval, pace: Pace) -> Self {
        Self::run(Side::Approval(approval), pace)
    }

    fn run(side: Side, pace: Pace) -> Self {
        let (answers, inbox) = mpsc::unbounded_channel();
        let (publish, view) = watch::channel(ExchangeView::Step(ExchangeStep::Waiting));
        let task = tokio::spawn(drive(side, pace, inbox, publish));
        Self {
            answers,
            view,
            task,
        }
    }

    /// Where the exchange stands now.
    #[must_use]
    pub fn view(&self) -> ExchangeView {
        self.view.borrow().clone()
    }

    /// Where the exchange stands, as a value that can be waited on.
    #[must_use]
    pub fn watch(&self) -> watch::Receiver<ExchangeView> {
        self.view.clone()
    }

    /// Passes on what the person said about the codes.
    pub fn answer(&self, answer: Answer) {
        // The task only ends when the exchange is over or this is dropped.
        let _ = self.answers.send(answer);
    }
}

impl Drop for Exchange {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn drive(
    mut side: Side,
    pace: Pace,
    mut inbox: mpsc::UnboundedReceiver<Answer>,
    publish: watch::Sender<ExchangeView>,
) {
    let mut last_progress = tokio::time::Instant::now();
    let mut shown = ExchangeView::Step(ExchangeStep::Waiting);
    // The first look comes straight away; later ones after a pause.
    let mut pause = Duration::ZERO;
    // What the person said, until it has been acted on. If the request it
    // leads to fails for want of a network, it is made again at the next
    // look: the person pressed the button once and is not asked twice.
    let mut unanswered: Option<Answer> = None;
    loop {
        tokio::select! {
            answer = inbox.recv() => match answer {
                Some(answer) => unanswered = Some(answer),
                // The screen is gone.
                None => return,
            },
            () = tokio::time::sleep(pause) => {}
        }
        pause = pace.every;

        let outcome = match unanswered {
            Some(Answer::Match) => side.confirm().await,
            Some(Answer::Mismatch) => side.reject(CancelReason::Mismatch).await,
            Some(Answer::Decline) => side.reject(CancelReason::Declined).await,
            None => side.poll().await,
        };
        let view = match outcome {
            Ok(step) => {
                unanswered = None;
                ExchangeView::Step(step)
            }
            // A request that failed for want of a network is tried again at
            // the next look, the person's answer included. Anything else
            // ends the exchange.
            Err(SessionError::Server(error))
                if error.class() == crate::api::ErrorClass::TryAgainLater =>
            {
                shown.clone()
            }
            Err(error) => ExchangeView::Failed(crate::util::as_sentence(&error)),
        };
        if view != shown {
            last_progress = tokio::time::Instant::now();
            shown = view.clone();
            publish.send_replace(view);
        }
        if shown.is_over() {
            return;
        }
        if last_progress.elapsed() >= pace.give_up_after {
            publish.send_replace(ExchangeView::TimedOut);
            return;
        }
    }
}
