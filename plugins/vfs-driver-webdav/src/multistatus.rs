//! Bounded, namespace-aware DAV multistatus parsing. DTDs and custom entities
//! are rejected; XML is never used to fetch external resources.
use super::*;
use quick_xml::{events::Event, name::ResolveResult, reader::NsReader};

const MAX_RESPONSES: usize = 10_000;
#[derive(Default)]
pub(super) struct Properties {
    pub collection: Option<bool>,
    pub other_type: bool,
    pub length: Option<String>,
    pub modified: Option<String>,
    pub etag: Option<String>,
}
#[derive(Default)]
pub(super) struct Response {
    pub href: Option<String>,
    pub status: Option<String>,
    pub properties: Properties,
}
#[derive(Default)]
struct Propstat {
    status: Option<String>,
    properties: Properties,
}
struct Tag {
    dav: bool,
    name: Vec<u8>,
}

fn at(stack: &[Tag], names: &[&[u8]]) -> bool {
    stack.len() == names.len()
        && stack
            .iter()
            .zip(names)
            .all(|(tag, name)| tag.dav && tag.name.as_slice() == *name)
}
fn scalar(stack: &[Tag]) -> bool {
    at(stack, &[b"multistatus", b"response", b"href"])
        || at(stack, &[b"multistatus", b"response", b"status"])
        || at(
            stack,
            &[b"multistatus", b"response", b"propstat", b"status"],
        )
        || [
            b"getcontentlength".as_slice(),
            b"getlastmodified",
            b"getetag",
        ]
        .iter()
        .any(|name| {
            at(
                stack,
                &[b"multistatus", b"response", b"propstat", b"prop", name],
            )
        })
}
fn empty(context: &api::ErrorContext, slot: &mut Option<String>) -> Result<()> {
    if slot.is_some() {
        return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
    }
    *slot = Some(String::new());
    Ok(())
}
fn merge<T: PartialEq>(
    context: &api::ErrorContext,
    destination: &mut Option<T>,
    value: Option<T>,
) -> Result<()> {
    if let Some(value) = value {
        if destination.as_ref().is_some_and(|old| old != &value) {
            return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
        }
        *destination = Some(value);
    }
    Ok(())
}
fn append(
    context: &api::ErrorContext,
    stack: &[Tag],
    response: &mut Option<Response>,
    propstat: &mut Option<Propstat>,
    text: &str,
) -> Result<()> {
    let target = if at(stack, &[b"multistatus", b"response", b"href"]) {
        response.as_mut().and_then(|r| r.href.as_mut())
    } else if at(stack, &[b"multistatus", b"response", b"status"]) {
        response.as_mut().and_then(|r| r.status.as_mut())
    } else if at(
        stack,
        &[b"multistatus", b"response", b"propstat", b"status"],
    ) {
        propstat.as_mut().and_then(|r| r.status.as_mut())
    } else if at(
        stack,
        &[
            b"multistatus",
            b"response",
            b"propstat",
            b"prop",
            b"getcontentlength",
        ],
    ) {
        propstat.as_mut().and_then(|r| r.properties.length.as_mut())
    } else if at(
        stack,
        &[
            b"multistatus",
            b"response",
            b"propstat",
            b"prop",
            b"getlastmodified",
        ],
    ) {
        propstat
            .as_mut()
            .and_then(|r| r.properties.modified.as_mut())
    } else if at(
        stack,
        &[
            b"multistatus",
            b"response",
            b"propstat",
            b"prop",
            b"getetag",
        ],
    ) {
        propstat.as_mut().and_then(|r| r.properties.etag.as_mut())
    } else {
        None
    };
    if let Some(target) = target {
        if target.len().saturating_add(text.len()) > p::MAX_CONTROL_BYTES {
            return Err(context.failure(Failure::Rejected(Rejection::LimitExceeded)));
        }
        target.push_str(text);
    } else if stack.is_empty() && !text.trim().is_empty() {
        return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
    }
    Ok(())
}
pub(super) fn status(context: &api::ErrorContext, value: &str) -> Result<u16> {
    let mut words = value.split_whitespace();
    if !words
        .next()
        .is_some_and(|version| version.starts_with("HTTP/"))
    {
        return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
    }
    let code: u16 = words
        .next()
        .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::CorruptData)))?
        .parse()
        .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
    if !(100..=599).contains(&code) {
        return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
    }
    Ok(code)
}

