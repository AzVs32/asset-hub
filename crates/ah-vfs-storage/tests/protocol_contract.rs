//! Test-only wire representations for the public conversion contract.
use ah_vfs_storage::*;
use std::{
    error::Error,
    num::NonZeroUsize,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

type TestResult = Result<(), Box<dyn Error>>;

const NANOS_PER_SECOND: i128 = 1_000_000_000;
type WireTime = (i64, u32);

fn encode_time(time: SystemTime) -> Result<WireTime, ContractViolation> {
    let invalid = ContractViolation::InvalidEntry;
    let nanos = match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i128::try_from(duration.as_nanos()).map_err(|_| invalid)?,
        Err(error) => -i128::try_from(error.duration().as_nanos()).map_err(|_| invalid)?,
    };
    Ok((
        i64::try_from(nanos.div_euclid(NANOS_PER_SECOND)).map_err(|_| invalid)?,
        u32::try_from(nanos.rem_euclid(NANOS_PER_SECOND)).map_err(|_| invalid)?,
    ))
}

fn decode_time(wire: WireTime) -> Result<SystemTime, ContractViolation> {
    let invalid = ContractViolation::InvalidEntry;
    let (seconds, nanos) = wire;
    if i128::from(nanos) >= NANOS_PER_SECOND {
        return Err(invalid);
    }
    let total = i128::from(seconds) * NANOS_PER_SECOND + i128::from(nanos);
    let duration = Duration::new(
        u64::try_from(total.abs() / NANOS_PER_SECOND).map_err(|_| invalid)?,
        u32::try_from(total.abs() % NANOS_PER_SECOND).map_err(|_| invalid)?,
    );
    let time = if total < 0 {
        UNIX_EPOCH.checked_sub(duration)
    } else {
        UNIX_EPOCH.checked_add(duration)
    }
    .ok_or(invalid)?;
    if encode_time(time)? != wire {
        return Err(invalid);
    }
    Ok(time)
}

#[test]
fn timestamps_keep_presence_and_signed_subseconds() -> TestResult {
    for wire in [
        None,
        Some((0, 0)),
        Some((-1, 500_000_000)),
        Some((42, 123_456_700)),
    ] {
        let decoded = wire.map(decode_time).transpose()?;
        assert_eq!(decoded.map(encode_time).transpose()?, wire);
    }
    assert_eq!(
        decode_time((0, 1_000_000_000)),
        Err(ContractViolation::InvalidEntry)
    );
    for (wire, host_time) in [
        (
            (i64::MIN, 0),
            UNIX_EPOCH.checked_sub(Duration::from_secs(1 << 63)),
        ),
        (
            (i64::MAX, 0),
            UNIX_EPOCH.checked_add(Duration::from_secs((1 << 63) - 1)),
        ),
        ((0, 1), UNIX_EPOCH.checked_add(Duration::from_nanos(1))),
    ] {
        // Different hosts have different ranges and timestamp precision.
        match host_time.filter(|time| encode_time(*time) == Ok(wire)) {
            Some(time) => assert_eq!(decode_time(wire), Ok(time)),
            None => assert_eq!(decode_time(wire), Err(ContractViolation::InvalidEntry)),
        }
    }
    if let Some(time) = UNIX_EPOCH.checked_add(Duration::from_secs(1 << 63)) {
        assert_eq!(encode_time(time), Err(ContractViolation::InvalidEntry));
    }
    Ok(())
}

fn decode_operation(tag: u8) -> Option<Operation> {
    match tag {
        1 => Some(Operation::Write),
        2 => Some(Operation::Read),
        _ => None,
    }
}

#[derive(Debug, Eq, PartialEq)]
enum RequestDecodeError {
    Operation,
    Path(PathExprError),
    Range(RangeError),
}

fn decode_read_request(
    operation: u8,
    operand: &str,
    offset: u64,
    length: Option<u64>,
) -> Result<ReadRequest, RequestDecodeError> {
    if decode_operation(operation) != Some(Operation::Read) {
        return Err(RequestDecodeError::Operation);
    }
    let path = PathExpr::parse(operand).map_err(RequestDecodeError::Path)?;
    let range = ReadRange::new(offset, length).map_err(RequestDecodeError::Range)?;
    Ok(ReadRequest::new(path, range))
}

