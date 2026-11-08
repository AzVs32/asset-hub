//! Isolated write lifecycle examples with independently observable contents
//! and the evidence available to an adapter.
use ah_vfs_storage::*;
use std::{error::Error, io};

const ORIGINAL: &[u8] = b"old content";
const REPLACEMENT: &[u8] = b"new content";
const TIMEOUT: Failure = Failure::Backend(BackendFailure::TimedOut);
const CONFLICT: Failure = Failure::Rejected(Rejection::Conflict);
const CAPACITY: Failure = Failure::Backend(BackendFailure::CapacityExceeded);
const UNAVAILABLE: Failure = Failure::Backend(BackendFailure::Unavailable);

enum CommitFault {
    ConfirmedRejection(Failure),
    ReplyLost {
        published: bool,
        cleanup_failure: Option<io::Error>,
    },
    PublishPrefix,
}

struct WriteAttempt<'a> {
    target: &'a mut Vec<u8>,
    buffer: Vec<u8>,
    publication: Publication,
}

impl<'a> WriteAttempt<'a> {
    fn new(target: &'a mut Vec<u8>, publication: Publication) -> Self {
        Self {
            target,
            buffer: Vec::new(),
            publication,
        }
    }

    fn stage(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    fn stage_with_capacity_failure(&mut self, prefix: &[u8]) -> StorageError {
        self.stage(prefix);
        StorageError::without_effects(Operation::Write, CAPACITY, None)
    }

    fn abort_with_cleanup_failure(self) -> (Vec<u8>, StorageError) {
        // Cleanup left private bytes behind but this attempt cannot publish.
        (
            self.buffer,
            StorageError::without_effects(Operation::Write, UNAVAILABLE, None),
        )
    }

    fn commit_with_fault(self, fault: CommitFault) -> StorageError {
        match fault {
            CommitFault::ConfirmedRejection(reason) => {
                // The mock backend reports that the commit was fenced out.
                StorageError::without_effects(Operation::Write, reason, None)
            }
            CommitFault::ReplyLost {
                published,
                cleanup_failure,
            } => {
                if published {
                    *self.target = self.buffer;
                }
                // The caller has the same timeout regardless of what the
                // backend actually did.
                StorageError::after_dispatch(
                    Operation::Write,
                    TIMEOUT,
                    cleanup_failure.map(|cause| Box::new(cause) as _),
                )
            }
            CommitFault::PublishPrefix => {
                *self.target = self.buffer[..3].to_vec();
                let reason = match self.publication {
                    Publication::Staged => CAPACITY,
                    Publication::Atomic => Failure::Contract(ContractViolation::CapabilityMismatch),
                };
                StorageError::after_dispatch(Operation::Write, reason, None)
            }
        }
    }
}

fn assert_error(error: StorageError, reason: Failure, outcome: MutationOutcome) {
    assert_eq!(error.operation(), Operation::Write);
    assert_eq!(error.failure(), reason);
    assert_eq!(error.outcome(), outcome);
}

#[test]
fn private_staging_and_failed_cleanup_leave_the_target_unchanged() {
    let mut target = ORIGINAL.to_vec();
    let mut attempt = WriteAttempt::new(&mut target, Publication::Atomic);
    let staging_error = attempt.stage_with_capacity_failure(b"new");
    let (unreclaimed_stage, abort_error) = attempt.abort_with_cleanup_failure();

    assert_error(staging_error, CAPACITY, MutationOutcome::Unchanged);
    assert_error(abort_error, UNAVAILABLE, MutationOutcome::Unchanged);
    assert_eq!(unreclaimed_stage, b"new");
    assert_eq!(target, ORIGINAL);
}

#[test]
fn confirmed_rejection_and_lost_commit_reply_have_different_effect_evidence() {
    for reason in [CONFLICT, TIMEOUT] {
        let mut target = ORIGINAL.to_vec();
        let mut attempt = WriteAttempt::new(&mut target, Publication::Atomic);
        attempt.stage(REPLACEMENT);
        let error = attempt.commit_with_fault(CommitFault::ConfirmedRejection(reason));
        assert_error(error, reason, MutationOutcome::Unchanged);
        assert_eq!(target, ORIGINAL);
    }

    for (published, expected_contents) in [(false, ORIGINAL), (true, REPLACEMENT)] {
        let mut target = ORIGINAL.to_vec();
        let mut attempt = WriteAttempt::new(&mut target, Publication::Atomic);
        attempt.stage(REPLACEMENT);
        let error = attempt.commit_with_fault(CommitFault::ReplyLost {
            published,
            cleanup_failure: (!published).then(|| io::Error::other("private cleanup failed")),
        });
        assert_eq!(error.source().is_some(), !published);
        assert_error(error, TIMEOUT, MutationOutcome::Unknown);
        assert_eq!(target, expected_contents);
    }
}

#[test]
fn visible_partial_publication_requires_reconciliation_under_either_guarantee() {
    for (publication, reason) in [
        (Publication::Staged, CAPACITY),
        (
            Publication::Atomic,
            Failure::Contract(ContractViolation::CapabilityMismatch),
        ),
    ] {
        let mut target = ORIGINAL.to_vec();
        let mut attempt = WriteAttempt::new(&mut target, publication);
        attempt.stage(REPLACEMENT);
        let error = attempt.commit_with_fault(CommitFault::PublishPrefix);
        assert_error(error, reason, MutationOutcome::Unknown);
        assert_eq!(target, b"new");
    }
}
