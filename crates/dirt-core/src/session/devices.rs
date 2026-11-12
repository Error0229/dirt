//! More devices: approving one, being approved, the device list, the
//! recovery phrase, and resetting or deleting the account.
//!
//! The approval exchange has two sides, [`Join`] on the new device and
//! [`Approval`] on the device that approves it. Each is driven by the
//! interface: it calls `poll` every few seconds while its screen is open,
//! shows the code when there is one, and calls `confirm` or `reject` with
//! the person's answer. That screen is the one place in Dirt that polls,
//! because each device is waiting for the other.

use dirt_proto::api::{
    ChangeKey, DeviceInfo, DeviceKey, DeviceStatus, EnrolmentMessage, ErrorCode, PutRecovery,
    WrappedKey,
};
use uuid::Uuid;

use super::{Session, SessionError, SessionState};
use crate::api::ApiClient;
use crate::crypto::{AccountKey, KEY_VERSION};
use crate::enrolment::{Agreed, Approving, CancelReason, EnrolmentError, Joining, Message};
use crate::key_change::{wrap_account_key, WrapContext};
use crate::recovery::{RecoveryError, RecoveryPhrase};
use crate::secrets::DeviceSecrets;
use crate::sync::worker::Trigger;

pub use dirt_proto::api::MyDevice;

/// Where an approval exchange stands, for the interface to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeStep {
    /// Waiting for the other device to open its screen or to answer.
    Waiting,
    /// Both devices show a code. The person says whether they are the same.
    Compare { code: String },
    /// The person confirmed here. Waiting for the other device to finish.
    Finishing,
    /// The new device has the account key and is approved.
    Done,
    /// One side called it off. The waiting device has been removed from
    /// the account.
    Cancelled(CancelReason),
    /// Another device of the account is already approving this one.
    HandledElsewhere,
}

/// What came of asking to change the account's key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyChange {
    /// The key was changed by this device.
    Changed {
        /// The new key's version.
        key_version: i32,
        /// Notes still stored under the old key after this device's first
        /// sync. They are sent again as devices sync.
        old_key_notes: u64,
    },
    /// Another device changed the key while this one was preparing to.
    ChangedByAnotherDevice {
        /// The version of the key the account has now.
        key_version: i32,
    },
}

/// What became of a request to reset the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetOutcome {
    /// The account was reset. This device is its only device and holds its
    /// new key.
    Done,
    /// A device that is not approved can only ask. The reset happens at
    /// this time, in Unix milliseconds, unless an approved device cancels
    /// it; ask again then.
    Scheduled { effective_at_ms: i64 },
}

fn cancel(exchange: &str, reason: CancelReason) -> Vec<u8> {
    Message::Cancel {
        exchange: exchange.to_owned(),
        reason,
    }
    .encode()
}

// ---- The new device's side ----

enum JoinPhase {
    /// The commitment is sent; no approving device has answered.
    AwaitingKey(Joining),
    /// The keys are exchanged and the code is on screen.
    Comparing(Agreed),
    /// The person said the codes match; the encrypted key is awaited.
    AwaitingAccountKey(Agreed),
    Done,
    Cancelled(CancelReason),
    /// Between two phases, while the old one's state is being consumed.
    Moving,
}

/// The new device's side of an approval exchange.
pub struct Join {
    session: Session,
    api: ApiClient,
    secrets: DeviceSecrets,
    exchange: String,
    /// The number of the last message read.
    cursor: i64,
    /// This device's reveal, until the server has taken it. Sending it is
    /// tried again on every poll, so a request that fails does not leave
    /// the exchange stuck.
    unsent_reveal: Option<Vec<u8>>,
    phase: JoinPhase,
}

impl std::fmt::Debug for Join {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Join")
            .field("exchange", &self.exchange)
            .finish_non_exhaustive()
    }
}

impl Join {
    async fn new_messages(&mut self) -> Result<Vec<(EnrolmentMessage, Message)>, SessionError> {
        let fetched = self
            .api
            .enrolment_messages(self.secrets.device_id, self.cursor)
            .await?;
        let mut mine = Vec::new();
        for relayed in fetched {
            self.cursor = self.cursor.max(relayed.seq);
            if let Some(message) = Message::decode(&relayed.body) {
                if message.exchange() == self.exchange {
                    mine.push((relayed, message));
                }
            }
        }
        Ok(mine)
    }