#[test]
fn requests_decode_with_checked_width_presence_and_path_types() -> TestResult {
    let nonzero_limit = |wire: u64, maximum: usize| {
        let size = NonZeroUsize::new(usize::try_from(wire).ok()?)?;
        (size.get() <= maximum).then_some(size)
    };
    let page_size = |wire| PageSize::new(nonzero_limit(wire, PageSize::MAX)?.get()).ok();
    let maximum = u64::try_from(PageSize::MAX)?;
    assert_eq!(page_size(maximum).map(PageSize::get), Some(PageSize::MAX));
    for wire in [0, maximum + 1, u64::MAX] {
        assert_eq!(page_size(wire), None);
    }
    let limit = nonzero_limit(4096, 4096).ok_or("valid attribute budget")?;
    assert_eq!(u64::try_from(limit.get())?, 4096);
    assert_eq!(nonzero_limit(4097, 4096), None);
    for wire_length in [None, Some(0), Some(42)] {
        let decoded = decode_read_request(2, "docs", 3, wire_length)
            .map_err(|_| "expected a valid read request")?;
        assert_eq!(
            (decoded.range().offset(), decoded.range().length()),
            (3, wire_length)
        );
    }
    assert_eq!(
        decode_read_request(255, "docs", 3, None),
        Err(RequestDecodeError::Operation)
    );
    assert_eq!(
        decode_read_request(2, "", 3, None),
        Err(RequestDecodeError::Path(PathExprError::Empty))
    );
    assert_eq!(
        decode_read_request(2, "docs", u64::MAX, Some(1)),
        Err(RequestDecodeError::Range(RangeError::Overflow))
    );

    // Separate schema fields select separate parsers.
    for operand in ["/", "/docs//é/", ".././docs", "docs"] {
        let path = PathExpr::parse(operand)?;
        assert_eq!(PathExpr::parse(&path.to_string())?, path);
    }
    for address in ["", "docs/é", "docs/e\u{301}"] {
        let path = StoragePath::parse(address)?;
        assert_eq!(StoragePath::parse(path.as_str())?, path);
    }
    assert!(StoragePath::parse("/").is_err());
    let bytes = [b'.', b'/', 0xff];
    let link = LinkTarget::new(bytes.to_vec());
    assert_eq!(
        LinkTarget::new(link.as_bytes().to_vec()).into_bytes(),
        bytes
    );
    Ok(())
}

fn decode_write_capability(tag: u8) -> Result<Capabilities, ContractViolation> {
    let support = match tag {
        0 => WriteSupport::None,
        1 => WriteSupport::staged(WriteModes::CreateNew, false),
        2 => WriteSupport::staged(WriteModes::Both, true),
        _ => return Err(ContractViolation::InvalidDeclaration),
    };
    Ok(Capabilities::default().with_write(support))
}

#[test]
fn an_unknown_capability_cannot_grant_a_known_guarantee() -> TestResult {
    assert_eq!(
        decode_write_capability(2)?.write(),
        WriteSupport::staged(WriteModes::Both, true)
    );
    assert_eq!(
        decode_write_capability(255),
        Err(ContractViolation::InvalidDeclaration)
    );
    Ok(())
}

// The fixture assigns independent fields to operation, family, reason and effect.
type WireFailure = [u8; 4];

fn encode_failure(error: &StorageError) -> Option<WireFailure> {
    let op = match error.operation() {
        Operation::Write => 1,
        Operation::Read => 2,
        _ => return None,
    };
    let (family, reason) = match error.failure() {
        Failure::Rejected(Rejection::Conflict) => (1, 1),
        Failure::Backend(BackendFailure::TimedOut) => (2, 1),
        Failure::Contract(ContractViolation::InvalidResponse) => (3, 1),
        _ => return None,
    };
    let outcome = match error.outcome() {
        MutationOutcome::Unchanged => 1,
        MutationOutcome::Unknown => 2,
    };
    Some([op, family, reason, outcome])
}

fn decode_failure(
    [op, family, reason, outcome]: WireFailure,
    expected: Operation,
) -> Result<StorageError, ContractViolation> {
    let invalid = ContractViolation::InvalidResponse;
    let operation = decode_operation(op).ok_or(invalid)?;
    if operation != expected {
        return Err(invalid);
    }
    let failure = match (family, reason) {
        (1, 1) => Failure::Rejected(Rejection::Conflict),
        (2, 1) => Failure::Backend(BackendFailure::TimedOut),
        (3, 1) => Failure::Contract(ContractViolation::InvalidResponse),
        _ => return Err(invalid),
    };
    match outcome {
        1 => Ok(StorageError::without_effects(operation, failure, None)),
        2 if operation.may_mutate() => Ok(StorageError::after_dispatch(operation, failure, None)),
        _ => Err(invalid),
    }
}

