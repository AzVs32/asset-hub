use crate::CoreError;
use crate::domain::Resource;
use crate::port::{ResourceAction, ResourceActionOutput};
use bytes::Bytes;

pub fn execute(
    resource: Resource,
    action: ResourceAction,
    content: Option<Bytes>,
) -> Result<ResourceActionOutput, CoreError> {
    super::media_output(resource, action, content)
}