pub(super) fn parse(context: &api::ErrorContext, bytes: &[u8]) -> Result<Vec<Response>> {
    if bytes.len() > p::MAX_DATA_BYTES {
        return Err(context.failure(Failure::Rejected(Rejection::LimitExceeded)));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
    let mut reader = NsReader::from_str(text);
    reader.config_mut().expand_empty_elements = true;
    let mut stack: Vec<Tag> = vec![];
    let mut response: Option<Response> = None;
    let mut propstat: Option<Propstat> = None;
    let mut responses = vec![];
    let mut complete = false;
    loop {
        let (namespace, event) = reader
            .read_resolved_event()
            .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
        let dav = match namespace {
            ResolveResult::Bound(namespace) => namespace.as_ref() == b"DAV:",
            ResolveResult::Unbound => false,
            _ => return Err(context.failure(Failure::Backend(BackendFailure::CorruptData))),
        };
        match event {
            Event::Start(start) => {
                if complete || stack.len() >= 64 || scalar(&stack) {
                    return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
                }
                stack.push(Tag {
                    dav,
                    name: start.local_name().as_ref().to_vec(),
                });
                if stack.len() == 1 && !at(&stack, &[b"multistatus"]) {
                    return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
                }
                if at(&stack, &[b"multistatus", b"response"]) {
                    response = Some(Response::default());
                } else if at(&stack, &[b"multistatus", b"response", b"href"]) {
                    empty(
                        context,
                        &mut response
                            .as_mut()
                            .ok_or_else(|| {
                                context.failure(Failure::Backend(BackendFailure::CorruptData))
                            })?
                            .href,
                    )?;
                } else if at(&stack, &[b"multistatus", b"response", b"status"]) {
                    empty(
                        context,
                        &mut response
                            .as_mut()
                            .ok_or_else(|| {
                                context.failure(Failure::Backend(BackendFailure::CorruptData))
                            })?
                            .status,
                    )?;
                } else if at(&stack, &[b"multistatus", b"response", b"propstat"]) {
                    propstat = Some(Propstat::default());
                } else if at(
                    &stack,
                    &[b"multistatus", b"response", b"propstat", b"status"],
                ) {
                    empty(
                        context,
                        &mut propstat
                            .as_mut()
                            .ok_or_else(|| {
                                context.failure(Failure::Backend(BackendFailure::CorruptData))
                            })?
                            .status,
                    )?;
                } else if stack.len() == 5
                    && at(
                        &stack[..4],
                        &[b"multistatus", b"response", b"propstat", b"prop"],
                    )
                    && dav
                {
                    let properties = &mut propstat
                        .as_mut()
                        .ok_or_else(|| {
                            context.failure(Failure::Backend(BackendFailure::CorruptData))
                        })?
                        .properties;
                    match stack[4].name.as_slice() {
                        b"resourcetype" => {
                            if properties.collection.replace(false).is_some() {
                                return Err(
                                    context.failure(Failure::Backend(BackendFailure::CorruptData))
                                );
                            }
                        }
                        b"getcontentlength" => empty(context, &mut properties.length)?,
                        b"getlastmodified" => empty(context, &mut properties.modified)?,
                        b"getetag" => empty(context, &mut properties.etag)?,
                        _ => {}
                    }
                } else if stack.len() == 6
                    && at(
                        &stack[..5],
                        &[
                            b"multistatus",
                            b"response",
                            b"propstat",
                            b"prop",
                            b"resourcetype",
                        ],
                    )
                {
                    let properties = &mut propstat
                        .as_mut()
                        .ok_or_else(|| {
                            context.failure(Failure::Backend(BackendFailure::CorruptData))
                        })?
                        .properties;
                    if dav && stack[5].name.as_slice() == b"collection" {
                        properties.collection = Some(true);
                    } else {
                        properties.other_type = true;
                    }
                }
            }
            Event::End(_) => {
                if at(&stack, &[b"multistatus", b"response", b"propstat"]) {
                    let fields = propstat.take().ok_or_else(|| {
                        context.failure(Failure::Backend(BackendFailure::CorruptData))
                    })?;
                    let code = status(
                        context,
                        fields.status.as_deref().ok_or_else(|| {
                            context.failure(Failure::Backend(BackendFailure::CorruptData))
                        })?,
                    )?;
                    if (200..300).contains(&code) {
                        let target = &mut response
                            .as_mut()
                            .ok_or_else(|| {
                                context.failure(Failure::Backend(BackendFailure::CorruptData))
                            })?
                            .properties;
                        merge(
                            context,
                            &mut target.collection,
                            fields.properties.collection,
                        )?;
                        merge(context, &mut target.length, fields.properties.length)?;
                        merge(context, &mut target.modified, fields.properties.modified)?;
                        merge(context, &mut target.etag, fields.properties.etag)?;
                        target.other_type |= fields.properties.other_type;
                    }
                } else if at(&stack, &[b"multistatus", b"response"]) {
                    let value = response.take().ok_or_else(|| {
                        context.failure(Failure::Backend(BackendFailure::CorruptData))
                    })?;
                    if value.href.is_none() {
                        return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
                    }
                    if responses.len() >= MAX_RESPONSES {
                        return Err(context.failure(Failure::Rejected(Rejection::LimitExceeded)));
                    }
                    responses.push(value);
                } else if at(&stack, &[b"multistatus"]) {
                    complete = true;
                }
                stack.pop().ok_or_else(|| {
                    context.failure(Failure::Backend(BackendFailure::CorruptData))
                })?;
            }
            Event::Text(text) => append(
                context,
                &stack,
                &mut response,
                &mut propstat,
                &text
                    .xml_content()
                    .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))?,
            )?,
            Event::CData(text) => append(
                context,
                &stack,
                &mut response,
                &mut propstat,
                &text
                    .xml_content()
                    .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))?,
            )?,
            Event::GeneralRef(reference) => {
                let value = if let Some(ch) = reference
                    .resolve_char_ref()
                    .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))?
                {
                    ch.to_string()
                } else {
                    match reference
                        .decode()
                        .map_err(|_| {
                            context.failure(Failure::Backend(BackendFailure::CorruptData))
                        })?
                        .as_ref()
                    {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => {
                            return Err(
                                context.failure(Failure::Backend(BackendFailure::CorruptData))
                            );
                        }
                    }
                    .into()
                };
                append(context, &stack, &mut response, &mut propstat, &value)?;
            }
            Event::DocType(_) => {
                return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
            }
            Event::Eof => {
                if !complete || !stack.is_empty() || response.is_some() || propstat.is_some() {
                    return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
                }
                return Ok(responses);
            }
            Event::Decl(_) | Event::Comment(_) | Event::PI(_) => {}
            _ => return Err(context.failure(Failure::Backend(BackendFailure::CorruptData))),
        }
    }
}