    /// Sends this device's reveal if the server has not taken it yet.
    /// Sending it twice does no harm: the approving device acts on the
    /// first and passes over the second.
    async fn send_reveal(&mut self) -> Result<(), SessionError> {
        if let Some(reveal) = &self.unsent_reveal {
            self.api
                .post_enrolment_message(self.secrets.device_id, reveal.clone(), false)
                .await?;
            self.unsent_reveal = None;
        }
        Ok(())
    }

    /// Looks for what the other device has sent and moves the exchange on.
    /// The interface calls this every few seconds while the screen is open.
    pub async fn poll(&mut self) -> Result<ExchangeStep, SessionError> {
        match self.look().await {
            // An approving device removed this one. Its message saying why
            // went with the request, so the reason is not known here; the
            // approving device shows it. Any of this device's requests can
            // be the one that brings the news.
            Err(SessionError::Server(error))
                if error.code() == Some(crate::api::ErrorCode::DeviceRevoked) =>
            {
                self.called_off(CancelReason::Declined)
            }
            other => other,
        }
    }

    async fn look(&mut self) -> Result<ExchangeStep, SessionError> {
        if matches!(self.phase, JoinPhase::Done) {
            return Ok(ExchangeStep::Done);
        }
        if let JoinPhase::Cancelled(reason) = self.phase {
            return Ok(ExchangeStep::Cancelled(reason));
        }
        self.send_reveal().await?;

        let messages = self.new_messages().await?;
        for (relayed, message) in messages {
            let from_other = relayed.sender != self.secrets.device_id;
            match message {
                Message::Cancel { reason, .. } if from_other => {
                    return self.called_off(reason);
                }
                Message::Key { .. } if from_other => {
                    // The first approving device to answer is the one this
                    // device goes on with.
                    let JoinPhase::AwaitingKey(_) = self.phase else {
                        continue;
                    };
                    let JoinPhase::AwaitingKey(joining) =
                        std::mem::replace(&mut self.phase, JoinPhase::Moving)
                    else {
                        continue;
                    };
                    match joining.receive_key(relayed.sender, &message) {
                        Ok((agreed, reveal)) => {
                            // The state is kept before anything is sent, so
                            // that a failed request can be tried again.
                            self.unsent_reveal = Some(reveal.encode());
                            self.phase = JoinPhase::Comparing(agreed);
                        }
                        Err(error) => {
                            // A key that cannot be used is a fault or an
                            // interference. Nothing more is accepted.
                            self.reject(CancelReason::Mismatch).await?;
                            return Err(error.into());
                        }
                    }
                }
                _ => {}
            }
        }
        self.send_reveal().await?;

        match &self.phase {
            JoinPhase::AwaitingKey(_) | JoinPhase::Moving => Ok(ExchangeStep::Waiting),
            JoinPhase::Comparing(agreed) => Ok(ExchangeStep::Compare {
                code: agreed.code(),
            }),
            JoinPhase::AwaitingAccountKey(_) => self.fetch_account_key().await,
            JoinPhase::Done => Ok(ExchangeStep::Done),
            JoinPhase::Cancelled(reason) => Ok(ExchangeStep::Cancelled(*reason)),
        }
    }

    /// The person says the two codes are the same.
    pub async fn confirm(&mut self) -> Result<ExchangeStep, SessionError> {
        if let JoinPhase::Comparing(_) = self.phase {
            if let JoinPhase::Comparing(agreed) =
                std::mem::replace(&mut self.phase, JoinPhase::Moving)
            {
                self.phase = JoinPhase::AwaitingAccountKey(agreed);
            }
        }
        self.poll().await
    }

    /// The person says the codes differ, or does not want to go on. This
    /// device signs out, and tells the approving device why; that device
    /// removes it from the account when it reads the message, which keeps
    /// the reason readable until then.
    pub async fn reject(&mut self, reason: CancelReason) -> Result<(), SessionError> {
        let _ = self
            .api
            .post_enrolment_message(
                self.secrets.device_id,
                cancel(&self.exchange, reason),
                false,
            )
            .await;
        self.phase = JoinPhase::Cancelled(reason);
        self.session.forget_account(&self.secrets)
    }

