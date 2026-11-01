;; The single export takes protobuf Request and returns protobuf Reply. All
;; top-level Request fields are length-delimited. Tests inject prepare failures.
(module
  (memory (export "memory") 1)
  (data (i32.const 1024) "@@DESCRIPTION@@")
  (data (i32.const 2048) "@@UNIT@@")
  (global $heap (mut i32) (i32.const 4096))
  (global $initialized (mut i32) (i32.const 0))
  (func (export "cabi_realloc") (param i32 i32 i32) (param $size i32) (result i32)
    (local $pointer i32)
    global.get $heap local.tee $pointer local.get $size i32.add
    i32.const 7 i32.add i32.const -8 i32.and global.set $heap
    local.get $pointer)
  (func $varint (param $pos i32) (result i32 i32)
    (local $value i32) (local $shift i32) (local $byte i32)
    (loop $read
      local.get $pos i32.load8_u local.set $byte
      local.get $value local.get $byte i32.const 127 i32.and
      local.get $shift i32.shl i32.or local.set $value
      local.get $pos i32.const 1 i32.add local.set $pos
      local.get $shift i32.const 7 i32.add local.set $shift
      local.get $byte i32.const 128 i32.and br_if $read)
    local.get $value local.get $pos)
  (func (export "assethub:vfs/driver@2.0.0#call") (param $ptr i32) (param $length i32) (result i32)
    (local $pos i32) (local $tag i32) (local $size i32) (local $op i32)
    i32.const 0 i32.const 1024 i32.store
    i32.const 4 i32.const @@DESCRIPTION_LENGTH@@ i32.store
    i32.const 16 i32.const 2048 i32.store
    i32.const 20 i32.const @@UNIT_LENGTH@@ i32.store
    local.get $ptr local.set $pos
    (block $found
      (loop $field
        local.get $pos local.get $ptr local.get $length i32.add i32.ge_u br_if $found
        local.get $pos call $varint local.set $pos local.set $tag
        local.get $pos call $varint local.set $pos local.set $size
        local.get $tag i32.const 3 i32.shr_u local.tee $op i32.const 10 i32.ge_u br_if $found
        local.get $pos local.get $size i32.add local.set $pos
        br $field))
    local.get $op i32.const 10 i32.eq if i32.const 0 return end
    local.get $op i32.const 11 i32.eq if @@INITIALIZE@@ return end
    local.get $op i32.const 12 i32.eq if @@PREPARE@@ return end
    i32.const 16)
)