#[test]
fn failure_roundtrip_keeps_reason_and_effect_and_excludes_private_source() -> TestResult {
    for failure in [
        Failure::Rejected(Rejection::Conflict),
        Failure::Backend(BackendFailure::TimedOut),
        Failure::Contract(ContractViolation::InvalidResponse),
    ] {
        for make_error in [StorageError::without_effects, StorageError::after_dispatch] {
            let error = make_error(
                Operation::Write,
                failure,
                Some(Box::new(std::io::Error::other(
                    "private backend diagnostics",
                ))),
            );
            let same_public_fields = make_error(Operation::Write, failure, None);
            let wire = encode_failure(&error).ok_or("supported failure")?;
            assert_eq!(Some(wire), encode_failure(&same_public_fields));
            let received = decode_failure(wire, Operation::Write)?;
            assert_eq!(received.operation(), error.operation());
            assert_eq!(received.failure(), error.failure());
            assert_eq!(received.outcome(), error.outcome());
            assert!(received.source().is_none());
        }
    }
    assert!(
        encode_failure(&StorageError::without_effects(
            Operation::Write,
            Failure::Rejected(Rejection::AlreadyExists),
            None,
        ))
        .is_none()
    );
    Ok(())
}

#[test]
fn malformed_failure_replies_cannot_report_unchanged_effects() -> TestResult {
    for wire in [
        [0, 2, 1, 1],
        [1, 0, 1, 1],
        [1, 2, 0, 1],
        [1, 2, 1, 0],
        [2, 2, 1, 1],
    ] {
        let Err(reason) = decode_failure(wire, Operation::Write) else {
            return Err("expected an invalid reply".into());
        };
        let error = StorageError::after_dispatch(Operation::Write, Failure::Contract(reason), None);
        assert_eq!(
            error.failure(),
            Failure::Contract(ContractViolation::InvalidResponse)
        );
        assert_eq!(error.outcome(), MutationOutcome::Unknown);
    }
    assert!(decode_failure([2, 2, 1, 2], Operation::Read).is_err());
    assert_eq!(
        decode_failure([2, 2, 1, 1], Operation::Read)?.outcome(),
        MutationOutcome::Unchanged
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum PageEnd<'a> {
    End,
    After(&'a str),
}

#[derive(Clone, Copy)]
struct WirePage<'a> {
    address: &'a str,
    identity: &'a [u8],
    // This fixture covers directory entries and receives borrowed path fields.
    entries: &'a [&'a str],
    completion: Option<PageEnd<'a>>,
}

fn decode_page(
    bound: &BoundListRequest<'_>,
    wire: WirePage<'_>,
) -> Result<DirectoryPage, ContractViolation> {
    let invalid = ContractViolation::InvalidPage;
    if wire.entries.len() > bound.size().get() {
        return Err(invalid);
    }
    let identity = DirectoryIdentity::from_bytes(wire.identity.try_into().map_err(|_| invalid)?);
    let address = StoragePath::parse(wire.address).map_err(|_| invalid)?;
    let next = match wire.completion.ok_or(invalid)? {
        PageEnd::End => None,
        PageEnd::After(name) => Some(EntryName::parse(name).map_err(|_| invalid)?),
    };
    let entries = wire
        .entries
        .iter()
        .map(|path| {
            let path = StoragePath::parse(path).map_err(|_| invalid)?;
            Entry::new(path, EntryType::Directory, None).map_err(|_| invalid)
        })
        .collect::<Result<Vec<_>, _>>()?;
    DirectoryPage::from_parts(bound, &address, identity, entries, next).map_err(|_| invalid)
}