    /// The other device called the exchange off, and has removed this one
    /// from the account. Sign out here to match.
    fn called_off(&mut self, reason: CancelReason) -> Result<ExchangeStep, SessionError> {
        self.phase = JoinPhase::Cancelled(reason);
        self.session.forget_account(&self.secrets)?;
        Ok(ExchangeStep::Cancelled(reason))
    }

    async fn fetch_account_key(&mut self) -> Result<ExchangeStep, SessionError> {
        let Some(wrapped) = self.api.enrolment_key(self.secrets.device_id).await? else {
            return Ok(ExchangeStep::Finishing);
        };
        let JoinPhase::AwaitingAccountKey(agreed) = &self.phase else {
            return Ok(ExchangeStep::Finishing);
        };
        let account_key = match agreed.open_key(&wrapped.wrapped) {
            Ok(key) => key,
            Err(error) => {
                // What was left is not from the device whose code the
                // person compared. Nothing is accepted from it.
                self.reject(CancelReason::Mismatch).await?;
                return Err(error.into());
            }
        };

        // The key is saved before the server is told. If the app stops in
        // between, the device holds the key and asks the server whether it
        // is approved the next time it looks.
        let mut secrets = self.secrets.clone();
        secrets.set_only_key(wrapped.key_version, account_key);
        self.session.store_secrets(&secrets)?;
        self.api.complete_enrolment(secrets.device_id).await?;
        secrets.approved = true;
        self.session.store_secrets(&secrets)?;
        self.session.activate(secrets)?;
        self.phase = JoinPhase::Done;
        Ok(ExchangeStep::Done)
    }
}

// ---- The approving device's side ----

enum ApprovalPhase {
    /// The new device has not opened its screen yet.
    AwaitingCommit,
    /// This device has answered and waits for the new device's key.
    /// `answered` is the number the server gave the answer, once it has
    /// taken it.
    AwaitingReveal {
        approving: Approving,
        answered: Option<i64>,
    },
    /// The code is on screen.
    Comparing(Agreed),
    /// The encrypted key is left; the new device has to fetch it.
    AwaitingCompletion,
    Done,
    Cancelled(CancelReason),
    HandledElsewhere,
    Moving,
}

/// The approving device's side of an approval exchange.
pub struct Approval {
    session: Session,
    api: ApiClient,
    secrets: DeviceSecrets,
    target: DeviceInfo,
    exchange: Option<String>,
    cursor: i64,
    /// This device's answer, until the server has taken it. Sending it is
    /// tried again on every poll.
    unsent_key: Option<Vec<u8>>,
    phase: ApprovalPhase,
}

impl std::fmt::Debug for Approval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Approval")
            .field("device", &self.target.id)
            .finish_non_exhaustive()
    }
}

impl Approval {
    /// The device being approved.
    #[must_use]
    pub const fn device(&self) -> &DeviceInfo {
        &self.target
    }

    /// Sends this device's answer if the server has not taken it yet. The
    /// same bytes are sent each time, so if an earlier try did arrive the
    /// new device sees the same key twice and nothing changes.
    async fn send_key(&mut self) -> Result<(), SessionError> {
        if let Some(key) = &self.unsent_key {
            let posted = self
                .api
                .post_enrolment_message(self.target.id, key.clone(), false)
                .await?;
            self.unsent_key = None;
            if let ApprovalPhase::AwaitingReveal { answered, .. } = &mut self.phase {
                *answered = Some(posted.seq);
            }
        }
        Ok(())
    }

    /// Answers a commitment from the new device: a first one, or a new one
    /// after that device began again. The answer is sent by
    /// [`send_key`](Self::send_key).
    async fn answer(&mut self, commit: &Message) -> Result<(), SessionError> {
        let answered = Approving::answer(
            self.secrets.account_id,
            self.secrets.device_id,
            &self.secrets.device_key.public_bytes(),
            self.target.id,
            &self.target.public_key,
            commit,
        );
        match answered {
            Ok((approving, key)) => {
                self.exchange = Some(approving.exchange().to_owned());
                self.unsent_key = Some(key.encode());
                self.phase = ApprovalPhase::AwaitingReveal {
                    approving,
                    answered: None,
                };
                Ok(())
            }
            Err(error) => {
                self.remove_device(commit.exchange(), CancelReason::Mismatch)
                    .await;
                Err(error.into())
            }
        }
    }

