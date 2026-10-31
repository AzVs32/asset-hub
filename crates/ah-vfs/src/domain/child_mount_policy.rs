/// Whether a driver's mounted instances may contain nested mount points.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildMountPolicy {
    Allow,
    Deny,
}