#[test]
fn page_decoding_uses_independent_binding_and_explicit_completion() -> TestResult {
    let request = ListRequest::new(
        ListTarget::Path(PathExpr::parse("/alias/")?),
        PageSize::new(1)?,
    );
    let bound = request.bind_selection(DirectorySelection::from_lookup(
        StoragePath::parse("docs")?,
        DirectoryIdentity::from_bytes([7; 16]),
    ))?;
    let entry = Entry::new(StoragePath::parse("docs/a")?, EntryType::Directory, None)?;
    let original = DirectoryPage::new(&bound, vec![entry], true)?;
    let (directory, entries, cursor) = original.clone().into_parts();
    let identity = directory.identity();
    let paths: Vec<_> = entries.iter().map(|entry| entry.path().as_str()).collect();
    let wire = WirePage {
        address: directory.address().as_str(),
        identity: identity.as_bytes(),
        entries: &paths,
        completion: Some(PageEnd::After(
            cursor.as_ref().ok_or("expected cursor")?.as_str(),
        )),
    };
    assert_eq!(decode_page(&bound, wire)?, original);
    let empty = WirePage {
        entries: &[],
        completion: Some(PageEnd::End),
        ..wire
    };
    assert!(decode_page(&bound, empty)?.entries().is_empty());
    for invalid in [
        WirePage {
            address: "alias",
            ..empty
        },
        WirePage {
            identity: &[8; 16],
            ..empty
        },
        WirePage {
            identity: &[7; 15],
            ..empty
        },
        WirePage {
            completion: None,
            ..empty
        },
        WirePage {
            entries: &["elsewhere/a"],
            ..wire
        },
        WirePage {
            entries: &["docs/a", "docs/b"],
            ..wire
        },
        WirePage {
            completion: Some(PageEnd::After("b")),
            ..wire
        },
    ] {
        assert_eq!(
            decode_page(&bound, invalid),
            Err(ContractViolation::InvalidPage)
        );
    }
    Ok(())
}

const XATTR_FRAME_LIMIT: usize = 1024;
const XATTR_COUNT_LIMIT: usize = 64;

fn read_size(bytes: &mut &[u8]) -> Option<usize> {
    let (header, tail) = bytes.split_first_chunk::<4>()?;
    *bytes = tail;
    usize::try_from(u32::from_be_bytes(*header)).ok()
}

fn decode_names(mut frame: &[u8], limit: NonZeroUsize) -> Result<XattrList, ContractViolation> {
    let invalid = ContractViolation::InvalidAttributes;
    if frame.len() > XATTR_FRAME_LIMIT {
        return Err(invalid);
    }
    let count = read_size(&mut frame).ok_or(invalid)?;
    // Each entry requires a length prefix and at least two logical bytes,
    // counting the terminator. Check before allocating any collection.
    if count > XATTR_COUNT_LIMIT || count > frame.len() / 4 || count > limit.get() / 2 {
        return Err(invalid);
    }
    let mut remaining = limit.get();
    let mut names = Vec::new();
    for _ in 0..count {
        let size = read_size(&mut frame).ok_or(invalid)?;
        if size > XattrName::MAX_BYTES {
            return Err(invalid);
        }
        remaining = remaining
            .checked_sub(size)
            .and_then(|rest| rest.checked_sub(1))
            .ok_or(invalid)?;
        let (bytes, rest) = frame.split_at_checked(size).ok_or(invalid)?;
        frame = rest;
        let name = XattrName::parse(std::str::from_utf8(bytes).map_err(|_| invalid)?)
            .map_err(|_| invalid)?;
        names.push(name);
    }
    if !frame.is_empty() {
        return Err(invalid);
    }
    XattrList::new(names, limit).map_err(|_| invalid)
}

fn names_frame(names: &[&[u8]]) -> Result<Vec<u8>, std::num::TryFromIntError> {
    let mut frame = u32::try_from(names.len())?.to_be_bytes().to_vec();
    for name in names {
        frame.extend(u32::try_from(name.len())?.to_be_bytes());
        frame.extend_from_slice(name);
    }
    Ok(frame)
}

#[test]
fn attribute_decoding_bounds_allocations_then_validates_the_logical_list() -> TestResult {
    let limit = NonZeroUsize::new(5).ok_or("valid attribute budget")?;
    let original = XattrList::new(vec![XattrName::parse("a")?, XattrName::parse("é")?], limit)?;
    let wire_names: Vec<_> = original
        .names()
        .iter()
        .map(|name| name.as_str().as_bytes())
        .collect();
    let frame = names_frame(&wire_names)?;
    assert_eq!(decode_names(&frame, limit)?, original);
    for invalid in [
        u32::MAX.to_be_bytes().to_vec(),
        [1u32.to_be_bytes(), u32::MAX.to_be_bytes()].concat(),
        vec![0; XATTR_FRAME_LIMIT + 1],
        frame[..frame.len() - 1].to_vec(),
        names_frame(&[b"b", b"a"])?,
        names_frame(&[b"a", b"a"])?,
        names_frame(&[b"\xff"])?,
        names_frame(&[b"a", b"abc"])?,
    ] {
        assert_eq!(
            decode_names(&invalid, limit),
            Err(ContractViolation::InvalidAttributes)
        );
    }
    Ok(())
}