    /// Tells the new device the exchange is off and removes it from the
    /// account.
    async fn remove_device(&mut self, exchange: &str, reason: CancelReason) {
        let _ = self
            .api
            .post_enrolment_message(self.target.id, cancel(exchange, reason), false)
            .await;
        if let Err(error) = self.api.revoke(self.target.id).await {
            tracing::warn!("the waiting device could not be removed: {error}");
        }
        self.phase = ApprovalPhase::Cancelled(reason);
    }

    /// Looks for what the new device has sent and moves the exchange on.
    /// The interface calls this every few seconds while the screen is open.
    pub async fn poll(&mut self) -> Result<ExchangeStep, SessionError> {
        match self.phase {
            ApprovalPhase::Done => return Ok(ExchangeStep::Done),
            ApprovalPhase::Cancelled(reason) => return Ok(ExchangeStep::Cancelled(reason)),
            ApprovalPhase::HandledElsewhere => return Ok(ExchangeStep::HandledElsewhere),
            _ => {}
        }
        self.send_key().await?;

        let fetched = match self
            .api
            .enrolment_messages(self.target.id, self.cursor)
            .await
        {
            Ok(messages) => messages,
            // The request is gone. If this device had left the key, the new
            // device may have fetched it and been approved; the device list
            // says. Otherwise another device approved or removed it, or the
            // request lapsed.
            Err(error) if error.code() == Some(crate::api::ErrorCode::NotFound) => {
                if matches!(self.phase, ApprovalPhase::AwaitingCompletion) {
                    return self.check_completion().await;
                }
                self.phase = ApprovalPhase::HandledElsewhere;
                return Ok(ExchangeStep::HandledElsewhere);
            }
            Err(error) => return Err(error.into()),
        };

        let decoded: Vec<(EnrolmentMessage, Message)> = fetched
            .into_iter()
            .filter_map(|relayed| Message::decode(&relayed.body).map(|message| (relayed, message)))
            .collect();
        for (index, (relayed, message)) in decoded.iter().enumerate() {
            self.cursor = self.cursor.max(relayed.seq);
            let from_new_device = relayed.sender == self.target.id;
            let from_me = relayed.sender == self.secrets.device_id;
            let current = self.exchange.as_deref() == Some(message.exchange());
            match message {
                // A commitment starts an exchange, also when one was under
                // way: the new device began again.
                Message::Commit { .. } if from_new_device => {
                    // If another approved device has answered it already,
                    // the new device goes on with that one.
                    let answered_by_another = decoded[index + 1..].iter().any(|(later, other)| {
                        matches!(other, Message::Key { .. })
                            && other.exchange() == message.exchange()
                            && later.sender != self.secrets.device_id
                            && later.sender != self.target.id
                    });
                    if answered_by_another {
                        self.phase = ApprovalPhase::HandledElsewhere;
                        return Ok(ExchangeStep::HandledElsewhere);
                    }
                    self.answer(message).await?;
                }
                // Another approved device answered this exchange first.
                Message::Key { .. } if !from_me && !from_new_device && current => {
                    if let ApprovalPhase::AwaitingReveal { answered, .. } = &self.phase {
                        // Theirs came first if this device's has no number
                        // yet, or a higher one.
                        if answered.is_none_or(|mine| relayed.seq < mine) {
                            self.unsent_key = None;
                            self.phase = ApprovalPhase::HandledElsewhere;
                            return Ok(ExchangeStep::HandledElsewhere);
                        }
                    }
                }
                Message::Reveal { .. } if from_new_device && current => {
                    let ApprovalPhase::AwaitingReveal { .. } = self.phase else {
                        continue;
                    };
                    let ApprovalPhase::AwaitingReveal { approving, .. } =
                        std::mem::replace(&mut self.phase, ApprovalPhase::Moving)
                    else {
                        continue;
                    };
                    match approving.receive_reveal(message) {
                        Ok(agreed) => self.phase = ApprovalPhase::Comparing(agreed),
                        Err(error) => {
                            // The key shown is not the one committed to:
                            // somebody is between the two devices.
                            self.remove_device(message.exchange(), CancelReason::Mismatch)
                                .await;
                            return Err(error.into());
                        }
                    }
                }
                Message::Cancel { reason, .. } if from_new_device && current => {
                    let reason = *reason;
                    if let Err(error) = self.api.revoke(self.target.id).await {
                        tracing::warn!("the waiting device could not be removed: {error}");
                    }
                    self.phase = ApprovalPhase::Cancelled(reason);
                    return Ok(ExchangeStep::Cancelled(reason));
                }
                _ => {}
            }
        }

        self.send_key().await?;

        // With the key left, the new device has only to fetch it. It may
        // still call the exchange off, which the loop above has handled, or
        // begin again, which has put this device back to answering.
        if matches!(self.phase, ApprovalPhase::AwaitingCompletion) {
            return self.check_completion().await;
        }
        Ok(match &self.phase {
            ApprovalPhase::Comparing(agreed) => ExchangeStep::Compare {
                code: agreed.code(),
            },
            ApprovalPhase::Cancelled(reason) => ExchangeStep::Cancelled(*reason),
            _ => ExchangeStep::Waiting,
        })
    }

