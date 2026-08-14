# Games plugin

`azvs.games` contributes a dedicated workspace for `directory:games` and game entries of kind
`directory:games:item`.

The Host only supplies bounded child/resource reads and the generic `create_tree` effect. This
plugin owns the game model: creating a game emits a game directory, `public/`, a generated
`README.md`, and an empty `HASH.md` reserved for a later integrity workflow. The README is the
default content rendered by both the library card and game workspace.

`directory:games:item` inherits from `directory:games`, so the Kind hierarchy reflects that a game
entry belongs to the Games model. It also restricts its direct container through
`allowed_parent_kinds: ["directory:games"]`. The Games Kind declares it as `default_child_kind`, so
new generic direct children and existing generic children present when a Directory becomes Games
are automatically reclassified as game entries. Explicit non-Core child Kinds are preserved.
