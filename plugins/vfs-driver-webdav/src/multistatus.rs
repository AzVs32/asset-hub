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
fn empty(slot: &mut Option<String>) -> Result<()> {
    if slot.is_some() {
        return Err(failure(p::FailureCode::InvalidData));
    }
    *slot = Some(String::new());
    Ok(())
}
fn merge<T: PartialEq>(destination: &mut Option<T>, value: Option<T>) -> Result<()> {
    if let Some(value) = value {
        if destination.as_ref().is_some_and(|old| old != &value) {
            return Err(failure(p::FailureCode::InvalidData));
        }
        *destination = Some(value);
    }
    Ok(())
}
fn append(
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
            return Err(failure(p::FailureCode::TooLarge));
        }
        target.push_str(text);
    } else if stack.is_empty() && !text.trim().is_empty() {
        return Err(failure(p::FailureCode::InvalidData));
    }
    Ok(())
}
pub(super) fn status(value: &str) -> Result<u16> {
    let mut words = value.split_whitespace();
    if !words
        .next()
        .is_some_and(|version| version.starts_with("HTTP/"))
    {
        return Err(failure(p::FailureCode::InvalidData));
    }
    let code: u16 = words
        .next()
        .ok_or_else(|| failure(p::FailureCode::InvalidData))?
        .parse()
        .map_err(|_| failure(p::FailureCode::InvalidData))?;
    if !(100..=599).contains(&code) {
        return Err(failure(p::FailureCode::InvalidData));
    }
    Ok(code)
}

pub(super) fn parse(bytes: &[u8]) -> Result<Vec<Response>> {
    if bytes.len() > p::MAX_DATA_BYTES {
        return Err(failure(p::FailureCode::TooLarge));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| failure(p::FailureCode::InvalidData))?;
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
            .map_err(|_| failure(p::FailureCode::InvalidData))?;
        let dav = match namespace {
            ResolveResult::Bound(namespace) => namespace.as_ref() == b"DAV:",
            ResolveResult::Unbound => false,
            _ => return Err(failure(p::FailureCode::InvalidData)),
        };
        match event {
            Event::Start(start) => {
                if complete || stack.len() >= 64 || scalar(&stack) {
                    return Err(failure(p::FailureCode::InvalidData));
                }
                stack.push(Tag {
                    dav,
                    name: start.local_name().as_ref().to_vec(),
                });
                if stack.len() == 1 && !at(&stack, &[b"multistatus"]) {
                    return Err(failure(p::FailureCode::InvalidData));
                }
                if at(&stack, &[b"multistatus", b"response"]) {
                    response = Some(Response::default());
                } else if at(&stack, &[b"multistatus", b"response", b"href"]) {
                    empty(
                        &mut response
                            .as_mut()
                            .ok_or_else(|| failure(p::FailureCode::InvalidData))?
                            .href,
                    )?;
                } else if at(&stack, &[b"multistatus", b"response", b"status"]) {
                    empty(
                        &mut response
                            .as_mut()
                            .ok_or_else(|| failure(p::FailureCode::InvalidData))?
                            .status,
                    )?;
                } else if at(&stack, &[b"multistatus", b"response", b"propstat"]) {
                    propstat = Some(Propstat::default());
                } else if at(
                    &stack,
                    &[b"multistatus", b"response", b"propstat", b"status"],
                ) {
                    empty(
                        &mut propstat
                            .as_mut()
                            .ok_or_else(|| failure(p::FailureCode::InvalidData))?
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
                        .ok_or_else(|| failure(p::FailureCode::InvalidData))?
                        .properties;
                    match stack[4].name.as_slice() {
                        b"resourcetype" => {
                            if properties.collection.replace(false).is_some() {
                                return Err(failure(p::FailureCode::InvalidData));
                            }
                        }
                        b"getcontentlength" => empty(&mut properties.length)?,
                        b"getlastmodified" => empty(&mut properties.modified)?,
                        b"getetag" => empty(&mut properties.etag)?,
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
                        .ok_or_else(|| failure(p::FailureCode::InvalidData))?
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
                    let fields = propstat
                        .take()
                        .ok_or_else(|| failure(p::FailureCode::InvalidData))?;
                    let code = status(
                        fields
                            .status
                            .as_deref()
                            .ok_or_else(|| failure(p::FailureCode::InvalidData))?,
                    )?;
                    if (200..300).contains(&code) {
                        let target = &mut response
                            .as_mut()
                            .ok_or_else(|| failure(p::FailureCode::InvalidData))?
                            .properties;
                        merge(&mut target.collection, fields.properties.collection)?;
                        merge(&mut target.length, fields.properties.length)?;
                        merge(&mut target.modified, fields.properties.modified)?;
                        merge(&mut target.etag, fields.properties.etag)?;
                        target.other_type |= fields.properties.other_type;
                    }
                } else if at(&stack, &[b"multistatus", b"response"]) {
                    let value = response
                        .take()
                        .ok_or_else(|| failure(p::FailureCode::InvalidData))?;
                    if value.href.is_none() {
                        return Err(failure(p::FailureCode::InvalidData));
                    }
                    if responses.len() >= MAX_RESPONSES {
                        return Err(failure(p::FailureCode::TooLarge));
                    }
                    responses.push(value);
                } else if at(&stack, &[b"multistatus"]) {
                    complete = true;
                }
                stack
                    .pop()
                    .ok_or_else(|| failure(p::FailureCode::InvalidData))?;
            }
            Event::Text(text) => append(
                &stack,
                &mut response,
                &mut propstat,
                &text
                    .xml_content()
                    .map_err(|_| failure(p::FailureCode::InvalidData))?,
            )?,
            Event::CData(text) => append(
                &stack,
                &mut response,
                &mut propstat,
                &text
                    .xml_content()
                    .map_err(|_| failure(p::FailureCode::InvalidData))?,
            )?,
            Event::GeneralRef(reference) => {
                let value = if let Some(ch) = reference
                    .resolve_char_ref()
                    .map_err(|_| failure(p::FailureCode::InvalidData))?
                {
                    ch.to_string()
                } else {
                    match reference
                        .decode()
                        .map_err(|_| failure(p::FailureCode::InvalidData))?
                        .as_ref()
                    {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => return Err(failure(p::FailureCode::InvalidData)),
                    }
                    .into()
                };
                append(&stack, &mut response, &mut propstat, &value)?;
            }
            Event::DocType(_) => return Err(failure(p::FailureCode::InvalidData)),
            Event::Eof => {
                if !complete || !stack.is_empty() || response.is_some() || propstat.is_some() {
                    return Err(failure(p::FailureCode::InvalidData));
                }
                return Ok(responses);
            }
            Event::Decl(_) | Event::Comment(_) | Event::PI(_) => {}
            _ => return Err(failure(p::FailureCode::InvalidData)),
        }
    }
}