    /// The person says the two codes are the same. The account key is
    /// encrypted for the new device and left with the server.
    pub async fn confirm(&mut self) -> Result<ExchangeStep, SessionError> {
        let ApprovalPhase::Comparing(agreed) = &self.phase else {
            return self.poll().await;
        };
        // The key this device holds now, not the one it held when the
        // exchange began: the account's key may have been changed since.
        let keys = self
            .session
            .saved_secrets()
            .and_then(|secrets| secrets.keys())
            .ok_or(SessionError::NotReady)?;
        let wrapped = WrappedKey {
            key_version: keys.version(),
            wrapped: agreed.seal_key(keys.current()),
        };
        self.api.put_enrolment_key(self.target.id, &wrapped).await?;
        self.phase = ApprovalPhase::AwaitingCompletion;
        self.check_completion().await
    }

    /// The person says the codes differ, or does not want this device. It
    /// is removed from the account.
    pub async fn reject(&mut self, reason: CancelReason) {
        let exchange = self.exchange.clone().unwrap_or_default();
        self.remove_device(&exchange, reason).await;
    }

    /// Whether the new device has fetched the key and been approved.
    async fn check_completion(&mut self) -> Result<ExchangeStep, SessionError> {
        let devices = self.api.devices().await?.devices;
        let status = devices
            .iter()
            .find(|device| device.id == self.target.id)
            .map(|device| device.status);
        Ok(match status {
            Some(DeviceStatus::Approved) => {
                self.phase = ApprovalPhase::Done;
                // The new device's notes may be arriving.
                self.session.notify(Trigger::Manual);
                ExchangeStep::Done
            }
            Some(DeviceStatus::Pending) => ExchangeStep::Finishing,
            // The new device found the codes different on its side, and
            // left.
            Some(DeviceStatus::Revoked) | None => {
                self.phase = ApprovalPhase::Cancelled(CancelReason::Mismatch);
                ExchangeStep::Cancelled(CancelReason::Mismatch)
            }
        })
    }
}

impl From<EnrolmentError> for SessionError {
    fn from(error: EnrolmentError) -> Self {
        Self::Enrolment(error)
    }
}

impl From<RecoveryError> for SessionError {
    fn from(error: RecoveryError) -> Self {
        Self::Recovery(error)
    }
}

impl Session {
    fn signed_in_api(&self) -> Result<(ApiClient, DeviceSecrets), SessionError> {
        let secrets = self.saved_secrets().ok_or(SessionError::NotReady)?;
        Ok((self.api().with_token(secrets.token.clone()), secrets))
    }

    fn ready_api(&self) -> Result<(ApiClient, DeviceSecrets), SessionError> {
        let (api, secrets) = self.signed_in_api()?;
        if secrets.is_ready() {
            Ok((api, secrets))
        } else {
            Err(SessionError::NotReady)
        }
    }

    // ---- What the account looks like from here ----

    /// This device as the server sees it: whether it is approved, how many
    /// other devices could approve it or keep the key if it leaves, whether
    /// a recovery copy exists, and whether a reset is pending.
    pub async fn my_device(&self) -> Result<MyDevice, SessionError> {
        let (api, _) = self.signed_in_api()?;
        Ok(api.me().await?)
    }

    /// The account's devices.
    pub async fn devices(&self) -> Result<Vec<DeviceInfo>, SessionError> {
        let (api, _) = self.ready_api()?;
        Ok(api.devices().await?.devices)
    }

    /// Removes a device from the account. It can no longer reach the
    /// server. It keeps the account key it had, so what it already holds it
    /// can still read; only changing the key ends that.
    pub async fn remove_device(&self, device: Uuid) -> Result<(), SessionError> {
        let (api, _) = self.ready_api()?;
        Ok(api.revoke(device).await?)
    }

    // ---- Approving and being approved ----

    /// On a waiting device: starts the exchange that gets it the account
    /// key. An approving device answers when its own screen is open.
    pub async fn begin_join(&self) -> Result<Join, SessionError> {
        let (api, secrets) = self.signed_in_api()?;
        if secrets.is_ready() {
            return Err(SessionError::NotReady);
        }
        let (joining, commit) = Joining::start(
            secrets.account_id,
            secrets.device_id,
            &secrets.device_key.public_bytes(),
        );
        // Beginning again drops whatever an earlier attempt left behind.
        let posted = api
            .post_enrolment_message(secrets.device_id, commit.encode(), true)
            .await?;
        Ok(Join {
            session: self.clone(),
            api,
            secrets,
            exchange: joining.exchange().to_owned(),
            cursor: posted.seq,
            unsent_reveal: None,
            phase: JoinPhase::AwaitingKey(joining),
        })
    }

    /// On an approved device: the devices waiting to join the account.
    pub async fn waiting_devices(&self) -> Result<Vec<DeviceInfo>, SessionError> {
        Ok(self
            .devices()
            .await?
            .into_iter()
            .filter(|device| device.status == DeviceStatus::Pending)
            .collect())
    }

    /// On an approved device: takes part in the exchange for `device`, one
    /// of [`waiting_devices`](Self::waiting_devices).
    pub fn begin_approval(&self, device: DeviceInfo) -> Result<Approval, SessionError> {
        let (api, secrets) = self.ready_api()?;
        Ok(Approval {
            session: self.clone(),
            api,
            secrets,
            target: device,
            exchange: None,
            cursor: 0,
            unsent_key: None,
            phase: ApprovalPhase::AwaitingCommit,
        })
    }

    // ---- Changing the account's key ----

    /// Changes the account's key. Notes written from now on are under a
    /// key that a device removed earlier never held.
    ///
    /// The new key is wrapped for every approved device and left with the
    /// server in one request. Each device, this one included, then gets it
    /// from its own copy at its next sync, and sends the notes it holds
    /// again under it. The recovery phrase stops working, because its copy
    /// holds the old key: make a new one afterwards.
    ///
    /// If another device changed the key first, nothing more is done: the
    /// account has a new key, which is what was asked for.
    ///
    /// An app shows how far a change has got from what its sync worker last
    /// reported. The syncs made here do not pass through the worker, so it
    /// is asked for one more when the change is made, which reports the
    /// new key version and the notes still under the old one.
    pub async fn change_account_key(&self) -> Result<KeyChange, SessionError> {
        // The key this device holds has to be the one being replaced.
        self.sync_now().await?;
        // The devices may change between looking and asking. Twice more is
        // plenty; more than that is not a coincidence.
        for _ in 0..3 {
            let (api, secrets) = self.ready_api()?;
            let keys = secrets.keys().ok_or(SessionError::NotReady)?;
            let new = AccountKey::generate();
            let mut wrapped = Vec::new();
            for device in api.devices().await?.devices {
                if device.status != DeviceStatus::Approved {
                    continue;
                }
                let context = WrapContext {
                    account_id: secrets.account_id,
                    device_id: device.id,
                    key_version: keys.version() + 1,
                };
                let copy = wrap_account_key(&new, keys.current(), &device.public_key, &context)
                    .map_err(|_| SessionError::UnusableDeviceKey {
                        device: device.name.clone(),
                    })?;
                wrapped.push(DeviceKey {
                    device_id: device.id,
                    wrapped: copy,
                });
            }
            let request = ChangeKey {
                from_version: keys.version(),
                keys: wrapped,
            };
            match api.change_key(&request).await {
                Ok(changed) => {
                    // This device takes the new key as the others do, from
                    // its own copy, and begins sending notes again.
                    let report = self.sync_now().await?;
                    self.notify(Trigger::Manual);
                    return Ok(KeyChange::Changed {
                        key_version: changed.key_version,
                        old_key_notes: report.old_key_notes,
                    });
                }
                Err(error) if error.code() == Some(ErrorCode::DevicesChanged) => {}
                Err(error) if error.code() == Some(ErrorCode::KeyChanged) => {
                    let report = self.sync_now().await?;
                    self.notify(Trigger::Manual);
                    return Ok(KeyChange::ChangedByAnotherDevice {
                        key_version: report.key_version,
                    });
                }
                Err(error) => return Err(error.into()),
            }
        }
        Err(SessionError::DevicesKeptChanging)
    }

    // ---- The recovery phrase ----

    /// Makes a recovery phrase and leaves the account key, encrypted under
    /// it, with the server. A phrase made earlier stops working. The phrase
    /// is returned to be shown once; it is kept nowhere.
    pub async fn create_recovery_phrase(&self) -> Result<RecoveryPhrase, SessionError> {
        let (api, secrets) = self.ready_api()?;
        let account_key = secrets.account_key.as_ref().ok_or(SessionError::NotReady)?;
        let phrase = RecoveryPhrase::generate();
        let copy = phrase.seal(secrets.account_id, account_key);
        api.put_recovery(&PutRecovery {
            key_version: secrets.key_version,
            wrapped: copy.wrapped,
            claim_hash: copy.claim_hash,
        })
        .await?;
        Ok(phrase)
    }

    /// Removes the recovery copy. The phrase no longer opens anything.
    pub async fn remove_recovery_phrase(&self) -> Result<(), SessionError> {
        let (api, _) = self.ready_api()?;
        Ok(api.delete_recovery().await?)
    }

    /// On a waiting device: opens the account with its recovery phrase.
    /// The device becomes approved without another device's help.
    pub async fn recover_with_phrase(&self, typed: &str) -> Result<SessionState, SessionError> {
        let (api, mut secrets) = self.signed_in_api()?;
        if secrets.is_ready() {
            return Ok(self.state());
        }
        let phrase = RecoveryPhrase::parse(typed)?;
        let copy = api.recovery().await?.ok_or(SessionError::NoRecoveryCopy)?;
        // A wrong phrase is found out here, on the device, before the
        // server is asked anything.
        let account_key = phrase.open(secrets.account_id, &copy.wrapped)?;

        secrets.set_only_key(copy.key_version, account_key);
        self.store_secrets(&secrets)?;
        api.claim_recovery(phrase.claim_code(secrets.account_id))
            .await?;
        secrets.approved = true;
        self.store_secrets(&secrets)?;
        self.activate(secrets)?;
        Ok(self.state())
    }

    // ---- Resetting and deleting the account ----

    /// Resets the account: the notes on the server, the recovery copy and
    /// every other device go, and this device becomes the account's first
    /// device with a new key. Its own notes are sent again under that key.
    ///
    /// An approved device resets at once. A waiting device does too if the
    /// server holds no notes; otherwise it can only ask, and must ask again
    /// when the wait is over.
    pub async fn reset_account(&self) -> Result<ResetOutcome, SessionError> {
        let (api, secrets) = self.signed_in_api()?;
        let answer = api.reset_account().await?;
        if !answer.done {
            return Ok(ResetOutcome::Scheduled {
                effective_at_ms: answer.effective_at_ms.unwrap_or_default(),
            });
        }
        let mut fresh = DeviceSecrets {
            approved: true,
            ..secrets
        };
        // A reset starts the account's keys again from the first.
        fresh.set_only_key(KEY_VERSION, AccountKey::generate());
        // As at first sign-in: this record is the only copy of the new key.
        self.store_secrets(&fresh)?;
        self.activate(fresh)?;
        Ok(ResetOutcome::Done)
    }

    /// On an approved device: refuses a reset that a waiting device asked
    /// for.
    pub async fn cancel_reset(&self) -> Result<(), SessionError> {
        let (api, _) = self.ready_api()?;
        Ok(api.cancel_reset().await?)
    }

    /// Deletes the account: everything the server holds for it. This
    /// device's copies of the notes are kept, moved back into its local
    /// store, and it is signed out. Other devices are signed out the next
    /// time they reach the server.
    pub async fn delete_account(&self) -> Result<(), SessionError> {
        let (api, secrets) = self.ready_api()?;
        api.delete_account().await?;

        // The notes exist nowhere else now, so they stay on this device.
        let account_store = self.store();
        for _ in 0..5 {
            if account_store.move_notes_into(&self.inner.local).await? == 0 {
                break;
            }
        }
        drop(account_store);
        self.forget_account(&secrets)?;
        Ok(())
    }
}
